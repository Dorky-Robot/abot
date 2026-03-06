use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::session::Session;
use super::{bundle, kubo, Engine};

impl Engine {
    // ── Session CRUD ─────────────────────────────────────────

    pub async fn list_sessions(&self) -> Vec<super::session::SessionSummary> {
        let sessions = self.sessions.lock().await;
        sessions.values().map(|s| s.summary()).collect()
    }

    pub async fn get_session(&self, name: &str) -> Result<super::session::SessionSummary> {
        let sessions = self.sessions.lock().await;
        match sessions.get(name) {
            Some(s) => Ok(s.summary()),
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
        let abot_dir = self.ensure_abot_in_kubo(&name, &kubo).await?;
        let qualified = format!("{}@{}", name, kubo);

        let backend_result = self
            .create_kubo_backend(&kubo, &name, cols, rows, &env)
            .await;
        let bundle_path = Some(abot_dir);

        match backend_result {
            Ok(backend) => {
                if let Some(ref bp) = bundle_path {
                    if let Err(e) = bundle::save_bundle(bp, &name, &env).await {
                        tracing::warn!("failed to write initial manifest for '{}': {}", name, e);
                    }
                }

                let kubo_for_scrollback = kubo.clone();
                let session = Session::new(
                    qualified.clone(),
                    backend,
                    env,
                    bundle_path.clone(),
                    Some(kubo),
                );

                let session_name = self
                    .register_session(
                        &qualified,
                        session,
                        &kubo_for_scrollback,
                        &name,
                        bundle_path.as_deref(),
                    )
                    .await;

                Ok(session_name)
            }
            Err(e) => anyhow::bail!("failed to create session: {}", e),
        }
    }

    pub async fn get_session_buffer(&self, session: &str) -> Result<String> {
        let sessions = self.sessions.lock().await;
        match sessions.get(session) {
            Some(s) => Ok(s.get_buffer()),
            None => anyhow::bail!("session '{}' not found", session),
        }
    }

    pub async fn delete_session(&self, name: &str) -> Result<()> {
        let (session, kubo_name) = {
            let mut sessions = self.sessions.lock().await;
            Self::teardown_session(&mut sessions, &self.output_tx, name)
                .ok_or_else(|| anyhow::anyhow!("session '{}' not found", name))?
        };
        self.decrement_kubo(kubo_name).await;
        if let Some(bp) = session.bundle_path {
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
            Ok(())
        } else {
            anyhow::bail!("session '{}' not found", old_name);
        }
    }

    pub async fn write_input(&self, name: &str, data: &str) -> Result<()> {
        let mut sessions = self.sessions.lock().await;
        if let Some(s) = sessions.get_mut(name) {
            if s.is_alive() {
                s.write(data.as_bytes())?;
            } else {
                anyhow::bail!("session '{}' is not alive", name);
            }
        } else {
            anyhow::bail!("session '{}' not found", name);
        }
        Ok(())
    }

    pub async fn resize(&self, name: &str, cols: u16, rows: u16) {
        let mut sessions = self.sessions.lock().await;
        if let Some(s) = sessions.get_mut(name) {
            let _ = s.resize(cols, rows);
        }
    }

    // ── Env methods ─────────────────────────────────────────

    pub async fn update_agent_env(&self, env: HashMap<String, Option<String>>) {
        let snapshot = {
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
            agent_env.clone()
        };

        let sessions = self.sessions.lock().await;
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

    // ── Bundle methods ──────────────────────────────────────

    pub async fn open_bundle(
        self: &Arc<Self>,
        path: &str,
        cols: u16,
        rows: u16,
        kubo_name: &str,
    ) -> Result<(String, String)> {
        let opened = bundle::open_bundle(path).await?;
        let name = opened.name.clone();
        kubo::validate_name(&name)?;
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

                let session_name = self
                    .register_session(&qualified, session, kubo_name, &name, Some(&worktree_path))
                    .await;

                if !opened.env.is_empty() {
                    let global_env = self.agent_env.lock().await;
                    let mut merged = global_env.clone();
                    merged.extend(opened.env);
                    let sessions = self.sessions.lock().await;
                    if let Some(s) = sessions.get(&qualified) {
                        s.backend.inject_env(&merged);
                    }
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
            let (bundle_path, env, name) = {
                let sessions = self.sessions.lock().await;
                match sessions.get(session_name) {
                    Some(s) => match &s.bundle_path {
                        Some(bp) => (bp.clone(), s.env.clone(), s.name.clone()),
                        None => anyhow::bail!(
                            "session '{}' has no bundle path (use save-as before close with save)",
                            session_name
                        ),
                    },
                    None => anyhow::bail!("session '{}' not found", session_name),
                }
            };
            let bundle_name = name.split('@').next().unwrap_or(&name);
            bundle::save_bundle(&bundle_path, bundle_name, &env).await?;
        }

        let (session, kubo_name) = {
            let mut sessions = self.sessions.lock().await;
            let (session, kubo_name) =
                Self::teardown_session(&mut sessions, &self.output_tx, session_name)
                    .ok_or_else(|| anyhow::anyhow!("session '{}' not found", session_name))?;
            if let Some(ref bp) = session.bundle_path {
                bundle::save_scrollback(bp, &session.get_buffer());
            }
            (session, kubo_name)
        };
        drop(session);
        self.decrement_kubo(kubo_name).await;
        Ok(())
    }

    // ── Autosave ────────────────────────────────────────────

    pub(super) async fn autosave(&self) {
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
            let bundle_name = name.split('@').next().unwrap_or(&name);
            match bundle::save_bundle(&bundle_path, bundle_name, &env).await {
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
}
