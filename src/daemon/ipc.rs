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
        } => handle_create_session(state, id, name, cols, rows, env).await,

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
            let bundle_path = {
                let mut sessions = state.sessions.lock().await;
                if let Some(mut session) = sessions.remove(&name) {
                    let bp = session.bundle_path.clone();
                    session.backend.kill();
                    let _ = state.output_tx.send(OutputEvent::SessionRemoved {
                        session: name.clone(),
                    });
                    bp
                } else {
                    return Some(DaemonResponse::Error {
                        id,
                        error: format!("session '{}' not found", name),
                    });
                }
            };
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
                    let bundle_path = bundle.path.clone();
                    let home_bind = bundle_path.join("home");
                    let result = state
                        .create_backend_with_env(&name, cols, rows, &bundle.env, &home_bind)
                        .await;

                    match result {
                        Ok(backend) => {
                            let session = Session::new(
                                name.clone(),
                                backend,
                                bundle.env.clone(),
                                Some(bundle_path.clone()),
                            );
                            let session_name = session.name.clone();

                            let output_tx = state.output_tx.clone();
                            let reader_name = session_name.clone();
                            let state_ref = state.clone();

                            let mut sessions = state.sessions.lock().await;
                            // Kill existing session with same name to prevent resource leaks
                            if let Some(mut old) = sessions.remove(&name) {
                                old.backend.kill();
                                let _ = state.output_tx.send(OutputEvent::SessionRemoved {
                                    session: name.clone(),
                                });
                            }
                            sessions.insert(name.clone(), session);

                            let rx = sessions
                                .get_mut(&name)
                                .and_then(|s| s.backend.take_reader());
                            let shared_name = sessions.get(&name).map(|s| s.shared_name.clone());
                            drop(sessions);

                            // Restore saved scrollback into the ring buffer
                            if let Some(scrollback) = super::bundle::load_scrollback(&bundle_path) {
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
                                );
                            }

                            Some(DaemonResponse::Opened {
                                id,
                                name: session_name,
                                path: bundle_path.to_string_lossy().to_string(),
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
                s.backend.kill();
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
    }
}

/// Spawn a task that relays output from a session's reader channel to the broadcast.
fn spawn_output_relay(
    output_tx: tokio::sync::broadcast::Sender<OutputEvent>,
    state_ref: Arc<DaemonState>,
    shared_name: Option<std::sync::Arc<std::sync::Mutex<String>>>,
    reader_name: String,
    rx: &mut tokio::sync::mpsc::Receiver<String>,
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

            {
                let mut sessions = state_ref.sessions.lock().await;
                if let Some(s) = sessions.get_mut(&current_name) {
                    s.buffer.push(data.clone());
                }
            }
            let _ = output_tx.send(OutputEvent::Output {
                session: current_name,
                data,
            });
        }

        let current_name = shared_name
            .as_ref()
            .map(|sn| sn.lock().unwrap().clone())
            .unwrap_or_else(|| reader_name.clone());

        let code = {
            let mut sessions = state_ref.sessions.lock().await;
            if let Some(s) = sessions.get_mut(&current_name) {
                let code = s.backend.try_exit_code().unwrap_or(0);
                s.mark_exited(code);
                Some(code)
            } else {
                None
            }
        };
        if let Some(code) = code {
            let _ = output_tx.send(OutputEvent::Exit {
                session: current_name,
                code,
            });
        }
    });
}

/// Create a PTY session and spawn its output reader task.
async fn handle_create_session(
    state: &Arc<DaemonState>,
    id: String,
    name: String,
    cols: u16,
    rows: u16,
    env: HashMap<String, String>,
) -> Option<DaemonResponse> {
    // Auto-create bundle home directory
    let home_bind = match super::bundle::ensure_bundle_home(&state.data_dir, &name) {
        Ok(path) => path,
        Err(e) => {
            return Some(DaemonResponse::Error {
                id,
                error: format!("failed to create bundle home: {}", e),
            })
        }
    };

    let backend = state
        .create_backend_with_env(&name, cols, rows, &env, &home_bind)
        .await;

    match backend {
        Ok(backend) => {
            // Compute bundle path from home_bind (parent of home/ is the .abot dir)
            let bundle_path = home_bind.parent().map(|p| p.to_path_buf());

            // Write initial manifest
            if let Some(ref bp) = bundle_path {
                let _ = super::bundle::save_bundle(bp, &name, &env).await;
            }

            let session = Session::new(name.clone(), backend, env, bundle_path.clone());
            let session_name = session.name.clone();

            let output_tx = state.output_tx.clone();
            let reader_name = session_name.clone();
            let state_ref = state.clone();

            let mut sessions = state.sessions.lock().await;
            sessions.insert(name.clone(), session);

            let rx = sessions
                .get_mut(&name)
                .and_then(|s| s.backend.take_reader());
            let shared_name = sessions.get(&name).map(|s| s.shared_name.clone());
            drop(sessions);

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
                spawn_output_relay(output_tx, state_ref, shared_name, reader_name, &mut rx);
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
