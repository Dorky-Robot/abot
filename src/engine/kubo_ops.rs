use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::backend::SessionBackend;
use super::{bundle, kubo, kubo_exec, Engine};

impl Engine {
    // ── Kubo CRUD ────────────────────────────────────────────

    pub async fn list_kubos(&self) -> Vec<kubo::KuboSummary> {
        let kubos = self.kubos.lock().await;
        let mut list = Vec::with_capacity(kubos.len());
        for k in kubos.values() {
            list.push(k.summary().await);
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
            let qualified = format!("{}@{}", abot_name, kubo_name);
            {
                let sessions = self.sessions.lock().await;
                if sessions.contains_key(&qualified) {
                    return Ok(Some(qualified));
                }
            }

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

        {
            let kubos = self.kubos.lock().await;
            if let Some(k) = kubos.get(kubo_name) {
                if let Some(ref cid) = k.container_id {
                    if let Ok(docker) = bollard::Docker::connect_with_socket_defaults() {
                        let tmux_name = kubo_exec::tmux_session_name(abot_name);
                        kubo_exec::tmux_kill_session(&docker, cid, &tmux_name).await;
                    }
                }
            }
        }

        self.remove_abot_from_kubo_manifest(kubo_name, abot_name)
            .await;

        Ok(())
    }

    // ── Internal: kubo backend ──────────────────────────────

    pub(super) async fn create_kubo_backend(
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

    pub(super) async fn ensure_abot_in_kubo(&self, name: &str, kubo: &str) -> Result<PathBuf> {
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

    pub(super) async fn add_abot_to_kubo_worktree(
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

    pub(super) async fn close_session_in_kubo(&self, abot: &str, kubo: &str) {
        let qualified = format!("{}@{}", abot, kubo);
        let kubo_name = {
            let mut sessions = self.sessions.lock().await;
            Self::teardown_session(&mut sessions, &self.output_tx, &qualified)
                .and_then(|(_, kn)| kn)
        };
        self.decrement_kubo(kubo_name).await;
    }

    pub(super) async fn remove_abot_from_kubo_manifest(&self, kubo: &str, abot: &str) {
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

    // ── Health & idle checks ────────────────────────────────

    pub(super) async fn health_check_kubos(&self) {
        let dead_kubos: Vec<String> = {
            let kubos = self.kubos.lock().await;
            let mut dead = Vec::new();
            for (name, kubo) in kubos.iter() {
                if kubo.container_id.is_some()
                    && kubo.active_sessions > 0
                    && !kubo.is_running().await
                {
                    dead.push(name.clone());
                }
            }
            dead
        };

        for kubo_name in dead_kubos {
            tracing::warn!(
                "kubo '{}' container is dead, cleaning up sessions",
                kubo_name
            );

            let to_remove: Vec<String> = {
                let sessions = self.sessions.lock().await;
                sessions
                    .iter()
                    .filter(|(_, s)| s.kubo.as_deref() == Some(&kubo_name))
                    .map(|(name, _)| name.clone())
                    .collect()
            };

            for session_name in &to_remove {
                let mut sessions = self.sessions.lock().await;
                Self::teardown_session(&mut sessions, &self.output_tx, session_name);
            }

            {
                let mut kubos = self.kubos.lock().await;
                if let Some(kubo) = kubos.get_mut(&kubo_name) {
                    kubo.container_id = None;
                    kubo.active_sessions = 0;
                    kubo.last_session_close = Some(std::time::Instant::now());
                }
            }
        }
    }

    pub(super) async fn idle_check_kubos(&self) {
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
