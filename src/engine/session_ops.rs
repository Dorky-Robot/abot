use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::session::Session;
use super::{bundle, kubo, Engine, EngineError, EngineResult};

impl Engine {
    // ── Session CRUD ─────────────────────────────────────────

    pub async fn list_sessions(&self) -> Vec<super::session::SessionSummary> {
        let sessions = self.sessions.lock().await;
        sessions.values().map(|s| s.summary()).collect()
    }

    pub async fn get_session(&self, name: &str) -> EngineResult<super::session::SessionSummary> {
        let sessions = self.sessions.lock().await;
        match sessions.get(name) {
            Some(s) => Ok(s.summary()),
            None => Err(EngineError::NotFound(format!("session '{name}' not found"))),
        }
    }

    pub async fn create_session(
        self: &Arc<Self>,
        name: String,
        cols: u16,
        rows: u16,
        env: HashMap<String, String>,
        kubo: String,
    ) -> EngineResult<String> {
        let abot_dir = self.ensure_abot_in_kubo(&name, &kubo).await?;
        let qualified = format!("{}@{}", name, kubo);

        let backend = self
            .create_kubo_backend(&kubo, &name, &qualified, cols, rows, &env)
            .await
            .map_err(|e| EngineError::Internal(format!("failed to create session: {e}")))?;

        let bundle_path = Some(abot_dir);

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

    pub async fn get_session_buffer(&self, session: &str) -> EngineResult<String> {
        let sessions = self.sessions.lock().await;
        match sessions.get(session) {
            Some(s) => Ok(s.get_buffer()),
            None => Err(EngineError::NotFound(format!(
                "session '{session}' not found"
            ))),
        }
    }

    pub async fn delete_session(&self, name: &str) -> EngineResult<()> {
        let (session, kubo_name) = {
            let mut sessions = self.sessions.lock().await;
            Self::teardown_session(&mut sessions, &self.output_tx, name)
                .ok_or_else(|| EngineError::NotFound(format!("session '{name}' not found")))?
        };
        self.decrement_kubo(kubo_name, name).await;
        if let Some(bp) = session.bundle_path {
            let _ = std::fs::remove_dir_all(&bp);
        }
        Ok(())
    }

    pub async fn rename_session(&self, old_name: &str, new_name: &str) -> EngineResult<()> {
        kubo::validate_name(new_name).map_err(|e| EngineError::InvalidInput(e.to_string()))?;

        // Rename in the sessions map (fast, no I/O).
        let bundle_path = {
            let mut sessions = self.sessions.lock().await;
            if sessions.contains_key(new_name) {
                return Err(EngineError::AlreadyExists(format!(
                    "session '{new_name}' already exists"
                )));
            }
            let mut session = sessions
                .remove(old_name)
                .ok_or_else(|| EngineError::NotFound(format!("session '{old_name}' not found")))?;
            session.name = new_name.to_string();
            session.name_tx.send_replace(new_name.to_string());
            let bp = session.bundle_path.clone();
            sessions.insert(new_name.to_string(), session);
            bp
        };

        // Update the on-disk manifest outside the lock (blocking I/O).
        if let Some(bp) = bundle_path {
            let manifest_path = bp.join("manifest.json");
            if manifest_path.exists() {
                if let Ok(mut manifest) = bundle::read_json(&manifest_path) {
                    manifest["name"] = serde_json::Value::String(new_name.to_string());
                    let _ = bundle::write_json(&manifest_path, &manifest);
                }
            }
        }

        Ok(())
    }

    pub async fn write_input(&self, name: &str, data: &str) -> EngineResult<()> {
        let mut sessions = self.sessions.lock().await;
        if let Some(s) = sessions.get_mut(name) {
            if s.is_alive() {
                s.write(data.as_bytes())
                    .map_err(|e| EngineError::Internal(e.to_string()))?;
            } else {
                return Err(EngineError::InvalidInput(format!(
                    "session '{name}' is not alive"
                )));
            }
        } else {
            return Err(EngineError::NotFound(format!("session '{name}' not found")));
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
            apply_env_update(&mut agent_env, &env);
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
    ) -> EngineResult<()> {
        let global_env = self.agent_env.lock().await;
        let global_snapshot = global_env.clone();
        drop(global_env);

        let mut sessions = self.sessions.lock().await;
        if let Some(s) = sessions.get_mut(session_name) {
            apply_env_update(&mut s.env, &env);
            s.mark_dirty();
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
            Err(EngineError::NotFound(format!(
                "session '{session_name}' not found"
            )))
        }
    }

    // ── Bundle methods ──────────────────────────────────────

    pub async fn open_bundle(
        self: &Arc<Self>,
        path: &str,
        cols: u16,
        rows: u16,
        kubo_name: &str,
    ) -> EngineResult<(String, String)> {
        let opened = bundle::open_bundle(path).await?;
        let name = opened.name.clone();
        kubo::validate_name(&name).map_err(|e| EngineError::InvalidInput(e.to_string()))?;
        let canonical_path = opened.path.clone();

        let worktree_path = self
            .add_abot_to_kubo_worktree(&canonical_path, &name, kubo_name)
            .await?;

        let qualified = format!("{}@{}", name, kubo_name);

        let backend = self
            .create_kubo_backend(kubo_name, &name, &qualified, cols, rows, &opened.env)
            .await
            .map_err(|e| {
                EngineError::Internal(format!("failed to create session from bundle: {e}"))
            })?;

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

    pub async fn save_session(&self, session_name: &str) -> EngineResult<String> {
        let sessions = self.sessions.lock().await;
        if let Some(s) = sessions.get(session_name) {
            let bundle_path = match &s.bundle_path {
                Some(p) => p.clone(),
                None => {
                    return Err(EngineError::InvalidInput(format!(
                        "session '{session_name}' has no bundle path (use save-as)"
                    )));
                }
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
                s.mark_saved();
            }
            Ok(bundle_path.to_string_lossy().to_string())
        } else {
            Err(EngineError::NotFound(format!(
                "session '{session_name}' not found"
            )))
        }
    }

    pub async fn save_session_as(&self, session_name: &str, path: &str) -> EngineResult<String> {
        {
            let mut check = Path::new(path);
            while let Some(parent) = check.parent() {
                if parent
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("abot"))
                {
                    return Err(EngineError::InvalidInput(format!(
                        "cannot save inside another .abot bundle: {}",
                        parent.display()
                    )));
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
                s.mark_saved();
            }
            Ok(new_bundle_path.to_string_lossy().to_string())
        } else {
            Err(EngineError::NotFound(format!(
                "session '{session_name}' not found"
            )))
        }
    }

    pub async fn close_session(&self, session_name: &str, save: bool) -> EngineResult<()> {
        if save {
            let (bundle_path, env, name) = {
                let sessions = self.sessions.lock().await;
                match sessions.get(session_name) {
                    Some(s) => match &s.bundle_path {
                        Some(bp) => (bp.clone(), s.env.clone(), s.name.clone()),
                        None => {
                            return Err(EngineError::InvalidInput(format!(
                                "session '{session_name}' has no bundle path (use save-as before close with save)"
                            )));
                        }
                    },
                    None => {
                        return Err(EngineError::NotFound(format!(
                            "session '{session_name}' not found"
                        )));
                    }
                }
            };
            let bundle_name = name.split('@').next().unwrap_or(&name);
            bundle::save_bundle(&bundle_path, bundle_name, &env).await?;
        }

        let (session, kubo_name) = {
            let mut sessions = self.sessions.lock().await;
            let (session, kubo_name) =
                Self::teardown_session(&mut sessions, &self.output_tx, session_name).ok_or_else(
                    || EngineError::NotFound(format!("session '{session_name}' not found")),
                )?;
            if let Some(ref bp) = session.bundle_path {
                bundle::save_scrollback(bp, &session.get_buffer());
            }
            (session, kubo_name)
        };
        drop(session);
        self.decrement_kubo(kubo_name, session_name).await;
        Ok(())
    }

    // ── Autosave ────────────────────────────────────────────

    #[allow(clippy::type_complexity)]
    pub(super) async fn autosave(&self) {
        // Snapshot dirty sessions with their current dirty_gen
        let to_save: Vec<(String, PathBuf, HashMap<String, String>, String, u64)> = {
            let sessions = self.sessions.lock().await;
            sessions
                .values()
                .filter(|s| s.is_dirty() && s.bundle_path.is_some() && s.is_alive())
                .map(|s| {
                    (
                        s.name.clone(),
                        s.bundle_path.clone().unwrap(),
                        s.env.clone(),
                        s.get_buffer(),
                        s.dirty_gen,
                    )
                })
                .collect()
        };

        for (name, bundle_path, env, scrollback, snapshot_gen) in to_save {
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
                    // Only mark saved if no mutations occurred since snapshot
                    let mut sessions = self.sessions.lock().await;
                    if let Some(s) = sessions.get_mut(&name) {
                        s.mark_saved_if_unchanged(snapshot_gen);
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

/// Apply an env update map: `Some(val)` inserts, `None` removes.
fn apply_env_update(
    target: &mut HashMap<String, String>,
    updates: &HashMap<String, Option<String>>,
) {
    for (key, value) in updates {
        match value {
            Some(val) => {
                target.insert(key.clone(), val.clone());
            }
            None => {
                target.remove(key);
            }
        }
    }
}
