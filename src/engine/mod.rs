pub mod backend;
pub mod bundle;
pub mod kubo;
pub mod kubo_exec;
pub mod ring_buffer;
pub mod session;

use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

use self::backend::SessionBackend;
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
/// Replaces DaemonState + ipc.rs handler.
pub struct Engine {
    pub sessions: Mutex<HashMap<String, Session>>,
    pub data_dir: PathBuf,
    pub output_tx: broadcast::Sender<OutputEvent>,
    pub client_attachments: Mutex<HashMap<String, HashSet<String>>>,
    pub agent_env: Mutex<HashMap<String, String>>,
    pub kubos: Mutex<HashMap<String, kubo::Kubo>>,
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
        let _ = std::fs::create_dir_all(&kubos_dir);
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
            client_attachments: Mutex::new(HashMap::new()),
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

        Ok(engine)
    }

    /// Subscribe to engine output events.
    pub fn subscribe(&self) -> broadcast::Receiver<OutputEvent> {
        self.output_tx.subscribe()
    }

    // ── Session methods ─────────────────────────────────────────

    pub async fn list_sessions(&self) -> Vec<serde_json::Value> {
        let sessions = self.sessions.lock().await;
        sessions.values().map(|s| s.to_json()).collect()
    }

    pub async fn get_session(&self, name: &str) -> Result<serde_json::Value> {
        let sessions = self.sessions.lock().await;
        match sessions.get(name) {
            Some(s) => Ok(s.to_json()),
            None => anyhow::bail!("session '{}' not found", name),
        }
    }

    pub async fn create_session(
        self: &Arc<Self>,
        name: String,
        cols: u16,
        rows: u16,
        env: HashMap<String, String>,
        kubo: String,
    ) -> Result<String> {
        // Ensure canonical abot + worktree + kubo manifest
        let abot_dir = self.ensure_abot_in_kubo(&name, &kubo).await?;

        // Kubo-qualified session key: "abot@kubo"
        let qualified = format!("{}@{}", name, kubo);

        let backend_result = self
            .create_kubo_backend(&kubo, &name, cols, rows, &env)
            .await;
        let bundle_path = Some(abot_dir);

        match backend_result {
            Ok(backend) => {
                // Write initial manifest
                if let Some(ref bp) = bundle_path {
                    if let Err(e) = bundle::save_bundle(bp, &name, &env).await {
                        tracing::warn!("failed to write initial manifest for '{}': {}", name, e);
                    }
                }

                let kubo_name_for_scrollback = kubo.clone();
                let session = Session::new(
                    qualified.clone(),
                    backend,
                    env,
                    bundle_path.clone(),
                    Some(kubo),
                );
                let session_name = session.name.clone();

                let output_tx = self.output_tx.clone();
                let reader_name = session_name.clone();
                let engine = self.clone();

                let mut sessions = self.sessions.lock().await;
                let old_kubo_name = if let Some(mut old) = sessions.remove(&qualified) {
                    let kn = if old.is_alive() {
                        old.kubo.clone()
                    } else {
                        None
                    };
                    old.backend.kill();
                    kn
                } else {
                    None
                };
                sessions.insert(qualified.clone(), session);

                let rx = sessions
                    .get_mut(&qualified)
                    .and_then(|s| s.backend.take_reader());
                let shared_name = sessions.get(&qualified).map(|s| s.shared_name.clone());
                let gen = sessions.get(&qualified).map(|s| s.generation).unwrap_or(0);
                drop(sessions);

                if let Some(kn) = old_kubo_name {
                    let mut kubos = self.kubos.lock().await;
                    if let Some(kubo) = kubos.get_mut(&kn) {
                        kubo.session_closed();
                    }
                }

                // Restore scrollback
                {
                    let mut scrollback: Option<String> = None;
                    {
                        let kubos = self.kubos.lock().await;
                        if let Some(k) = kubos.get(&kubo_name_for_scrollback) {
                            scrollback = capture_tmux_scrollback(k, &name).await;
                        }
                    }
                    if scrollback.is_none() {
                        if let Some(ref bp) = bundle_path {
                            scrollback = bundle::load_scrollback(bp);
                        }
                    }
                    if let Some(sb) = scrollback {
                        let mut sessions = self.sessions.lock().await;
                        if let Some(s) = sessions.get_mut(&qualified) {
                            s.buffer.pre_populate(sb);
                        }
                    }
                }

                if let Some(mut rx) = rx {
                    spawn_output_relay(output_tx, engine, shared_name, reader_name, &mut rx, gen);
                }

                Ok(session_name)
            }
            Err(e) => anyhow::bail!("failed to create session: {}", e),
        }
    }

    pub async fn attach(&self, client_id: &str, session: &str) -> Result<String> {
        {
            let mut attachments = self.client_attachments.lock().await;
            attachments
                .entry(client_id.to_string())
                .or_default()
                .insert(session.to_string());
        }

        let sessions = self.sessions.lock().await;
        match sessions.get(session) {
            Some(s) => Ok(s.get_buffer()),
            None => anyhow::bail!("session '{}' not found", session),
        }
    }

    pub async fn delete_session(&self, name: &str) -> Result<()> {
        let (bundle_path, kubo_name) = {
            let mut sessions = self.sessions.lock().await;
            if let Some(mut session) = sessions.remove(name) {
                let bp = session.bundle_path.clone();
                let kn = if session.is_alive() {
                    session.kubo.clone()
                } else {
                    None
                };
                session.backend.kill();
                let _ = self.output_tx.send(OutputEvent::SessionRemoved {
                    session: name.to_string(),
                });
                (bp, kn)
            } else {
                anyhow::bail!("session '{}' not found", name);
            }
        };
        if let Some(kn) = kubo_name {
            let mut kubos = self.kubos.lock().await;
            if let Some(kubo) = kubos.get_mut(&kn) {
                kubo.session_closed();
            }
        }
        if let Some(bp) = bundle_path {
            let _ = std::fs::remove_dir_all(&bp);
        }
        Ok(())
    }

    pub async fn rename_session(&self, old_name: &str, new_name: &str) -> Result<()> {
        kubo::validate_name(new_name)?;
        let mut sessions = self.sessions.lock().await;
        if sessions.contains_key(new_name) {
            anyhow::bail!("session '{}' already exists", new_name);
        }
        if let Some(mut session) = sessions.remove(old_name) {
            session.name = new_name.to_string();
            *session.shared_name.lock().unwrap() = new_name.to_string();

            if let Some(ref bp) = session.bundle_path {
                let manifest_path = bp.join("manifest.json");
                if manifest_path.exists() {
                    if let Ok(mut manifest) = bundle::read_json(&manifest_path) {
                        manifest["name"] = serde_json::Value::String(new_name.to_string());
                        let _ = bundle::write_json(&manifest_path, &manifest);
                    }
                }
            }

            sessions.insert(new_name.to_string(), session);
            drop(sessions);

            let mut attachments = self.client_attachments.lock().await;
            for (_client_id, attached_sessions) in attachments.iter_mut() {
                if attached_sessions.remove(old_name) {
                    attached_sessions.insert(new_name.to_string());
                }
            }
            Ok(())
        } else {
            anyhow::bail!("session '{}' not found", old_name);
        }
    }

    pub async fn write_input(&self, session: &str, data: &str) -> Result<()> {
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get_mut(session) {
            if session.is_alive() {
                session.write(data.as_bytes())?;
            } else {
                anyhow::bail!("session is not alive");
            }
        } else {
            anyhow::bail!("session not found");
        }
        Ok(())
    }

    pub async fn resize(&self, session: &str, cols: u16, rows: u16) {
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get_mut(session) {
            let _ = session.resize(cols, rows);
        }
    }

    pub async fn detach(&self, client_id: &str, session: Option<&str>) {
        let mut attachments = self.client_attachments.lock().await;
        if let Some(session_name) = session {
            if let Some(sessions) = attachments.get_mut(client_id) {
                sessions.remove(session_name);
                if sessions.is_empty() {
                    attachments.remove(client_id);
                }
            }
        } else {
            attachments.remove(client_id);
        }
    }

    // ── Env methods ─────────────────────────────────────────────

    pub async fn update_agent_env(&self, env: HashMap<String, Option<String>>) {
        let mut agent_env = self.agent_env.lock().await;
        for (key, value) in &env {
            match value {
                Some(val) => {
                    agent_env.insert(key.clone(), val.clone());
                }
                None => {
                    agent_env.remove(key);
                }
            }
        }
        tracing::info!("agent_env updated ({} entries)", agent_env.len());

        let sessions = self.sessions.lock().await;
        let snapshot = agent_env.clone();
        for session in sessions.values() {
            session.backend.inject_env(&snapshot);
        }
    }

    pub async fn update_session_env(
        &self,
        session_name: &str,
        env: HashMap<String, Option<String>>,
    ) -> Result<()> {
        let global_env = self.agent_env.lock().await;
        let global_snapshot = global_env.clone();
        drop(global_env);

        let mut sessions = self.sessions.lock().await;
        if let Some(s) = sessions.get_mut(session_name) {
            for (key, value) in &env {
                match value {
                    Some(val) => {
                        s.env.insert(key.clone(), val.clone());
                    }
                    None => {
                        s.env.remove(key);
                    }
                }
            }
            s.dirty = true;
            tracing::info!(
                "session '{}' env updated ({} entries)",
                session_name,
                s.env.len()
            );

            let mut merged = global_snapshot;
            merged.extend(s.env.clone());
            s.backend.inject_env(&merged);

            Ok(())
        } else {
            anyhow::bail!("session '{}' not found", session_name);
        }
    }

    // ── Bundle methods ──────────────────────────────────────────

    pub async fn open_bundle(
        self: &Arc<Self>,
        path: &str,
        cols: u16,
        rows: u16,
        kubo_name: &str,
    ) -> Result<(String, String)> {
        let opened = bundle::open_bundle(path).await?;
        let name = opened.name.clone();
        let canonical_path = opened.path.clone();

        let worktree_path = self
            .add_abot_to_kubo_worktree(&canonical_path, &name, kubo_name)
            .await?;

        let qualified = format!("{}@{}", name, kubo_name);

        let result = self
            .create_kubo_backend(kubo_name, &name, cols, rows, &opened.env)
            .await;

        match result {
            Ok(backend) => {
                let session = Session::new(
                    qualified.clone(),
                    backend,
                    opened.env.clone(),
                    Some(worktree_path.clone()),
                    Some(kubo_name.to_string()),
                );
                let session_name = session.name.clone();

                let output_tx = self.output_tx.clone();
                let reader_name = session_name.clone();
                let engine = self.clone();

                let mut sessions = self.sessions.lock().await;
                let old_kubo_name = if let Some(mut old) = sessions.remove(&qualified) {
                    let kn = if old.is_alive() {
                        old.kubo.clone()
                    } else {
                        None
                    };
                    old.backend.kill();
                    let _ = self.output_tx.send(OutputEvent::SessionRemoved {
                        session: qualified.clone(),
                    });
                    kn
                } else {
                    None
                };
                sessions.insert(qualified.clone(), session);

                let rx = sessions
                    .get_mut(&qualified)
                    .and_then(|s| s.backend.take_reader());
                let shared_name = sessions.get(&qualified).map(|s| s.shared_name.clone());
                let gen = sessions.get(&qualified).map(|s| s.generation).unwrap_or(0);
                drop(sessions);

                if let Some(kn) = old_kubo_name {
                    let mut kubos = self.kubos.lock().await;
                    if let Some(kubo) = kubos.get_mut(&kn) {
                        kubo.session_closed();
                    }
                }

                // Restore scrollback
                {
                    let mut scrollback: Option<String> = None;
                    {
                        let kubos = self.kubos.lock().await;
                        if let Some(k) = kubos.get(kubo_name) {
                            scrollback = capture_tmux_scrollback(k, &name).await;
                        }
                    }
                    if scrollback.is_none() {
                        scrollback = bundle::load_scrollback(&worktree_path);
                    }
                    if let Some(sb) = scrollback {
                        let mut sessions = self.sessions.lock().await;
                        if let Some(s) = sessions.get_mut(&qualified) {
                            s.buffer.pre_populate(sb);
                        }
                    }
                }

                // Inject credentials
                if !opened.env.is_empty() {
                    let global_env = self.agent_env.lock().await;
                    let mut merged = global_env.clone();
                    merged.extend(opened.env);
                    let sessions = self.sessions.lock().await;
                    if let Some(s) = sessions.get(&qualified) {
                        s.backend.inject_env(&merged);
                    }
                }

                if let Some(mut rx) = rx {
                    spawn_output_relay(output_tx, engine, shared_name, reader_name, &mut rx, gen);
                }

                bundle::add_known_abot(&self.data_dir, &name);

                Ok((session_name, worktree_path.to_string_lossy().to_string()))
            }
            Err(e) => anyhow::bail!("failed to create session from bundle: {}", e),
        }
    }

    pub async fn save_session(&self, session_name: &str) -> Result<String> {
        let sessions = self.sessions.lock().await;
        if let Some(s) = sessions.get(session_name) {
            let bundle_path = match &s.bundle_path {
                Some(p) => p.clone(),
                None => anyhow::bail!(
                    "session '{}' has no bundle path (use save-as)",
                    session_name
                ),
            };
            let env = s.env.clone();
            let name = s.name.clone();
            let scrollback = s.get_buffer();
            drop(sessions);

            // Use the abot name (before @) for the bundle manifest
            let bundle_name = name.split('@').next().unwrap_or(&name);
            bundle::save_bundle(&bundle_path, bundle_name, &env).await?;
            bundle::save_scrollback(&bundle_path, &scrollback);

            let mut sessions = self.sessions.lock().await;
            if let Some(s) = sessions.get_mut(session_name) {
                s.dirty = false;
            }
            Ok(bundle_path.to_string_lossy().to_string())
        } else {
            anyhow::bail!("session '{}' not found", session_name);
        }
    }

    pub async fn save_session_as(&self, session_name: &str, path: &str) -> Result<String> {
        // Reject save paths inside another .abot bundle
        {
            let mut check = Path::new(path);
            while let Some(parent) = check.parent() {
                if parent
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("abot"))
                {
                    anyhow::bail!(
                        "cannot save inside another .abot bundle: {}",
                        parent.display()
                    );
                }
                check = parent;
            }
        }

        let sessions = self.sessions.lock().await;
        if let Some(s) = sessions.get(session_name) {
            let env = s.env.clone();
            let name = s.name.clone();
            let existing_bundle = s.bundle_path.clone();
            let scrollback = s.get_buffer();
            drop(sessions);

            // Use the abot name (before @) for the bundle manifest
            let bundle_name = name.split('@').next().unwrap_or(&name);

            let new_bundle_path = PathBuf::from(path);

            if let Some(ref src) = existing_bundle {
                bundle::copy_dir_recursive(src, &new_bundle_path)?;
            }

            bundle::save_bundle(&new_bundle_path, bundle_name, &env).await?;
            bundle::save_scrollback(&new_bundle_path, &scrollback);

            let mut sessions = self.sessions.lock().await;
            if let Some(s) = sessions.get_mut(session_name) {
                s.bundle_path = Some(new_bundle_path.clone());
                s.dirty = false;
            }
            Ok(new_bundle_path.to_string_lossy().to_string())
        } else {
            anyhow::bail!("session '{}' not found", session_name);
        }
    }

    pub async fn close_session(&self, session_name: &str, save: bool) -> Result<()> {
        if save {
            let sessions = self.sessions.lock().await;
            if let Some(s) = sessions.get(session_name) {
                if let Some(bundle_path) = &s.bundle_path {
                    let bundle_path = bundle_path.clone();
                    let env = s.env.clone();
                    let name = s.name.clone();
                    drop(sessions);
                    let bundle_name = name.split('@').next().unwrap_or(&name);
                    bundle::save_bundle(&bundle_path, bundle_name, &env).await?;
                } else {
                    anyhow::bail!(
                        "session '{}' has no bundle path (use save-as before close with save)",
                        session_name
                    );
                }
            } else {
                anyhow::bail!("session '{}' not found", session_name);
            }
        }

        let mut sessions = self.sessions.lock().await;
        if let Some(mut s) = sessions.remove(session_name) {
            if let Some(ref bp) = s.bundle_path {
                bundle::save_scrollback(bp, &s.get_buffer());
            }
            let kubo_name = if s.is_alive() { s.kubo.clone() } else { None };
            s.backend.kill();
            drop(sessions);
            if let Some(kn) = kubo_name {
                let mut kubos = self.kubos.lock().await;
                if let Some(kubo) = kubos.get_mut(&kn) {
                    kubo.session_closed();
                }
            }
            let _ = self.output_tx.send(OutputEvent::SessionRemoved {
                session: session_name.to_string(),
            });
            Ok(())
        } else {
            anyhow::bail!("session '{}' not found", session_name);
        }
    }

    // ── Kubo methods ────────────────────────────────────────────

    pub async fn list_kubos(&self) -> Vec<serde_json::Value> {
        let kubos = self.kubos.lock().await;
        let mut list = Vec::with_capacity(kubos.len());
        for k in kubos.values() {
            list.push(k.to_json().await);
        }
        list
    }

    pub async fn create_kubo(&self, name: &str) -> Result<String> {
        let kubos_dir = bundle::resolve_kubos_dir(&self.data_dir);
        let kubo_path = kubo::Kubo::ensure_kubo_dir(&kubos_dir, name)?;
        let new_kubo = kubo::new_kubo(name.to_string(), kubo_path.clone())?;
        let mut kubos = self.kubos.lock().await;
        kubos.insert(name.to_string(), new_kubo);
        Ok(kubo_path.to_string_lossy().to_string())
    }

    pub async fn start_kubo(&self, name: &str) -> Result<()> {
        let mut kubos = self.kubos.lock().await;
        if let Some(kubo) = kubos.get_mut(name) {
            kubo.start().await?;
            Ok(())
        } else {
            anyhow::bail!("kubo '{}' not found", name);
        }
    }

    pub async fn stop_kubo(&self, name: &str) -> Result<()> {
        let mut kubos = self.kubos.lock().await;
        if let Some(kubo) = kubos.get_mut(name) {
            kubo.stop().await?;
            Ok(())
        } else {
            anyhow::bail!("kubo '{}' not found", name);
        }
    }

    pub async fn open_kubo(&self, path: &str) -> Result<String> {
        let kubo_path = PathBuf::from(path);
        if !kubo_path.exists() || !kubo_path.is_dir() {
            anyhow::bail!("kubo path does not exist: {}", path);
        }

        let dir_name = kubo_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let name = dir_name
            .strip_suffix(".kubo")
            .unwrap_or(dir_name)
            .to_string();

        if name.is_empty() {
            anyhow::bail!("could not determine kubo name from path");
        }
        kubo::validate_name(&name)?;

        let manifest_path = kubo_path.join("manifest.json");
        if !manifest_path.exists() {
            let now = chrono::Utc::now().to_rfc3339();
            let manifest = kubo::KuboManifest {
                version: 1,
                name: name.clone(),
                created_at: now.clone(),
                updated_at: Some(now),
                abots: vec![],
            };
            kubo::Kubo::write_manifest(&kubo_path, &manifest)?;
        }

        let mut kubos = self.kubos.lock().await;
        if kubos.contains_key(&name) {
            return Ok(name);
        }
        let k = kubo::new_kubo(name.clone(), kubo_path)?;
        kubos.insert(name.clone(), k);
        tracing::info!("opened kubo '{}' from {}", name, path);
        Ok(name)
    }

    pub async fn add_abot_to_kubo(
        self: &Arc<Self>,
        kubo_name: &str,
        abot_name: &str,
        create_session: bool,
        cols: u16,
        rows: u16,
        env: HashMap<String, String>,
    ) -> Result<Option<String>> {
        self.ensure_abot_in_kubo(abot_name, kubo_name).await?;
        bundle::add_known_abot(&self.data_dir, abot_name);

        if create_session {
            let session_name = self
                .create_session(
                    abot_name.to_string(),
                    cols,
                    rows,
                    env,
                    kubo_name.to_string(),
                )
                .await?;
            Ok(Some(session_name))
        } else {
            Ok(None)
        }
    }

    pub async fn remove_abot_from_kubo(&self, kubo_name: &str, abot_name: &str) -> Result<()> {
        kubo::validate_name(abot_name)?;
        kubo::validate_name(kubo_name)?;

        self.close_session_in_kubo(abot_name, kubo_name).await;

        let kubo_path = {
            let kubos = self.kubos.lock().await;
            match kubos.get(kubo_name) {
                Some(k) => k.path.clone(),
                None => anyhow::bail!("kubo '{}' not found", kubo_name),
            }
        };

        if let Ok(mut manifest) = kubo::Kubo::read_manifest(&kubo_path) {
            manifest.abots.retain(|a| a != abot_name);
            manifest.updated_at = Some(chrono::Utc::now().to_rfc3339());
            if let Err(e) = kubo::Kubo::write_manifest(&kubo_path, &manifest) {
                tracing::warn!("failed to write kubo manifest for '{}': {}", kubo_name, e);
            }
        }

        Ok(())
    }

    // ── Abot methods ────────────────────────────────────────────

    pub async fn list_abots(&self) -> Vec<serde_json::Value> {
        let session_keys = self.build_session_keys().await;
        let abots = bundle::read_known_abots(&self.data_dir);
        abots
            .iter()
            .map(|a| match bundle::get_abot_detail(&self.data_dir, &a.name) {
                Ok(detail) => {
                    let mut val = serde_json::to_value(&detail)
                        .unwrap_or_else(|_| serde_json::json!({ "name": a.name }));
                    if let Some(obj) = val.as_object_mut() {
                        obj.insert("added_at".to_string(), serde_json::json!(a.added_at));
                    }
                    inject_has_session(&mut val, &session_keys);
                    val
                }
                Err(_) => serde_json::json!({ "name": a.name, "added_at": a.added_at }),
            })
            .collect()
    }

    pub async fn get_abot_info(&self, abot_name: &str) -> Result<serde_json::Value> {
        kubo::validate_name(abot_name)?;
        let detail = bundle::get_abot_detail(&self.data_dir, abot_name)?;
        let session_keys = self.build_session_keys().await;
        let mut val = serde_json::to_value(&detail)?;
        inject_has_session(&mut val, &session_keys);
        Ok(val)
    }

    pub async fn remove_known_abot(&self, abot_name: &str) -> Result<()> {
        kubo::validate_name(abot_name)?;
        bundle::remove_known_abot(&self.data_dir, abot_name);
        Ok(())
    }

    pub async fn dismiss_variant(&self, abot_name: &str, kubo_name: &str) -> Result<()> {
        self.variant_op(abot_name, kubo_name, VariantOp::Dismiss)
            .await
    }

    pub async fn integrate_variant(&self, abot_name: &str, kubo_name: &str) -> Result<()> {
        self.variant_op(abot_name, kubo_name, VariantOp::Integrate)
            .await
    }

    pub async fn discard_variant(&self, abot_name: &str, kubo_name: &str) -> Result<()> {
        self.variant_op(abot_name, kubo_name, VariantOp::Discard)
            .await
    }

    // ── Scrollback ──────────────────────────────────────────────

    /// Save scrollback for all sessions that have a bundle path.
    pub async fn save_all_scrollback(&self) {
        let sessions = self.sessions.lock().await;
        for s in sessions.values() {
            if let Some(ref bp) = s.bundle_path {
                bundle::save_scrollback(bp, &s.get_buffer());
            }
        }
    }

    // ── Internal helpers ────────────────────────────────────────

    async fn create_kubo_backend(
        &self,
        kubo_name: &str,
        abot_name: &str,
        cols: u16,
        rows: u16,
        session_env: &HashMap<String, String>,
    ) -> Result<Box<dyn SessionBackend>> {
        let global_env = self.agent_env.lock().await;
        let mut merged = global_env.clone();
        drop(global_env);

        let mut kubos = self.kubos.lock().await;
        let kubo = kubos
            .get_mut(kubo_name)
            .ok_or_else(|| anyhow::anyhow!("kubo '{}' not found", kubo_name))?;

        let kubo_creds = bundle::read_credentials(&kubo.path.join("credentials.json"));
        merged.extend(kubo_creds);
        merged.extend(session_env.iter().map(|(k, v)| (k.clone(), v.clone())));
        let env: Vec<String> = merged.iter().map(|(k, v)| format!("{k}={v}")).collect();

        kubo.start().await?;
        let container_id = kubo
            .container_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("kubo '{}' failed to start", kubo_name))?
            .clone();

        kubo.ensure_abot_home(abot_name)?;
        drop(kubos);

        let backend =
            kubo_exec::KuboExecBackend::spawn(&container_id, abot_name, cols, rows, env).await?;

        let mut kubos = self.kubos.lock().await;
        if let Some(kubo) = kubos.get_mut(kubo_name) {
            kubo.session_opened();
        }

        Ok(Box::new(backend))
    }

    async fn ensure_abot_in_kubo(&self, name: &str, kubo: &str) -> Result<PathBuf> {
        kubo::validate_name(name)?;
        kubo::validate_name(kubo)?;

        let abots_dir = bundle::resolve_abots_dir(&self.data_dir);
        if let Err(e) = std::fs::create_dir_all(&abots_dir) {
            tracing::warn!("failed to create abots dir: {}", e);
        }
        let canonical_path = bundle::create_canonical_abot(&abots_dir, name)?;

        self.add_abot_to_kubo_worktree(&canonical_path, name, kubo)
            .await
    }

    async fn add_abot_to_kubo_worktree(
        &self,
        canonical_path: &Path,
        name: &str,
        kubo: &str,
    ) -> Result<PathBuf> {
        let kubo_path = {
            let kubos = self.kubos.lock().await;
            match kubos.get(kubo) {
                Some(k) => k.path.clone(),
                None => anyhow::bail!("kubo '{}' not found", kubo),
            }
        };

        bundle::worktree_add_abot(canonical_path, &kubo_path, name, kubo)?;

        if let Ok(mut manifest) = kubo::Kubo::read_manifest(&kubo_path) {
            if !manifest.abots.contains(&name.to_string()) {
                manifest.abots.push(name.to_string());
                manifest.updated_at = Some(chrono::Utc::now().to_rfc3339());
                if let Err(e) = kubo::Kubo::write_manifest(&kubo_path, &manifest) {
                    tracing::warn!("failed to write kubo manifest for '{}': {}", kubo, e);
                }
            }
        }

        Ok(kubo_path.join(name))
    }

    async fn close_session_in_kubo(&self, abot: &str, kubo: &str) {
        let qualified = format!("{}@{}", abot, kubo);
        let mut sessions = self.sessions.lock().await;
        if let Some(mut session) = sessions.remove(&qualified) {
            let kubo_name = if session.is_alive() {
                session.kubo.clone()
            } else {
                None
            };
            session.backend.kill();
            let _ = self
                .output_tx
                .send(OutputEvent::SessionRemoved { session: qualified });
            drop(sessions);
            if let Some(kn) = kubo_name {
                let mut kubos = self.kubos.lock().await;
                if let Some(k) = kubos.get_mut(&kn) {
                    k.session_closed();
                }
            }
        }
    }

    async fn build_session_keys(&self) -> HashSet<String> {
        let sessions = self.sessions.lock().await;
        sessions.keys().cloned().collect()
    }

    async fn variant_op(&self, abot: &str, kubo: &str, op: VariantOp) -> Result<()> {
        kubo::validate_name(abot)?;
        kubo::validate_name(kubo)?;
        let abots_dir = bundle::resolve_abots_dir(&self.data_dir);
        let canonical_path = abots_dir.join(format!("{abot}.abot"));
        let kubo_branch = format!("kubo/{kubo}");

        self.close_session_in_kubo(abot, kubo).await;

        match op {
            VariantOp::Dismiss => bundle::dismiss_variant(&canonical_path, &kubo_branch)?,
            VariantOp::Integrate => bundle::integrate_variant(&canonical_path, &kubo_branch)?,
            VariantOp::Discard => bundle::discard_variant(&canonical_path, &kubo_branch)?,
        }

        self.remove_abot_from_kubo_manifest(kubo, abot).await;
        Ok(())
    }

    async fn remove_abot_from_kubo_manifest(&self, kubo: &str, abot: &str) {
        let kubo_path = {
            let kubos = self.kubos.lock().await;
            match kubos.get(kubo) {
                Some(k) => k.path.clone(),
                None => bundle::resolve_kubos_dir(&self.data_dir).join(format!("{kubo}.kubo")),
            }
        };
        if let Ok(mut manifest) = kubo::Kubo::read_manifest(&kubo_path) {
            manifest.abots.retain(|a| a != abot);
            manifest.updated_at = Some(chrono::Utc::now().to_rfc3339());
            if let Err(e) = kubo::Kubo::write_manifest(&kubo_path, &manifest) {
                tracing::warn!("failed to update kubo manifest for '{}': {}", kubo, e);
            }
        }
    }

    async fn autosave(&self) {
        let to_save: Vec<(String, PathBuf, HashMap<String, String>, String)> = {
            let sessions = self.sessions.lock().await;
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
                    if bundle_path.join(".git").exists() {
                        match bundle::auto_commit_abot(&bundle_path) {
                            Ok(true) => {
                                tracing::debug!("autosave: git commit for '{}'", name);
                            }
                            Ok(false) => {}
                            Err(e) => {
                                tracing::warn!("autosave: git commit failed for '{}': {}", name, e);
                            }
                        }
                    }
                    let mut sessions = self.sessions.lock().await;
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

    async fn idle_check_kubos(&self) {
        let mut kubos = self.kubos.lock().await;
        let names: Vec<String> = kubos
            .values()
            .filter(|k| k.should_idle_stop())
            .map(|k| k.name.clone())
            .collect();
        for name in names {
            if let Some(kubo) = kubos.get_mut(&name) {
                if let Err(e) = kubo.stop().await {
                    tracing::error!("failed to idle-stop kubo '{}': {}", name, e);
                }
            }
        }
    }
}

/// Capture tmux scrollback for an abot in a kubo container.
pub async fn capture_tmux_scrollback(kubo: &kubo::Kubo, abot_name: &str) -> Option<String> {
    let container_id = kubo.container_id.as_ref()?;
    let docker = bollard::Docker::connect_with_socket_defaults().ok()?;
    let tmux_name = abot_name.replace(['.', ':'], "_");
    kubo_exec::capture_scrollback(&docker, container_id, &tmux_name).await
}

#[derive(Clone, Copy)]
enum VariantOp {
    Dismiss,
    Integrate,
    Discard,
}

/// Inject `has_session` into each kubo_branch entry.
fn inject_has_session(val: &mut serde_json::Value, session_keys: &HashSet<String>) {
    let abot_name = val
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    if let Some(branches) = val.get_mut("kubo_branches").and_then(|v| v.as_array_mut()) {
        for branch in branches {
            if let Some(obj) = branch.as_object_mut() {
                let kubo_name = obj
                    .get("kubo_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let qualified = format!("{}@{}", abot_name, kubo_name);
                let active = session_keys.contains(&qualified);
                obj.insert("has_session".to_string(), serde_json::json!(active));
            }
        }
    }
}

/// Spawn a task that relays output from a session's reader channel to the broadcast.
fn spawn_output_relay(
    output_tx: broadcast::Sender<OutputEvent>,
    engine: Arc<Engine>,
    shared_name: Option<std::sync::Arc<std::sync::Mutex<String>>>,
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
            let current_name = shared_name
                .as_ref()
                .map(|sn| sn.lock().unwrap().clone())
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

        let current_name = shared_name
            .as_ref()
            .map(|sn| sn.lock().unwrap().clone())
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
        if let Some(kn) = kubo_name {
            let mut kubos = engine.kubos.lock().await;
            if let Some(kubo) = kubos.get_mut(&kn) {
                kubo.session_closed();
            }
        }
        if let Some(code) = code {
            let _ = output_tx.send(OutputEvent::Exit {
                session: current_name,
                code,
            });
        }
    });
}
