pub(crate) mod backend;
pub(crate) mod bundle;
pub(crate) mod credentials;
pub(crate) mod kubo;
pub(crate) mod kubo_exec;
pub(crate) mod ring_buffer;
pub(crate) mod session;

mod abot_ops;
mod kubo_ops;
mod session_ops;

use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

use self::session::Session;

/// Broadcast events from the engine (sent to all connected WebSocket clients).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type")]
pub enum OutputEvent {
    #[serde(rename = "output")]
    Output { session: String, data: String },

    #[serde(rename = "exit")]
    Exit { session: String, code: u32 },

    #[serde(rename = "session-removed")]
    SessionRemoved { session: String },
}

/// The engine owns sessions, kubos, and abots directly.
pub struct Engine {
    pub(super) sessions: Mutex<HashMap<String, Session>>,
    pub(super) data_dir: PathBuf,
    pub(super) output_tx: broadcast::Sender<OutputEvent>,
    pub(super) agent_env: Mutex<HashMap<String, String>>,
    pub(super) kubos: Mutex<HashMap<String, kubo::Kubo>>,
}

impl Engine {
    /// Initialize the engine: migrate data dir, discover kubos, sync abots,
    /// spawn autosave + idle check background tasks.
    pub async fn new(data_dir: &Path) -> Result<Arc<Self>> {
        let (output_tx, _) = broadcast::channel(4096);

        let mut agent_env = HashMap::new();
        if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
            agent_env.insert("ANTHROPIC_API_KEY".into(), key);
        }

        // Migrate v1 data directory layout to v2
        if let Err(e) = bundle::migrate_data_dir(data_dir) {
            tracing::warn!("data dir migration failed: {}", e);
        }

        // Initialize kubos from existing kubo directories
        let kubos_dir = bundle::resolve_kubos_dir(data_dir);
        if let Err(e) = std::fs::create_dir_all(&kubos_dir) {
            tracing::warn!("failed to create kubos dir: {}", e);
        }
        let mut kubos_map = HashMap::new();
        for (name, _) in kubo::list_kubo_dirs(&kubos_dir) {
            match kubo::Kubo::ensure_kubo_dir(&kubos_dir, &name) {
                Ok(path) => match kubo::new_kubo(name.clone(), path) {
                    Ok(k) => {
                        tracing::info!("discovered kubo '{}'", name);
                        kubos_map.insert(name, k);
                    }
                    Err(e) => tracing::warn!("failed to load kubo '{}': {}", name, e),
                },
                Err(e) => tracing::warn!("failed to ensure kubo '{}': {}", name, e),
            }
        }

        // Sync known abots with kubo manifests
        bundle::sync_known_abots(data_dir);

        let engine = Arc::new(Self {
            sessions: Mutex::new(HashMap::new()),
            data_dir: data_dir.to_path_buf(),
            output_tx,
            agent_env: Mutex::new(agent_env),
            kubos: Mutex::new(kubos_map),
        });

        // Autosave loop
        {
            let engine = engine.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
                interval.tick().await;
                loop {
                    interval.tick().await;
                    engine.autosave().await;
                }
            });
        }

        // Kubo idle check loop
        {
            let engine = engine.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
                interval.tick().await;
                loop {
                    interval.tick().await;
                    engine.idle_check_kubos().await;
                }
            });
        }

        // Container health check loop — detect dead containers and mark sessions as exited
        {
            let engine = engine.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
                interval.tick().await;
                loop {
                    interval.tick().await;
                    engine.health_check_kubos().await;
                }
            });
        }

        Ok(engine)
    }

    /// Subscribe to engine output events.
    pub fn subscribe(&self) -> broadcast::Receiver<OutputEvent> {
        self.output_tx.subscribe()
    }

    /// Save scrollback for all sessions that have a bundle path.
    pub async fn save_all_scrollback(&self) {
        let sessions = self.sessions.lock().await;
        for s in sessions.values() {
            if let Some(ref bp) = s.bundle_path {
                bundle::save_scrollback(bp, &s.get_buffer());
            }
        }
    }

    // ── Internal: session teardown ─────────────────────────

    /// Core teardown: remove session from map, kill backend, broadcast removal.
    pub(super) fn teardown_session(
        sessions: &mut HashMap<String, Session>,
        output_tx: &broadcast::Sender<OutputEvent>,
        name: &str,
    ) -> Option<(Session, Option<String>)> {
        let mut session = sessions.remove(name)?;
        let kubo_name = session.kubo.clone();
        session.backend.kill();
        let _ = output_tx.send(OutputEvent::SessionRemoved {
            session: name.to_string(),
        });
        Some((session, kubo_name))
    }

    /// Decrement the kubo's active session counter.
    pub(super) async fn decrement_kubo(&self, kubo_name: Option<String>) {
        if let Some(kn) = kubo_name {
            let mut kubos = self.kubos.lock().await;
            if let Some(kubo) = kubos.get_mut(&kn) {
                kubo.session_closed();
            }
        }
    }

    // ── Internal: session registration ────────────────────

    /// Register a new session: replace any old one, take the reader,
    /// restore scrollback, and spawn the output relay.
    pub(super) async fn register_session(
        self: &Arc<Self>,
        qualified: &str,
        session: Session,
        kubo_name: &str,
        abot_name: &str,
        bundle_path: Option<&std::path::Path>,
    ) -> String {
        let session_name = session.name.clone();
        let output_tx = self.output_tx.clone();
        let reader_name = session_name.clone();
        let engine = self.clone();

        let mut sessions = self.sessions.lock().await;
        let old_kubo_name = Self::teardown_session(&mut sessions, &self.output_tx, qualified)
            .and_then(|(_, kn)| kn);
        sessions.insert(qualified.to_string(), session);

        let rx = sessions
            .get_mut(qualified)
            .and_then(|s| s.backend.take_reader());
        let name_rx = sessions.get(qualified).map(|s| s.name_tx.subscribe());
        let gen = sessions.get(qualified).map(|s| s.generation).unwrap_or(0);
        drop(sessions);

        self.decrement_kubo(old_kubo_name).await;

        // Restore scrollback unless the backend handles it (e.g. control mode
        // sends the current screen via refresh-client -S).
        let skip_scrollback = {
            let sessions = self.sessions.lock().await;
            sessions
                .get(qualified)
                .map(|s| s.backend.restores_own_scrollback())
                .unwrap_or(false)
        };
        if !skip_scrollback {
            let mut scrollback: Option<String> = None;
            {
                let kubos = self.kubos.lock().await;
                if let Some(k) = kubos.get(kubo_name) {
                    scrollback = capture_tmux_scrollback(k, abot_name).await;
                }
            }
            if scrollback.is_none() {
                if let Some(bp) = bundle_path {
                    scrollback = bundle::load_scrollback(bp);
                }
            }
            if let Some(sb) = scrollback {
                let mut sessions = self.sessions.lock().await;
                if let Some(s) = sessions.get_mut(qualified) {
                    s.buffer.pre_populate(sb);
                }
            }
        }

        if let Some(mut rx) = rx {
            spawn_output_relay(output_tx, engine, name_rx, reader_name, &mut rx, gen);
        }

        session_name
    }
}

/// Capture tmux scrollback for an abot in a kubo container.
async fn capture_tmux_scrollback(kubo: &kubo::Kubo, abot_name: &str) -> Option<String> {
    let container_id = kubo.container_id.as_ref()?;
    let docker = bollard::Docker::connect_with_socket_defaults().ok()?;
    let tmux_name = kubo_exec::tmux_session_name(abot_name);
    kubo_exec::capture_scrollback(&docker, container_id, &tmux_name).await
}

/// Spawn a task that relays output from a session's reader channel to the broadcast.
fn spawn_output_relay(
    output_tx: broadcast::Sender<OutputEvent>,
    engine: Arc<Engine>,
    name_rx: Option<tokio::sync::watch::Receiver<String>>,
    reader_name: String,
    rx: &mut tokio::sync::mpsc::Receiver<String>,
    generation: u64,
) {
    let mut rx = {
        let (_, dummy_rx) = tokio::sync::mpsc::channel(1);
        std::mem::replace(rx, dummy_rx)
    };
    tokio::spawn(async move {
        while let Some(data) = rx.recv().await {
            let current_name = name_rx
                .as_ref()
                .map(|rx| rx.borrow().clone())
                .unwrap_or_else(|| reader_name.clone());

            let is_current = {
                let mut sessions = engine.sessions.lock().await;
                if let Some(s) = sessions.get_mut(&current_name) {
                    if s.generation == generation {
                        s.buffer.push(data.clone());
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            };
            if is_current {
                let _ = output_tx.send(OutputEvent::Output {
                    session: current_name,
                    data,
                });
            }
        }

        let current_name = name_rx
            .as_ref()
            .map(|rx| rx.borrow().clone())
            .unwrap_or_else(|| reader_name.clone());

        let (code, kubo_name) = {
            let mut sessions = engine.sessions.lock().await;
            if let Some(s) = sessions.get_mut(&current_name) {
                if s.generation == generation {
                    let code = s.backend.try_exit_code().unwrap_or(0);
                    s.mark_exited(code);
                    (Some(code), s.kubo.clone())
                } else {
                    (None, None)
                }
            } else {
                (None, None)
            }
        };
        Engine::decrement_kubo(&engine, kubo_name).await;
        if let Some(code) = code {
            let _ = output_tx.send(OutputEvent::Exit {
                session: current_name,
                code,
            });
        }
    });
}
