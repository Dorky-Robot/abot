use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::session::Session;
use super::DaemonState;

/// Messages from server to daemon
#[derive(Debug, Deserialize)]
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

    /// Fire-and-forget: detach a client
    #[serde(rename = "detach")]
    Detach {
        #[serde(rename = "clientId")]
        client_id: String,
    },
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

pub async fn handle_request(state: &Arc<DaemonState>, req: DaemonRequest) -> Option<DaemonResponse> {
    match req {
        DaemonRequest::ListSessions { id } => {
            let sessions = state.sessions.lock().await;
            let list: Vec<serde_json::Value> = sessions.values().map(|s| s.to_json()).collect();
            Some(DaemonResponse::SessionList { id, sessions: list })
        }

        DaemonRequest::CreateSession {
            id,
            name,
            cols,
            rows,
        } => {
            let backend = state.create_backend(&name, cols, rows).await;

            match backend {
                Ok(backend) => {
                    let session = Session::new(name.clone(), backend);
                    let session_name = session.name.clone();

                    // Spawn output reader task
                    let output_tx = state.output_tx.clone();
                    let reader_name = session_name.clone();
                    let state_ref = state.clone();

                    let mut sessions = state.sessions.lock().await;
                    sessions.insert(name.clone(), session);

                    // Extract backend reader channel, then drop the lock
                    let rx = sessions
                        .get_mut(&name)
                        .and_then(|s| s.backend.take_reader());
                    drop(sessions);

                    if let Some(mut rx) = rx {
                        let buffer_name = name.clone();
                        tokio::spawn(async move {
                            while let Some(data) = rx.recv().await {
                                // Write to ring buffer so attach can replay history
                                {
                                    let mut sessions = state_ref.sessions.lock().await;
                                    if let Some(s) = sessions.get_mut(&buffer_name) {
                                        s.buffer.push(data.clone());
                                    }
                                }
                                let _ = output_tx.send(OutputEvent::Output {
                                    session: reader_name.clone(),
                                    data,
                                });
                            }
                        });
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

        DaemonRequest::Attach {
            id,
            client_id,
            session,
            cols: _,
            rows: _,
        } => {
            // Record the client→session mapping
            {
                let mut attachments = state.client_attachments.lock().await;
                attachments.insert(client_id, session.clone());
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
            let mut sessions = state.sessions.lock().await;
            if let Some(mut session) = sessions.remove(&name) {
                session.backend.kill();
                let _ = state.output_tx.send(OutputEvent::SessionRemoved {
                    session: name.clone(),
                });
                Some(DaemonResponse::Deleted { id, name })
            } else {
                Some(DaemonResponse::Error {
                    id,
                    error: format!("session '{}' not found", name),
                })
            }
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
                sessions.insert(new_name.clone(), session);

                // Update client attachments that point to old name
                drop(sessions);
                let mut attachments = state.client_attachments.lock().await;
                for (_client_id, attached_session) in attachments.iter_mut() {
                    if attached_session == &old_name {
                        *attached_session = new_name.clone();
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
        DaemonRequest::Input { client_id, session, data } => {
            // Use explicit session name if provided, otherwise fall back to attachment
            let session_name = if let Some(s) = session {
                Some(s)
            } else {
                let attachments = state.client_attachments.lock().await;
                attachments.get(&client_id).cloned()
            };

            if let Some(session_name) = session_name {
                let mut sessions = state.sessions.lock().await;
                if let Some(session) = sessions.get_mut(&session_name) {
                    if session.alive {
                        match session.write(data.as_bytes()) {
                            Ok(_) => {
                                tracing::debug!("daemon: wrote {} bytes to session '{}'", data.len(), session.name);
                            }
                            Err(e) => {
                                tracing::error!("daemon: write to session '{}' failed: {}", session.name, e);
                            }
                        }
                    } else {
                        tracing::warn!("daemon: session '{}' is not alive", session_name);
                    }
                } else {
                    tracing::warn!("daemon: session '{}' not found for client '{}'", session_name, client_id);
                }
            } else {
                tracing::warn!("daemon: no session attached for client '{}'", client_id);
            }
            None
        }

        DaemonRequest::Resize {
            client_id,
            session,
            cols,
            rows,
        } => {
            // Use explicit session name if provided, otherwise fall back to attachment
            let session_name = if let Some(s) = session {
                Some(s)
            } else {
                let attachments = state.client_attachments.lock().await;
                attachments.get(&client_id).cloned()
            };

            if let Some(session_name) = session_name {
                let mut sessions = state.sessions.lock().await;
                if let Some(session) = sessions.get_mut(&session_name) {
                    let _ = session.resize(cols, rows);
                }
            }
            None
        }

        DaemonRequest::Detach { client_id } => {
            let mut attachments = state.client_attachments.lock().await;
            attachments.remove(&client_id);
            None
        }
    }
}
