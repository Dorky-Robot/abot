use serde::{Deserialize, Serialize};
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

/// Resolve session name from explicit parameter.
/// No fallback — clients must always specify which session they're targeting.
fn resolve_session(explicit: Option<String>) -> Option<String> {
    explicit
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
        } => handle_create_session(state, id, name, cols, rows).await,

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
                // Update the shared name so background relay tasks use the new name
                *session.shared_name.lock().unwrap() = new_name.clone();
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
            let session_name = resolve_session(session);

            if let Some(session_name) = session_name {
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
            let session_name = resolve_session(session);

            if let Some(session_name) = session_name {
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

        DaemonRequest::Ping { id } => Some(DaemonResponse::Pong { id }),
    }
}

/// Create a PTY session and spawn its output reader task.
async fn handle_create_session(
    state: &Arc<DaemonState>,
    id: String,
    name: String,
    cols: u16,
    rows: u16,
) -> Option<DaemonResponse> {
    let backend = state.create_backend(&name, cols, rows).await;

    match backend {
        Ok(backend) => {
            let session = Session::new(name.clone(), backend);
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

            if let Some(mut rx) = rx {
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
    fn test_resolve_session_some() {
        assert_eq!(
            resolve_session(Some("main".to_string())),
            Some("main".to_string())
        );
    }

    #[test]
    fn test_resolve_session_none() {
        assert_eq!(resolve_session(None), None);
    }

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
}
