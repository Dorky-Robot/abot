pub mod backend;
pub mod bundle;
pub mod docker;
pub mod ipc;
pub mod ring_buffer;
pub mod session;

use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::{broadcast, Mutex};

use self::backend::SessionBackend;
use self::ipc::DaemonRequest;
use self::session::Session;

pub struct DaemonState {
    pub sessions: Mutex<HashMap<String, Session>>,
    pub data_dir: PathBuf,
    /// Broadcast channel for session output events (sent to all connected servers)
    pub output_tx: broadcast::Sender<ipc::OutputEvent>,
    /// Client-to-session attachment mapping (clientId → set of session names)
    pub client_attachments: Mutex<HashMap<String, HashSet<String>>>,
    /// Environment variables to inject into agent containers (used by Docker backend).
    /// Wrapped in Mutex so the server can update it at runtime via IPC.
    pub agent_env: Mutex<HashMap<String, String>>,
}

impl DaemonState {
    /// Create a Docker container backend with additional per-session env vars.
    /// `home_bind` specifies the host path to bind-mount as `/home/dev`.
    pub async fn create_backend_with_env(
        &self,
        name: &str,
        cols: u16,
        rows: u16,
        session_env: &HashMap<String, String>,
        home_bind: &Path,
    ) -> anyhow::Result<Box<dyn SessionBackend>> {
        let global_env = self.agent_env.lock().await;
        let mut merged = global_env.clone();
        // Session env overrides global on key conflicts
        merged.extend(session_env.iter().map(|(k, v)| (k.clone(), v.clone())));
        let env: Vec<String> = merged.iter().map(|(k, v)| format!("{k}={v}")).collect();
        let backend = docker::DockerBackend::spawn(name, cols, rows, env, home_bind).await?;
        Ok(Box::new(backend))
    }
}

pub async fn run(data_dir: &Path) -> Result<()> {
    let sock_path = data_dir.join("daemon.sock");
    let pid_path = data_dir.join("daemon.pid");

    // Check if another daemon is already running
    if let Some(pid) = crate::pid::read_live_pid(&pid_path) {
        anyhow::bail!(
            "daemon already running (pid {}). Remove {:?} if stale.",
            pid,
            pid_path
        );
    }

    // Clean up stale socket
    if sock_path.exists() {
        let _ = std::fs::remove_file(&sock_path);
    }

    // Write PID file
    std::fs::write(&pid_path, std::process::id().to_string())?;

    let listener = UnixListener::bind(&sock_path)?;

    // Set socket permissions to 0o600
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o600))?;
    }

    tracing::info!("daemon listening on {:?}", sock_path);

    let (output_tx, _) = broadcast::channel(4096);

    // Log Docker availability (sessions will fail with a clear error if Docker is down)
    if docker::DockerBackend::is_available().await {
        tracing::info!("Docker daemon detected");
    } else {
        tracing::warn!("Docker not available — sessions will fail until Docker is started");
    }

    // Collect environment variables to inject into agent containers
    let mut agent_env = HashMap::new();
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        agent_env.insert("ANTHROPIC_API_KEY".into(), key);
    }

    let state = Arc::new(DaemonState {
        sessions: Mutex::new(HashMap::new()),
        data_dir: data_dir.to_path_buf(),
        output_tx,
        client_attachments: Mutex::new(HashMap::new()),
        agent_env: Mutex::new(agent_env),
    });

    // Autosave loop — every 5 minutes, save dirty sessions with a bundle path
    {
        let state = state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
            interval.tick().await; // first tick is immediate, skip it
            loop {
                interval.tick().await;
                let to_save: Vec<(String, std::path::PathBuf, HashMap<String, String>, String)> = {
                    let sessions = state.sessions.lock().await;
                    sessions
                        .values()
                        .filter(|s| s.dirty && s.bundle_path.is_some() && s.is_alive())
                        .map(|s| {
                            (
                                s.name.clone(),
                                s.bundle_path.clone().unwrap(),
                                s.env.clone(),
                                s.get_buffer(),
                            )
                        })
                        .collect()
                };

                for (name, bundle_path, env, scrollback) in to_save {
                    match bundle::save_bundle(&bundle_path, &name, &env).await {
                        Ok(()) => {
                            bundle::save_scrollback(&bundle_path, &scrollback);
                            let mut sessions = state.sessions.lock().await;
                            if let Some(s) = sessions.get_mut(&name) {
                                s.dirty = false;
                            }
                            tracing::info!("autosave: saved session '{}'", name);
                        }
                        Err(e) => {
                            tracing::error!("autosave: failed to save '{}': {}", name, e);
                        }
                    }
                }
            }
        });
    }

    // Listen for connections, but also handle SIGTERM/SIGINT for graceful shutdown
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;

    loop {
        tokio::select! {
            result = listener.accept() => {
                let (stream, _) = result?;
                let state = state.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(state, stream).await {
                        tracing::error!("daemon connection error: {}", e);
                    }
                });
            }
            _ = sigterm.recv() => {
                tracing::info!("daemon received SIGTERM, saving scrollback…");
                save_all_scrollback(&state).await;
                break;
            }
            _ = sigint.recv() => {
                tracing::info!("daemon received SIGINT, saving scrollback…");
                save_all_scrollback(&state).await;
                break;
            }
        }
    }

    // Clean up PID file and socket
    let _ = std::fs::remove_file(&pid_path);
    let _ = std::fs::remove_file(&sock_path);
    Ok(())
}

/// Save scrollback for all sessions that have a bundle path.
async fn save_all_scrollback(state: &DaemonState) {
    let sessions = state.sessions.lock().await;
    for s in sessions.values() {
        if let Some(ref bp) = s.bundle_path {
            bundle::save_scrollback(bp, &s.get_buffer());
        }
    }
}

async fn handle_connection(state: Arc<DaemonState>, stream: tokio::net::UnixStream) -> Result<()> {
    let (reader, writer) = stream.into_split();
    let reader = BufReader::new(reader);
    let writer = Arc::new(Mutex::new(writer));

    // Spawn a task to relay output events to this connection
    let mut output_rx = state.output_tx.subscribe();
    let relay_writer = writer.clone();
    let relay_handle = tokio::spawn(async move {
        loop {
            match output_rx.recv().await {
                Ok(event) => {
                    let json = match serde_json::to_string(&event) {
                        Ok(j) => j,
                        Err(_) => continue,
                    };
                    let mut w = relay_writer.lock().await;
                    if w.write_all(json.as_bytes()).await.is_err() {
                        break;
                    }
                    if w.write_all(b"\n").await.is_err() {
                        break;
                    }
                    let _ = w.flush().await;
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    let mut lines = reader.lines();
    while let Some(line) = lines.next_line().await? {
        if line.is_empty() {
            continue;
        }

        match serde_json::from_str::<DaemonRequest>(&line) {
            Ok(req) => {
                let response = ipc::handle_request(&state, req).await;
                if let Some(resp) = response {
                    let json = serde_json::to_string(&resp)?;
                    let mut w = writer.lock().await;
                    w.write_all(json.as_bytes()).await?;
                    w.write_all(b"\n").await?;
                    w.flush().await?;
                }
            }
            Err(e) => {
                tracing::warn!("invalid daemon request: {} — {}", e, line);
            }
        }
    }

    relay_handle.abort();
    Ok(())
}
