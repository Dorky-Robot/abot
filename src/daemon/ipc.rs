use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use super::session::Session;
use super::DaemonState;

/// Messages from server to daemon
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
#[allow(dead_code)]
pub enum DaemonRequest {
    /// RPC: list all sessions
    #[serde(rename = "list-sessions")]
    ListSessions { id: String },

    /// RPC: create a new session
    #[serde(rename = "create-session")]
    CreateSession {
        id: String,
        name: String,
        #[serde(default = "default_cols")]
        cols: u16,
        #[serde(default = "default_rows")]
        rows: u16,
        /// Optional per-session environment variables
        #[serde(default)]
        env: HashMap<String, String>,
        /// Kubo name. Session runs inside the named kubo container.
        kubo: String,
    },

    /// RPC: attach a client to a session
    #[serde(rename = "attach")]
    Attach {
        id: String,
        #[serde(rename = "clientId")]
        client_id: String,
        session: String,
        #[serde(default = "default_cols")]
        cols: u16,
        #[serde(default = "default_rows")]
        rows: u16,
    },

    /// RPC: get a single session by name
    #[serde(rename = "get-session")]
    GetSession { id: String, name: String },

    /// RPC: delete a session
    #[serde(rename = "delete-session")]
    DeleteSession { id: String, name: String },

    /// RPC: rename a session
    #[serde(rename = "rename-session")]
    RenameSession {
        id: String,
        #[serde(rename = "oldName")]
        old_name: String,
        #[serde(rename = "newName")]
        new_name: String,
    },

    /// Fire-and-forget: send input to PTY
    #[serde(rename = "input")]
    Input {
        #[serde(rename = "clientId")]
        client_id: String,
        /// Explicit session name (preferred for multi-session routing)
        #[serde(default)]
        session: Option<String>,
        data: String,
    },

    /// Fire-and-forget: resize PTY
    #[serde(rename = "resize")]
    Resize {
        #[serde(rename = "clientId")]
        client_id: String,
        /// Explicit session name (preferred for multi-session routing)
        #[serde(default)]
        session: Option<String>,
        cols: u16,
        rows: u16,
    },

    /// Fire-and-forget: detach a client from one or all sessions
    #[serde(rename = "detach")]
    Detach {
        #[serde(rename = "clientId")]
        client_id: String,
        /// If set, detach from this specific session; otherwise detach from all
        #[serde(default)]
        session: Option<String>,
    },

    /// RPC: update environment variables injected into agent containers
    #[serde(rename = "update-agent-env")]
    UpdateAgentEnv {
        id: String,
        /// key→Some(val) to set, key→None to remove
        env: HashMap<String, Option<String>>,
    },

    /// RPC: update environment variables for a single session
    #[serde(rename = "update-session-env")]
    UpdateSessionEnv {
        id: String,
        session: String,
        /// key→Some(val) to set, key→None to remove
        env: HashMap<String, Option<String>>,
    },

    /// RPC: open a .abot bundle as a new session
    #[serde(rename = "open-bundle")]
    OpenBundle {
        id: String,
        path: String,
        #[serde(default = "default_cols")]
        cols: u16,
        #[serde(default = "default_rows")]
        rows: u16,
    },

    /// RPC: save session to its tracked bundle path
    #[serde(rename = "save-session")]
    SaveSession { id: String, session: String },

    /// RPC: save session to a new bundle path
    #[serde(rename = "save-session-as")]
    SaveSessionAs {
        id: String,
        session: String,
        path: String,
    },

    /// RPC: close session (optionally save first)
    #[serde(rename = "close-session")]
    CloseSession {
        id: String,
        session: String,
        #[serde(default)]
        save: bool,
    },

    /// RPC: health check
    #[serde(rename = "ping")]
    Ping { id: String },

    // ── Kubo management ───────────────────────────────────────────
    /// RPC: create a new kubo (shared runtime room)
    #[serde(rename = "create-kubo")]
    CreateKubo { id: String, name: String },

    /// RPC: list all kubos
    #[serde(rename = "list-kubos")]
    ListKubos { id: String },

    /// RPC: start a kubo container
    #[serde(rename = "start-kubo")]
    StartKubo { id: String, name: String },

    /// RPC: stop a kubo container
    #[serde(rename = "stop-kubo")]
    StopKubo { id: String, name: String },

    /// RPC: open a kubo from a path on disk (register it in the daemon)
    #[serde(rename = "open-kubo")]
    OpenKubo { id: String, path: String },

    /// RPC: add an abot to a kubo (create canonical abot + worktree + optionally a session)
    #[serde(rename = "add-abot-to-kubo")]
    AddAbotToKubo {
        id: String,
        kubo: String,
        abot: String,
        #[serde(default, rename = "createSession")]
        create_session: bool,
        #[serde(default = "default_cols")]
        cols: u16,
        #[serde(default = "default_rows")]
        rows: u16,
        #[serde(default)]
        env: HashMap<String, String>,
    },

    /// RPC: remove an abot from a kubo (close session, remove worktree, update manifest)
    #[serde(rename = "remove-abot-from-kubo")]
    RemoveAbotFromKubo {
        id: String,
        kubo: String,
        abot: String,
    },

    // ── Abot git operations ───────────────────────────────────────
    /// RPC: clone an abot (create a variant)
    #[serde(rename = "clone-abot")]
    CloneAbot {
        id: String,
        /// Source abot name
        source: String,
        /// Target abot name
        target: String,
    },

    /// RPC: run a git operation on an abot
    #[serde(rename = "abot-git")]
    AbotGit {
        id: String,
        abot: String,
        /// Git subcommand: "status", "log", "diff"
        op: String,
    },

    // ── Known abots registry ─────────────────────────────────────
    /// RPC: list all known abots
    #[serde(rename = "list-abots")]
    ListAbots { id: String },

    /// RPC: get detailed info for a single abot
    #[serde(rename = "get-abot-info")]
    GetAbotInfo { id: String, abot: String },

    /// RPC: remove an abot from the known list
    #[serde(rename = "remove-known-abot")]
    RemoveKnownAbot { id: String, abot: String },
}

fn default_cols() -> u16 {
    120
}
fn default_rows() -> u16 {
    40
}

/// RPC responses from daemon to server
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum DaemonResponse {
    SessionList {
        id: String,
        sessions: Vec<serde_json::Value>,
    },
    SessionCreated {
        id: String,
        name: String,
    },
    Attached {
        id: String,
        session: String,
        buffer: String,
    },
    Deleted {
        id: String,
        name: String,
    },
    Renamed {
        id: String,
        #[serde(rename = "oldName")]
        old_name: String,
        #[serde(rename = "newName")]
        new_name: String,
    },
    SessionDetail {
        id: String,
        session: serde_json::Value,
    },
    EnvUpdated {
        id: String,
    },
    SessionEnvUpdated {
        id: String,
        session: String,
    },
    Opened {
        id: String,
        name: String,
        path: String,
    },
    Saved {
        id: String,
        session: String,
        path: String,
    },
    Closed {
        id: String,
        session: String,
    },
    Pong {
        id: String,
    },
    Error {
        id: String,
        error: String,
    },
    // ── Kubo responses ────────────────────────────────────────────
    KuboCreated {
        id: String,
        name: String,
        path: String,
    },
    KuboList {
        id: String,
        kubos: Vec<serde_json::Value>,
    },
    KuboStarted {
        id: String,
        name: String,
    },
    KuboStopped {
        id: String,
        name: String,
    },
    KuboOpened {
        id: String,
        name: String,
        path: String,
    },
    AbotAddedToKubo {
        id: String,
        kubo: String,
        abot: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        session: Option<String>,
    },
    AbotRemovedFromKubo {
        id: String,
        kubo: String,
        abot: String,
    },
    AbotCloned {
        id: String,
        source: String,
        target: String,
        path: String,
    },
    AbotGitResult {
        id: String,
        abot: String,
        op: String,
        output: String,
    },
    // ── Known abots responses ────────────────────────────────────
    AbotList {
        id: String,
        abots: Vec<serde_json::Value>,
    },
    AbotInfo {
        id: String,
        abot: serde_json::Value,
    },
    KnownAbotRemoved {
        id: String,
        abot: String,
    },
}

/// Broadcast events from daemon (no id, sent to all connected servers)
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
#[allow(dead_code)]
pub enum OutputEvent {
    #[serde(rename = "output")]
    Output { session: String, data: String },

    #[serde(rename = "exit")]
    Exit { session: String, code: u32 },

    #[serde(rename = "session-removed")]
    SessionRemoved { session: String },
}

pub async fn handle_request(
    state: &Arc<DaemonState>,
    req: DaemonRequest,
) -> Option<DaemonResponse> {
    match req {
        DaemonRequest::ListSessions { id } => {
            let sessions = state.sessions.lock().await;
            let list: Vec<serde_json::Value> = sessions.values().map(|s| s.to_json()).collect();
            Some(DaemonResponse::SessionList { id, sessions: list })
        }

        DaemonRequest::GetSession { id, name } => {
            let sessions = state.sessions.lock().await;
            match sessions.get(&name) {
                Some(s) => Some(DaemonResponse::SessionDetail {
                    id,
                    session: s.to_json(),
                }),
                None => Some(DaemonResponse::Error {
                    id,
                    error: format!("session '{}' not found", name),
                }),
            }
        }

        DaemonRequest::CreateSession {
            id,
            name,
            cols,
            rows,
            env,
            kubo,
        } => handle_create_session(state, id, name, cols, rows, env, kubo).await,

        DaemonRequest::Attach {
            id,
            client_id,
            session,
            cols: _,
            rows: _,
        } => {
            // Record the client→session mapping (additive — supports multi-session)
            {
                let mut attachments = state.client_attachments.lock().await;
                attachments
                    .entry(client_id)
                    .or_default()
                    .insert(session.clone());
            }

            let sessions = state.sessions.lock().await;
            match sessions.get(&session) {
                Some(s) => {
                    let buffer = s.get_buffer();
                    Some(DaemonResponse::Attached {
                        id,
                        session,
                        buffer,
                    })
                }
                None => Some(DaemonResponse::Error {
                    id,
                    error: format!("session '{}' not found", session),
                }),
            }
        }

        DaemonRequest::DeleteSession { id, name } => {
            let (bundle_path, kubo_name) = {
                let mut sessions = state.sessions.lock().await;
                if let Some(mut session) = sessions.remove(&name) {
                    let bp = session.bundle_path.clone();
                    // Only decrement kubo count if the session was still running
                    // (if already exited, the output relay already called session_closed)
                    let kn = if session.is_alive() {
                        session.kubo.clone()
                    } else {
                        None
                    };
                    session.backend.kill();
                    let _ = state.output_tx.send(OutputEvent::SessionRemoved {
                        session: name.clone(),
                    });
                    (bp, kn)
                } else {
                    return Some(DaemonResponse::Error {
                        id,
                        error: format!("session '{}' not found", name),
                    });
                }
            };
            if let Some(kn) = kubo_name {
                let mut kubos = state.kubos.lock().await;
                if let Some(kubo) = kubos.get_mut(&kn) {
                    kubo.session_closed();
                }
            }
            // Remove bundle directory outside the lock to avoid blocking
            if let Some(bp) = bundle_path {
                let _ = std::fs::remove_dir_all(&bp);
            }
            Some(DaemonResponse::Deleted { id, name })
        }

        DaemonRequest::RenameSession {
            id,
            old_name,
            new_name,
        } => {
            if let Err(e) = super::kubo::validate_name(&new_name) {
                return Some(DaemonResponse::Error {
                    id,
                    error: format!("invalid session name: {}", e),
                });
            }
            let mut sessions = state.sessions.lock().await;
            if sessions.contains_key(&new_name) {
                return Some(DaemonResponse::Error {
                    id,
                    error: format!("session '{}' already exists", new_name),
                });
            }
            if let Some(mut session) = sessions.remove(&old_name) {
                session.name = new_name.clone();
                // Update the shared name so background relay tasks use the new name
                *session.shared_name.lock().unwrap() = new_name.clone();

                // Update manifest.json name field if bundle exists
                if let Some(ref bp) = session.bundle_path {
                    let manifest_path = bp.join("manifest.json");
                    if manifest_path.exists() {
                        if let Ok(mut manifest) = super::bundle::read_json(&manifest_path) {
                            manifest["name"] = serde_json::Value::String(new_name.clone());
                            let _ = super::bundle::write_json(&manifest_path, &manifest);
                        }
                    }
                }

                sessions.insert(new_name.clone(), session);

                // Update client attachments that point to old name
                drop(sessions);
                let mut attachments = state.client_attachments.lock().await;
                for (_client_id, attached_sessions) in attachments.iter_mut() {
                    if attached_sessions.remove(&old_name) {
                        attached_sessions.insert(new_name.clone());
                    }
                }

                Some(DaemonResponse::Renamed {
                    id,
                    old_name,
                    new_name,
                })
            } else {
                Some(DaemonResponse::Error {
                    id,
                    error: format!("session '{}' not found", old_name),
                })
            }
        }

        // Fire-and-forget messages — no response
        DaemonRequest::Input {
            client_id,
            session,
            data,
        } => {
            if let Some(session_name) = session {
                let mut sessions = state.sessions.lock().await;
                if let Some(session) = sessions.get_mut(&session_name) {
                    if session.is_alive() {
                        match session.write(data.as_bytes()) {
                            Ok(_) => {
                                tracing::debug!(
                                    "daemon: wrote {} bytes to session '{}'",
                                    data.len(),
                                    session.name
                                );
                            }
                            Err(e) => {
                                tracing::error!(
                                    "daemon: write to session '{}' failed: {}",
                                    session.name,
                                    e
                                );
                            }
                        }
                    } else {
                        tracing::warn!("daemon: session '{}' is not alive", session_name);
                    }
                } else {
                    tracing::warn!(
                        "daemon: session '{}' not found for client '{}'",
                        session_name,
                        client_id
                    );
                }
            } else {
                tracing::warn!("daemon: no session attached for client '{}'", client_id);
            }
            None
        }

        DaemonRequest::Resize {
            client_id: _,
            session,
            cols,
            rows,
        } => {
            if let Some(session_name) = session {
                let mut sessions = state.sessions.lock().await;
                if let Some(session) = sessions.get_mut(&session_name) {
                    let _ = session.resize(cols, rows);
                }
            }
            None
        }

        DaemonRequest::Detach { client_id, session } => {
            let mut attachments = state.client_attachments.lock().await;
            if let Some(session_name) = session {
                if let Some(sessions) = attachments.get_mut(&client_id) {
                    sessions.remove(&session_name);
                    if sessions.is_empty() {
                        attachments.remove(&client_id);
                    }
                }
            } else {
                attachments.remove(&client_id);
            }
            None
        }

        DaemonRequest::UpdateAgentEnv { id, env } => {
            let mut agent_env = state.agent_env.lock().await;
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

            // Inject into all running sessions so they pick up the change immediately
            {
                let sessions = state.sessions.lock().await;
                let snapshot = agent_env.clone();
                for session in sessions.values() {
                    session.backend.inject_env(&snapshot);
                }
            }

            Some(DaemonResponse::EnvUpdated { id })
        }

        DaemonRequest::UpdateSessionEnv { id, session, env } => {
            // Acquire agent_env first to match lock ordering with UpdateAgentEnv
            let global_env = state.agent_env.lock().await;
            let global_snapshot = global_env.clone();
            drop(global_env);

            let mut sessions = state.sessions.lock().await;
            if let Some(s) = sessions.get_mut(&session) {
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
                    session,
                    s.env.len()
                );

                // Inject merged global + session env into the running container
                let mut merged = global_snapshot;
                merged.extend(s.env.clone());
                s.backend.inject_env(&merged);

                Some(DaemonResponse::SessionEnvUpdated { id, session })
            } else {
                Some(DaemonResponse::Error {
                    id,
                    error: format!("session '{}' not found", session),
                })
            }
        }

        DaemonRequest::OpenBundle {
            id,
            path,
            cols,
            rows,
        } => {
            match super::bundle::open_bundle(&path).await {
                Ok(bundle) => {
                    let name = bundle.name.clone();
                    let canonical_path = bundle.path.clone();
                    let kubo_name = "default";

                    // Ensure default kubo exists
                    if let Err(e) = state.ensure_default_kubo().await {
                        return Some(DaemonResponse::Error {
                            id,
                            error: format!("failed to ensure default kubo: {}", e),
                        });
                    }

                    // Create worktree in the default kubo so the session
                    // filesystem lives inside the kubo directory
                    let kubo_path = {
                        let kubos = state.kubos.lock().await;
                        match kubos.get(kubo_name) {
                            Some(k) => k.path.clone(),
                            None => {
                                return Some(DaemonResponse::Error {
                                    id,
                                    error: "kubo 'default' not found".to_string(),
                                })
                            }
                        }
                    };

                    if let Err(e) = super::bundle::worktree_add_abot(
                        &canonical_path,
                        &kubo_path,
                        &name,
                        kubo_name,
                    ) {
                        return Some(DaemonResponse::Error {
                            id,
                            error: format!("failed to add worktree for opened bundle: {}", e),
                        });
                    }

                    // Update kubo manifest
                    match super::kubo::Kubo::read_manifest(&kubo_path) {
                        Ok(mut manifest) => {
                            if !manifest.abots.contains(&name) {
                                manifest.abots.push(name.clone());
                                manifest.updated_at = Some(chrono::Utc::now().to_rfc3339());
                                if let Err(e) =
                                    super::kubo::Kubo::write_manifest(&kubo_path, &manifest)
                                {
                                    tracing::warn!(
                                        "failed to write kubo manifest for '{}': {}",
                                        kubo_name,
                                        e
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                "failed to read kubo manifest for '{}': {}",
                                kubo_name,
                                e
                            );
                        }
                    }

                    // Point bundle_path at the worktree (inside the kubo), not
                    // the canonical abot. This is where the session filesystem
                    // lives and where autosave will write.
                    let worktree_path = kubo_path.join(&name);

                    let result = state
                        .create_kubo_backend(kubo_name, &name, cols, rows, &bundle.env)
                        .await;

                    match result {
                        Ok(backend) => {
                            let session = Session::new(
                                name.clone(),
                                backend,
                                bundle.env.clone(),
                                Some(worktree_path.clone()),
                                Some(kubo_name.to_string()),
                            );
                            let session_name = session.name.clone();

                            let output_tx = state.output_tx.clone();
                            let reader_name = session_name.clone();
                            let state_ref = state.clone();

                            let mut sessions = state.sessions.lock().await;
                            // Kill existing session with same name to prevent resource leaks
                            let old_kubo_name = if let Some(mut old) = sessions.remove(&name) {
                                let kn = if old.is_alive() {
                                    old.kubo.clone()
                                } else {
                                    None
                                };
                                old.backend.kill();
                                let _ = state.output_tx.send(OutputEvent::SessionRemoved {
                                    session: name.clone(),
                                });
                                kn
                            } else {
                                None
                            };
                            sessions.insert(name.clone(), session);

                            let rx = sessions
                                .get_mut(&name)
                                .and_then(|s| s.backend.take_reader());
                            let shared_name = sessions.get(&name).map(|s| s.shared_name.clone());
                            let gen = sessions.get(&name).map(|s| s.generation).unwrap_or(0);
                            drop(sessions);

                            if let Some(kn) = old_kubo_name {
                                let mut kubos = state.kubos.lock().await;
                                if let Some(kubo) = kubos.get_mut(&kn) {
                                    kubo.session_closed();
                                }
                            }

                            // Restore saved scrollback into the ring buffer
                            if let Some(scrollback) = super::bundle::load_scrollback(&worktree_path)
                            {
                                let mut sessions = state.sessions.lock().await;
                                if let Some(s) = sessions.get_mut(&name) {
                                    s.buffer.pre_populate(scrollback);
                                }
                            }

                            // Inject credentials into the new container
                            if !bundle.env.is_empty() {
                                let global_env = state.agent_env.lock().await;
                                let mut merged = global_env.clone();
                                merged.extend(bundle.env);
                                let sessions = state.sessions.lock().await;
                                if let Some(s) = sessions.get(&name) {
                                    s.backend.inject_env(&merged);
                                }
                            }

                            if let Some(mut rx) = rx {
                                spawn_output_relay(
                                    output_tx,
                                    state_ref,
                                    shared_name,
                                    reader_name,
                                    &mut rx,
                                    gen,
                                );
                            }

                            // Track in known abots registry
                            super::bundle::add_known_abot(&state.data_dir, &session_name);

                            Some(DaemonResponse::Opened {
                                id,
                                name: session_name,
                                path: worktree_path.to_string_lossy().to_string(),
                            })
                        }
                        Err(e) => Some(DaemonResponse::Error {
                            id,
                            error: format!("failed to create session from bundle: {}", e),
                        }),
                    }
                }
                Err(e) => Some(DaemonResponse::Error {
                    id,
                    error: format!("open failed: {}", e),
                }),
            }
        }

        DaemonRequest::SaveSession { id, session } => {
            let sessions = state.sessions.lock().await;
            if let Some(s) = sessions.get(&session) {
                let bundle_path = match &s.bundle_path {
                    Some(p) => p.clone(),
                    None => {
                        return Some(DaemonResponse::Error {
                            id,
                            error: format!(
                                "session '{}' has no bundle path (use save-as)",
                                session
                            ),
                        })
                    }
                };
                let env = s.env.clone();
                let name = s.name.clone();
                let scrollback = s.get_buffer();
                drop(sessions);

                match super::bundle::save_bundle(&bundle_path, &name, &env).await {
                    Ok(()) => {
                        super::bundle::save_scrollback(&bundle_path, &scrollback);
                        // Clear dirty flag
                        let mut sessions = state.sessions.lock().await;
                        if let Some(s) = sessions.get_mut(&session) {
                            s.dirty = false;
                        }
                        Some(DaemonResponse::Saved {
                            id,
                            session,
                            path: bundle_path.to_string_lossy().to_string(),
                        })
                    }
                    Err(e) => Some(DaemonResponse::Error {
                        id,
                        error: format!("save failed: {}", e),
                    }),
                }
            } else {
                Some(DaemonResponse::Error {
                    id,
                    error: format!("session '{}' not found", session),
                })
            }
        }

        DaemonRequest::SaveSessionAs { id, session, path } => {
            // Reject save paths inside another .abot bundle
            {
                let mut check = std::path::Path::new(&path);
                while let Some(parent) = check.parent() {
                    if parent
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("abot"))
                    {
                        return Some(DaemonResponse::Error {
                            id,
                            error: format!(
                                "cannot save inside another .abot bundle: {}",
                                parent.display()
                            ),
                        });
                    }
                    check = parent;
                }
            }

            let sessions = state.sessions.lock().await;
            if let Some(s) = sessions.get(&session) {
                let env = s.env.clone();
                let name = s.name.clone();
                let existing_bundle = s.bundle_path.clone();
                let scrollback = s.get_buffer();
                drop(sessions);

                let new_bundle_path = PathBuf::from(&path);

                // Copy the existing bundle directory if available, then write metadata
                if let Some(ref src) = existing_bundle {
                    if let Err(e) = super::bundle::copy_dir_recursive(src, &new_bundle_path) {
                        return Some(DaemonResponse::Error {
                            id,
                            error: format!("save-as copy failed: {}", e),
                        });
                    }
                }

                match super::bundle::save_bundle(&new_bundle_path, &name, &env).await {
                    Ok(()) => {
                        super::bundle::save_scrollback(&new_bundle_path, &scrollback);
                        // Update bundle_path and clear dirty flag
                        let mut sessions = state.sessions.lock().await;
                        if let Some(s) = sessions.get_mut(&session) {
                            s.bundle_path = Some(new_bundle_path.clone());
                            s.dirty = false;
                        }
                        Some(DaemonResponse::Saved {
                            id,
                            session,
                            path: new_bundle_path.to_string_lossy().to_string(),
                        })
                    }
                    Err(e) => Some(DaemonResponse::Error {
                        id,
                        error: format!("save-as failed: {}", e),
                    }),
                }
            } else {
                Some(DaemonResponse::Error {
                    id,
                    error: format!("session '{}' not found", session),
                })
            }
        }

        DaemonRequest::CloseSession { id, session, save } => {
            // Optionally save first
            if save {
                let sessions = state.sessions.lock().await;
                if let Some(s) = sessions.get(&session) {
                    if let Some(bundle_path) = &s.bundle_path {
                        let bundle_path = bundle_path.clone();
                        let env = s.env.clone();
                        let name = s.name.clone();
                        drop(sessions);

                        if let Err(e) = super::bundle::save_bundle(&bundle_path, &name, &env).await
                        {
                            return Some(DaemonResponse::Error {
                                id,
                                error: format!("save before close failed: {}", e),
                            });
                        }
                    } else {
                        return Some(DaemonResponse::Error {
                            id,
                            error: format!(
                                "session '{}' has no bundle path (use save-as before close with save)",
                                session
                            ),
                        });
                    }
                } else {
                    return Some(DaemonResponse::Error {
                        id,
                        error: format!("session '{}' not found", session),
                    });
                }
            }

            // Kill and remove
            let mut sessions = state.sessions.lock().await;
            if let Some(mut s) = sessions.remove(&session) {
                // Save scrollback to bundle before killing (always, regardless of save flag)
                if let Some(ref bp) = s.bundle_path {
                    super::bundle::save_scrollback(bp, &s.get_buffer());
                }
                // Only decrement kubo count if the session was still running
                // (if already exited, the output relay already called session_closed)
                let kubo_name = if s.is_alive() { s.kubo.clone() } else { None };
                s.backend.kill();
                drop(sessions);
                if let Some(kn) = kubo_name {
                    let mut kubos = state.kubos.lock().await;
                    if let Some(kubo) = kubos.get_mut(&kn) {
                        kubo.session_closed();
                    }
                }
                let _ = state.output_tx.send(OutputEvent::SessionRemoved {
                    session: session.clone(),
                });
                Some(DaemonResponse::Closed { id, session })
            } else {
                Some(DaemonResponse::Error {
                    id,
                    error: format!("session '{}' not found", session),
                })
            }
        }

        DaemonRequest::Ping { id } => Some(DaemonResponse::Pong { id }),

        // ── Kubo management ───────────────────────────────────────────
        DaemonRequest::CreateKubo { id, name } => {
            let kubos_dir = super::bundle::resolve_kubos_dir(&state.data_dir);
            match super::kubo::Kubo::ensure_kubo_dir(&kubos_dir, &name) {
                Ok(kubo_path) => match super::kubo::new_kubo(name.clone(), kubo_path.clone()) {
                    Ok(new_kubo) => {
                        let mut kubos = state.kubos.lock().await;
                        kubos.insert(name.clone(), new_kubo);
                        Some(DaemonResponse::KuboCreated {
                            id,
                            name,
                            path: kubo_path.to_string_lossy().to_string(),
                        })
                    }
                    Err(e) => Some(DaemonResponse::Error {
                        id,
                        error: format!("failed to create kubo: {}", e),
                    }),
                },
                Err(e) => Some(DaemonResponse::Error {
                    id,
                    error: format!("failed to create kubo dir: {}", e),
                }),
            }
        }

        DaemonRequest::ListKubos { id } => {
            let kubos = state.kubos.lock().await;
            let list: Vec<serde_json::Value> = kubos.values().map(|k| k.to_json()).collect();
            Some(DaemonResponse::KuboList { id, kubos: list })
        }

        DaemonRequest::StartKubo { id, name } => {
            let mut kubos = state.kubos.lock().await;
            if let Some(kubo) = kubos.get_mut(&name) {
                match kubo.start().await {
                    Ok(()) => Some(DaemonResponse::KuboStarted { id, name }),
                    Err(e) => Some(DaemonResponse::Error {
                        id,
                        error: format!("failed to start kubo: {}", e),
                    }),
                }
            } else {
                Some(DaemonResponse::Error {
                    id,
                    error: format!("kubo '{}' not found", name),
                })
            }
        }

        DaemonRequest::StopKubo { id, name } => {
            let mut kubos = state.kubos.lock().await;
            if let Some(kubo) = kubos.get_mut(&name) {
                match kubo.stop().await {
                    Ok(()) => Some(DaemonResponse::KuboStopped { id, name }),
                    Err(e) => Some(DaemonResponse::Error {
                        id,
                        error: format!("failed to stop kubo: {}", e),
                    }),
                }
            } else {
                Some(DaemonResponse::Error {
                    id,
                    error: format!("kubo '{}' not found", name),
                })
            }
        }

        DaemonRequest::OpenKubo { id, path } => {
            let kubo_path = PathBuf::from(&path);
            if !kubo_path.exists() || !kubo_path.is_dir() {
                return Some(DaemonResponse::Error {
                    id,
                    error: format!("kubo path does not exist: {}", path),
                });
            }

            // Derive the kubo name from the directory name (strip .kubo suffix)
            let dir_name = kubo_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let name = dir_name
                .strip_suffix(".kubo")
                .unwrap_or(dir_name)
                .to_string();

            if name.is_empty() {
                return Some(DaemonResponse::Error {
                    id,
                    error: "could not determine kubo name from path".to_string(),
                });
            }

            // Validate the name
            if let Err(e) = super::kubo::validate_name(&name) {
                return Some(DaemonResponse::Error {
                    id,
                    error: format!("invalid kubo name: {}", e),
                });
            }

            // Ensure manifest exists (create one if missing)
            let manifest_path = kubo_path.join("manifest.json");
            if !manifest_path.exists() {
                let now = chrono::Utc::now().to_rfc3339();
                let manifest = super::kubo::KuboManifest {
                    version: 1,
                    name: name.clone(),
                    created_at: now.clone(),
                    updated_at: Some(now),
                    abots: vec![],
                };
                if let Err(e) = super::kubo::Kubo::write_manifest(&kubo_path, &manifest) {
                    return Some(DaemonResponse::Error {
                        id,
                        error: format!("failed to write manifest: {}", e),
                    });
                }
            }

            // Register the kubo in the daemon
            let mut kubos = state.kubos.lock().await;
            if kubos.contains_key(&name) {
                // Already registered — just return success
                return Some(DaemonResponse::KuboOpened { id, name, path });
            }
            match super::kubo::new_kubo(name.clone(), kubo_path) {
                Ok(k) => {
                    kubos.insert(name.clone(), k);
                    tracing::info!("opened kubo '{}' from {}", name, path);
                    Some(DaemonResponse::KuboOpened { id, name, path })
                }
                Err(e) => Some(DaemonResponse::Error {
                    id,
                    error: format!("failed to open kubo: {}", e),
                }),
            }
        }

        DaemonRequest::CloneAbot { id, source, target } => {
            if let Err(e) = super::kubo::validate_name(&source) {
                return Some(DaemonResponse::Error {
                    id,
                    error: format!("invalid source name: {}", e),
                });
            }
            if let Err(e) = super::kubo::validate_name(&target) {
                return Some(DaemonResponse::Error {
                    id,
                    error: format!("invalid target name: {}", e),
                });
            }
            let abots_dir = super::bundle::resolve_abots_dir(&state.data_dir);
            let mut source_path = abots_dir.join(format!("{}.abot", source));
            let target_path = abots_dir.join(format!("{}.abot", target));

            if !source_path.exists() {
                // Try legacy bundles/ path
                let legacy_source = state
                    .data_dir
                    .join("bundles")
                    .join(format!("{}.abot", source));
                if !legacy_source.exists() {
                    return Some(DaemonResponse::Error {
                        id,
                        error: format!("source abot '{}' not found", source),
                    });
                }
                source_path = legacy_source;
            }

            if target_path.exists() {
                return Some(DaemonResponse::Error {
                    id,
                    error: format!("target abot '{}' already exists", target),
                });
            }

            // Clone git repo
            let source_str = source_path.to_string_lossy().to_string();
            let target_str = target_path.to_string_lossy().to_string();
            match std::process::Command::new("git")
                .args(["clone", &source_str, &target_str])
                .output()
            {
                Ok(output) if output.status.success() => {
                    // Update manifest name in clone
                    let manifest_path = target_path.join("manifest.json");
                    if let Ok(mut manifest) = super::bundle::read_json(&manifest_path) {
                        manifest["name"] = serde_json::Value::String(target.clone());
                        let _ = super::bundle::write_json(&manifest_path, &manifest);
                    }
                    Some(DaemonResponse::AbotCloned {
                        id,
                        source,
                        target,
                        path: target_str,
                    })
                }
                Ok(output) => {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    Some(DaemonResponse::Error {
                        id,
                        error: format!("git clone failed: {}", stderr),
                    })
                }
                Err(e) => Some(DaemonResponse::Error {
                    id,
                    error: format!("failed to run git clone: {}", e),
                }),
            }
        }

        DaemonRequest::AbotGit { id, abot, op } => {
            if let Err(e) = super::kubo::validate_name(&abot) {
                return Some(DaemonResponse::Error {
                    id,
                    error: format!("invalid abot name: {}", e),
                });
            }
            let abots_dir = super::bundle::resolve_abots_dir(&state.data_dir);
            let mut abot_path = abots_dir.join(format!("{}.abot", abot));

            if !abot_path.exists() {
                // Try legacy
                let legacy = state
                    .data_dir
                    .join("bundles")
                    .join(format!("{}.abot", abot));
                if !legacy.exists() {
                    return Some(DaemonResponse::Error {
                        id,
                        error: format!("abot '{}' not found", abot),
                    });
                }
                abot_path = legacy;
            }

            let args: Vec<&str> = match op.as_str() {
                "status" => vec!["status", "--short"],
                "log" => vec!["log", "--oneline", "-20"],
                "diff" => vec!["diff"],
                _ => {
                    return Some(DaemonResponse::Error {
                        id,
                        error: format!("unsupported git op: {}", op),
                    })
                }
            };

            match std::process::Command::new("git")
                .args(&args)
                .current_dir(&abot_path)
                .output()
            {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    Some(DaemonResponse::AbotGitResult {
                        id,
                        abot,
                        op,
                        output: stdout,
                    })
                }
                Err(e) => Some(DaemonResponse::Error {
                    id,
                    error: format!("git failed: {}", e),
                }),
            }
        }

        DaemonRequest::AddAbotToKubo {
            id,
            kubo,
            abot,
            create_session,
            cols,
            rows,
            env,
        } => {
            // Ensure canonical abot + worktree + kubo manifest
            if let Err(error) = ensure_abot_in_kubo(state, &abot, &kubo).await {
                return Some(DaemonResponse::Error { id, error });
            }

            // Track in known abots registry
            super::bundle::add_known_abot(&state.data_dir, &abot);

            // Optionally create a session. If the abot already has a LIVE
            // session in a different kubo, skip session creation to avoid
            // killing it. Dead sessions or sessions in the same kubo are
            // replaced normally.
            let should_create = if create_session {
                let sessions = state.sessions.lock().await;
                let existing = sessions.get(&abot);
                !matches!(existing, Some(s) if s.is_alive() && s.kubo.as_deref() != Some(&kubo))
            } else {
                false
            };

            if should_create {
                match handle_create_session(
                    state,
                    id.clone(),
                    abot.clone(),
                    cols,
                    rows,
                    env,
                    kubo.clone(),
                )
                .await
                {
                    Some(DaemonResponse::SessionCreated { name, .. }) => {
                        Some(DaemonResponse::AbotAddedToKubo {
                            id,
                            kubo,
                            abot,
                            session: Some(name),
                        })
                    }
                    Some(DaemonResponse::Error { error, .. }) => Some(DaemonResponse::Error {
                        id,
                        error: format!("abot added but session creation failed: {}", error),
                    }),
                    _ => Some(DaemonResponse::AbotAddedToKubo {
                        id,
                        kubo,
                        abot,
                        session: None,
                    }),
                }
            } else {
                Some(DaemonResponse::AbotAddedToKubo {
                    id,
                    kubo,
                    abot,
                    session: None,
                })
            }
        }

        DaemonRequest::RemoveAbotFromKubo { id, kubo, abot } => {
            // Validate names
            if let Err(e) = super::kubo::validate_name(&abot) {
                return Some(DaemonResponse::Error {
                    id,
                    error: format!("invalid abot name: {}", e),
                });
            }
            if let Err(e) = super::kubo::validate_name(&kubo) {
                return Some(DaemonResponse::Error {
                    id,
                    error: format!("invalid kubo name: {}", e),
                });
            }

            // Close the session if one exists for this abot in this kubo
            {
                let mut sessions = state.sessions.lock().await;
                if let Some(mut session) = sessions.remove(&abot) {
                    if session.kubo.as_deref() == Some(&kubo) {
                        let kn = if session.is_alive() {
                            session.kubo.clone()
                        } else {
                            None
                        };
                        session.backend.kill();
                        let _ = state.output_tx.send(OutputEvent::SessionRemoved {
                            session: abot.clone(),
                        });
                        drop(sessions);
                        if let Some(kn) = kn {
                            let mut kubos = state.kubos.lock().await;
                            if let Some(k) = kubos.get_mut(&kn) {
                                k.session_closed();
                            }
                        }
                    } else {
                        // Session exists but belongs to a different kubo — put it back
                        sessions.insert(abot.clone(), session);
                    }
                }
            }

            // Get kubo path and resolve canonical abot path
            let kubo_path = {
                let kubos = state.kubos.lock().await;
                match kubos.get(&kubo) {
                    Some(k) => k.path.clone(),
                    None => {
                        return Some(DaemonResponse::Error {
                            id,
                            error: format!("kubo '{}' not found", kubo),
                        })
                    }
                }
            };

            // Keep the worktree on disk so the abot can be re-added later
            // (resume semantics). Only update the manifest.

            // Update kubo manifest: remove abot from list
            match super::kubo::Kubo::read_manifest(&kubo_path) {
                Ok(mut manifest) => {
                    manifest.abots.retain(|a| a != &abot);
                    manifest.updated_at = Some(chrono::Utc::now().to_rfc3339());
                    if let Err(e) = super::kubo::Kubo::write_manifest(&kubo_path, &manifest) {
                        tracing::warn!("failed to write kubo manifest for '{}': {}", kubo, e);
                    }
                }
                Err(e) => {
                    tracing::warn!("failed to read kubo manifest for '{}': {}", kubo, e);
                }
            }

            Some(DaemonResponse::AbotRemovedFromKubo { id, kubo, abot })
        }

        // ── Known abots registry ─────────────────────────────────────
        DaemonRequest::ListAbots { id } => {
            let abots = super::bundle::read_known_abots(&state.data_dir);
            let list: Vec<serde_json::Value> = abots
                .iter()
                .map(|a| {
                    // Include detail inline to avoid N+1 fetches
                    match super::bundle::get_abot_detail(&state.data_dir, &a.name) {
                        Ok(detail) => {
                            let mut val = serde_json::to_value(&detail)
                                .unwrap_or_else(|_| serde_json::json!({ "name": a.name }));
                            if let Some(obj) = val.as_object_mut() {
                                obj.insert("added_at".to_string(), serde_json::json!(a.added_at));
                            }
                            val
                        }
                        Err(_) => serde_json::json!({ "name": a.name, "added_at": a.added_at }),
                    }
                })
                .collect();
            Some(DaemonResponse::AbotList { id, abots: list })
        }

        DaemonRequest::GetAbotInfo { id, abot } => {
            match super::bundle::get_abot_detail(&state.data_dir, &abot) {
                Ok(detail) => {
                    let val = serde_json::to_value(&detail).unwrap_or_default();
                    Some(DaemonResponse::AbotInfo { id, abot: val })
                }
                Err(e) => Some(DaemonResponse::Error {
                    id,
                    error: e.to_string(),
                }),
            }
        }

        DaemonRequest::RemoveKnownAbot { id, abot } => {
            super::bundle::remove_known_abot(&state.data_dir, &abot);
            Some(DaemonResponse::KnownAbotRemoved { id, abot })
        }
    }
}

/// Spawn a task that relays output from a session's reader channel to the broadcast.
/// `generation` is the session's generation at spawn time — used to detect stale relays
/// when a session name is reused (overwritten).
fn spawn_output_relay(
    output_tx: tokio::sync::broadcast::Sender<OutputEvent>,
    state_ref: Arc<DaemonState>,
    shared_name: Option<std::sync::Arc<std::sync::Mutex<String>>>,
    reader_name: String,
    rx: &mut tokio::sync::mpsc::Receiver<String>,
    generation: u64,
) {
    let mut rx = {
        // We need to take ownership — swap with a dummy channel
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
                let mut sessions = state_ref.sessions.lock().await;
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
            // Only broadcast if this relay belongs to the current session generation
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

        // Only mark exited and call session_closed if the session still belongs to this
        // generation. If the session was overwritten (new session with same name), the
        // generation will differ and we must not touch the new session's state.
        let (code, kubo_name) = {
            let mut sessions = state_ref.sessions.lock().await;
            if let Some(s) = sessions.get_mut(&current_name) {
                if s.generation == generation {
                    let code = s.backend.try_exit_code().unwrap_or(0);
                    s.mark_exited(code);
                    (Some(code), s.kubo.clone())
                } else {
                    // Stale relay — session was overwritten, skip cleanup
                    (None, None)
                }
            } else {
                (None, None)
            }
        };
        if let Some(kn) = kubo_name {
            let mut kubos = state_ref.kubos.lock().await;
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

/// Ensure a canonical abot exists and has a worktree in the given kubo.
/// Returns the worktree path (the abot's dir inside the kubo).
/// Both `handle_create_session` and `AddAbotToKubo` delegate to this.
async fn ensure_abot_in_kubo(
    state: &Arc<DaemonState>,
    name: &str,
    kubo: &str,
) -> Result<PathBuf, String> {
    // Validate names
    if let Err(e) = super::kubo::validate_name(name) {
        return Err(format!("invalid abot name: {}", e));
    }
    if let Err(e) = super::kubo::validate_name(kubo) {
        return Err(format!("invalid kubo name: {}", e));
    }

    // Ensure default kubo exists if "default" requested
    if kubo == "default" {
        if let Err(e) = state.ensure_default_kubo().await {
            return Err(format!("failed to ensure default kubo: {}", e));
        }
    }

    // Resolve canonical abots dir and create the canonical abot
    let abots_dir = super::bundle::resolve_abots_dir(&state.data_dir);
    if let Err(e) = std::fs::create_dir_all(&abots_dir) {
        tracing::warn!("failed to create abots dir: {}", e);
    }
    let canonical_path = super::bundle::create_canonical_abot(&abots_dir, name)
        .map_err(|e| format!("failed to create canonical abot: {}", e))?;

    // Get kubo path
    let kubo_path = {
        let kubos = state.kubos.lock().await;
        match kubos.get(kubo) {
            Some(k) => k.path.clone(),
            None => return Err(format!("kubo '{}' not found", kubo)),
        }
    };

    // Create worktree in the kubo (skips if already set up)
    super::bundle::worktree_add_abot(&canonical_path, &kubo_path, name, kubo)
        .map_err(|e| format!("failed to add worktree: {}", e))?;

    // Update kubo manifest (add abot to list if not present)
    match super::kubo::Kubo::read_manifest(&kubo_path) {
        Ok(mut manifest) => {
            if !manifest.abots.contains(&name.to_string()) {
                manifest.abots.push(name.to_string());
                manifest.updated_at = Some(chrono::Utc::now().to_rfc3339());
                if let Err(e) = super::kubo::Kubo::write_manifest(&kubo_path, &manifest) {
                    tracing::warn!("failed to write kubo manifest for '{}': {}", kubo, e);
                }
            }
        }
        Err(e) => {
            tracing::warn!("failed to read kubo manifest for '{}': {}", kubo, e);
        }
    }

    let worktree_path = kubo_path.join(name);
    Ok(worktree_path)
}

/// Create a PTY session and spawn its output reader task.
/// The session runs inside the named kubo container via `docker exec`.
/// Also ensures a canonical abot + worktree exist for the session.
async fn handle_create_session(
    state: &Arc<DaemonState>,
    id: String,
    name: String,
    cols: u16,
    rows: u16,
    env: HashMap<String, String>,
    kubo: String,
) -> Option<DaemonResponse> {
    // Ensure canonical abot + worktree + kubo manifest
    let abot_dir = match ensure_abot_in_kubo(state, &name, &kubo).await {
        Ok(path) => path,
        Err(error) => return Some(DaemonResponse::Error { id, error }),
    };

    let backend_result = state
        .create_kubo_backend(&kubo, &name, cols, rows, &env)
        .await;
    let bundle_path = Some(abot_dir);

    match backend_result {
        Ok(backend) => {
            // Write initial manifest (best-effort; autosave will retry)
            if let Some(ref bp) = bundle_path {
                if let Err(e) = super::bundle::save_bundle(bp, &name, &env).await {
                    tracing::warn!("failed to write initial manifest for '{}': {}", name, e);
                }
            }

            let session = Session::new(name.clone(), backend, env, bundle_path.clone(), Some(kubo));
            let session_name = session.name.clone();

            let output_tx = state.output_tx.clone();
            let reader_name = session_name.clone();
            let state_ref = state.clone();

            let mut sessions = state.sessions.lock().await;
            // Kill old session if it exists to avoid orphaning backends
            let old_kubo_name = if let Some(mut old) = sessions.remove(&name) {
                // Only decrement kubo count if the session was still running
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
            sessions.insert(name.clone(), session);

            let rx = sessions
                .get_mut(&name)
                .and_then(|s| s.backend.take_reader());
            let shared_name = sessions.get(&name).map(|s| s.shared_name.clone());
            let gen = sessions.get(&name).map(|s| s.generation).unwrap_or(0);
            drop(sessions);

            // Decrement session count for the old session's kubo (if overwriting)
            if let Some(kn) = old_kubo_name {
                let mut kubos = state.kubos.lock().await;
                if let Some(kubo) = kubos.get_mut(&kn) {
                    kubo.session_closed();
                }
            }

            // Restore saved scrollback from a previous session (e.g. after close + restart)
            if let Some(ref bp) = bundle_path {
                if let Some(scrollback) = super::bundle::load_scrollback(bp) {
                    let mut sessions = state.sessions.lock().await;
                    if let Some(s) = sessions.get_mut(&name) {
                        s.buffer.pre_populate(scrollback);
                    }
                }
            }

            if let Some(mut rx) = rx {
                spawn_output_relay(output_tx, state_ref, shared_name, reader_name, &mut rx, gen);
            }

            Some(DaemonResponse::SessionCreated {
                id,
                name: session_name,
            })
        }
        Err(e) => Some(DaemonResponse::Error {
            id,
            error: format!("failed to create session: {}", e),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_daemon_request_serializes_roundtrip() {
        let req = DaemonRequest::ListSessions {
            id: "abc".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        match parsed {
            DaemonRequest::ListSessions { id } => assert_eq!(id, "abc"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_daemon_request_input_serde() {
        let req = DaemonRequest::Input {
            client_id: "c1".to_string(),
            session: Some("main".to_string()),
            data: "hello".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""type":"input""#));
        assert!(json.contains(r#""clientId":"c1""#));

        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        match parsed {
            DaemonRequest::Input {
                client_id,
                session,
                data,
            } => {
                assert_eq!(client_id, "c1");
                assert_eq!(session, Some("main".to_string()));
                assert_eq!(data, "hello");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_daemon_request_ping_serde() {
        let req = DaemonRequest::Ping {
            id: "p1".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""type":"ping""#));

        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        match parsed {
            DaemonRequest::Ping { id } => assert_eq!(id, "p1"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_daemon_response_pong_serializes() {
        let resp = DaemonResponse::Pong {
            id: "p1".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""id":"p1""#));
    }

    #[test]
    fn test_open_bundle_request_serde() {
        let json = r#"{"type":"open-bundle","id":"x","path":"/tmp/test.abot","cols":80,"rows":24}"#;
        let parsed: DaemonRequest = serde_json::from_str(json).unwrap();
        match parsed {
            DaemonRequest::OpenBundle {
                id,
                path,
                cols,
                rows,
            } => {
                assert_eq!(id, "x");
                assert_eq!(path, "/tmp/test.abot");
                assert_eq!(cols, 80);
                assert_eq!(rows, 24);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_save_session_request_serde() {
        let json = r#"{"type":"save-session","id":"x","session":"main"}"#;
        let parsed: DaemonRequest = serde_json::from_str(json).unwrap();
        match parsed {
            DaemonRequest::SaveSession { id, session } => {
                assert_eq!(id, "x");
                assert_eq!(session, "main");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_close_session_request_serde() {
        let json = r#"{"type":"close-session","id":"x","session":"main","save":true}"#;
        let parsed: DaemonRequest = serde_json::from_str(json).unwrap();
        match parsed {
            DaemonRequest::CloseSession { id, session, save } => {
                assert_eq!(id, "x");
                assert_eq!(session, "main");
                assert!(save);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_remove_abot_from_kubo_roundtrip() {
        // Deserialize
        let json = r#"{"type":"remove-abot-from-kubo","id":"x","kubo":"default","abot":"alice"}"#;
        let parsed: DaemonRequest = serde_json::from_str(json).unwrap();
        match &parsed {
            DaemonRequest::RemoveAbotFromKubo { id, kubo, abot } => {
                assert_eq!(id, "x");
                assert_eq!(kubo, "default");
                assert_eq!(abot, "alice");
            }
            _ => panic!("wrong variant"),
        }

        // Serialize (what the server does) — then inject id (what rpc_with_timeout does)
        let req = DaemonRequest::RemoveAbotFromKubo {
            id: String::new(),
            kubo: "default".into(),
            abot: "alice".into(),
        };
        let mut value = serde_json::to_value(&req).unwrap();
        value["id"] = serde_json::json!("test-uuid");
        let serialized = serde_json::to_string(&value).unwrap();
        let reparsed: DaemonRequest = serde_json::from_str(&serialized).unwrap();
        match reparsed {
            DaemonRequest::RemoveAbotFromKubo { id, kubo, abot } => {
                assert_eq!(id, "test-uuid");
                assert_eq!(kubo, "default");
                assert_eq!(abot, "alice");
            }
            _ => panic!("wrong variant after roundtrip"),
        }
    }

    #[test]
    fn test_abot_removed_from_kubo_response_serializes() {
        let resp = DaemonResponse::AbotRemovedFromKubo {
            id: "r1".into(),
            kubo: "default".into(),
            abot: "alice".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""kubo":"default""#));
        assert!(json.contains(r#""abot":"alice""#));
    }

    #[test]
    fn test_close_session_save_defaults_false() {
        let json = r#"{"type":"close-session","id":"x","session":"main"}"#;
        let parsed: DaemonRequest = serde_json::from_str(json).unwrap();
        match parsed {
            DaemonRequest::CloseSession { save, .. } => {
                assert!(!save);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_open_bundle_uses_default_cols_rows() {
        let json = r#"{"type":"open-bundle","id":"x","path":"/tmp/test.abot"}"#;
        let parsed: DaemonRequest = serde_json::from_str(json).unwrap();
        match parsed {
            DaemonRequest::OpenBundle { cols, rows, .. } => {
                assert_eq!(cols, 120);
                assert_eq!(rows, 40);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_save_session_as_request_serde() {
        let json =
            r#"{"type":"save-session-as","id":"x","session":"proj","path":"/home/u/proj.abot"}"#;
        let parsed: DaemonRequest = serde_json::from_str(json).unwrap();
        match parsed {
            DaemonRequest::SaveSessionAs { id, session, path } => {
                assert_eq!(id, "x");
                assert_eq!(session, "proj");
                assert_eq!(path, "/home/u/proj.abot");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_opened_response_serializes() {
        let resp = DaemonResponse::Opened {
            id: "r1".into(),
            name: "proj".into(),
            path: "/tmp/proj.abot".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""name":"proj""#));
        assert!(json.contains(r#""path":"/tmp/proj.abot""#));
    }

    #[test]
    fn test_saved_response_serializes() {
        let resp = DaemonResponse::Saved {
            id: "r1".into(),
            session: "main".into(),
            path: "/tmp/main.abot".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""session":"main""#));
        assert!(json.contains(r#""path":"/tmp/main.abot""#));
    }

    #[test]
    fn test_closed_response_serializes() {
        let resp = DaemonResponse::Closed {
            id: "r1".into(),
            session: "main".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""session":"main""#));
    }
}
