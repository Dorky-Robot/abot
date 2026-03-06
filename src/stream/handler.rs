use axum::extract::ws::{Message, WebSocket};
use axum::extract::{ConnectInfo, State, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

use super::messages::{ClientMessage, ServerMessage};
use super::p2p::{P2pEvent, ServerPeer};
use crate::auth::{middleware, state as auth_state};
use crate::server::AppState;

/// WebSocket upgrade handler with auth + origin check
pub async fn ws_upgrade(
    ws: WebSocketUpgrade,
    State(app): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let host = headers.get("host").and_then(|v| v.to_str().ok());
    let origin = headers.get("origin").and_then(|v| v.to_str().ok());
    let is_local = middleware::is_local_request(&addr, host, origin);

    // Auth check — delegate to centralized middleware helper
    if let Err(e) = middleware::require_auth(&app, &addr, &headers) {
        return e.into_response();
    }

    // Origin check for non-localhost — reject missing Origin to prevent CSWSH
    if !is_local {
        let origin = match origin {
            Some(o) => o,
            None => return axum::http::StatusCode::FORBIDDEN.into_response(),
        };
        let allowed = host
            .map(|h| {
                let host_without_port = h.split(':').next().unwrap_or(h);
                let expected = format!("https://{}", host_without_port);
                let expected_http = format!("http://{}", host_without_port);
                origin == expected || origin == expected_http
            })
            .unwrap_or(false);

        if !allowed {
            return axum::http::StatusCode::FORBIDDEN.into_response();
        }
    }

    // Extract credential_id from session cookie for revocation tracking
    let credential_id = {
        let cookie = headers.get("cookie").and_then(|v| v.to_str().ok());
        if let Some(token) = middleware::get_session_token(cookie) {
            let db = app.auth.db.lock().ok();
            db.and_then(|db| {
                auth_state::get_auth_grant_credential_id(&db, &token)
                    .ok()
                    .flatten()
            })
        } else {
            None
        }
    };

    ws.on_upgrade(move |socket| handle_socket(socket, app, credential_id))
}

/// Per-client P2P peer state, shared between message handler and event task
type PeerMap = Arc<Mutex<HashMap<String, PeerState>>>;

struct PeerState {
    peer: ServerPeer,
    event_tx: mpsc::Sender<P2pEvent>,
}

async fn handle_socket(socket: WebSocket, app: Arc<AppState>, credential_id: Option<String>) {
    let client_id = uuid::Uuid::new_v4().to_string();
    let (mut ws_sink, mut ws_stream) = socket.split();

    // Channel for sending messages to this client
    let (tx, mut rx) = mpsc::channel::<ServerMessage>(256);
    app.stream_clients
        .add(client_id.clone(), tx, credential_id)
        .await;

    // P2P peers for this client connection
    let peers: PeerMap = Arc::new(Mutex::new(HashMap::new()));

    // Subscribe to engine broadcast events
    let mut engine_rx = app.engine.subscribe();
    let clients = app.stream_clients.clone();

    // Task: relay engine events → this client only
    let relay_client_id = client_id.clone();
    let relay_handle = tokio::spawn(async move {
        loop {
            match engine_rx.recv().await {
                Ok(event) => match event {
                    crate::engine::OutputEvent::Output { session, data } => {
                        if !clients.is_attached(&relay_client_id, &session).await {
                            continue;
                        }
                        clients
                            .send_to_prefer_p2p(
                                &relay_client_id,
                                ServerMessage::Output {
                                    data,
                                    session: Some(session),
                                },
                            )
                            .await;
                    }
                    crate::engine::OutputEvent::Exit { session, code } => {
                        if !clients.is_attached(&relay_client_id, &session).await {
                            continue;
                        }
                        clients
                            .send_to(
                                &relay_client_id,
                                ServerMessage::Exit {
                                    code,
                                    session: Some(session),
                                },
                            )
                            .await;
                    }
                    crate::engine::OutputEvent::SessionRemoved { session } => {
                        clients
                            .send_to(&relay_client_id, ServerMessage::SessionRemoved { session })
                            .await;
                    }
                },
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Task: send queued messages → WebSocket
    let send_handle = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Ok(json) = serde_json::to_string(&msg) {
                if ws_sink.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        }
    });

    // Read messages from WebSocket
    while let Some(Ok(msg)) = ws_stream.next().await {
        match msg {
            Message::Text(text) => {
                if let Err(e) = handle_client_message(&app, &client_id, &text, &peers).await {
                    tracing::warn!("client {} message error: {}", client_id, e);
                    app.stream_clients
                        .send_to(
                            &client_id,
                            ServerMessage::Error {
                                message: e.to_string(),
                            },
                        )
                        .await;
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    // Cleanup: destroy all P2P peers for this client
    {
        let mut map = peers.lock().await;
        for (_id, peer_state) in map.drain() {
            peer_state.peer.destroy().await;
        }
    }
    app.stream_clients.remove(&client_id).await;
    relay_handle.abort();
    send_handle.abort();

    tracing::info!("client {} disconnected", client_id);
}

async fn handle_client_message(
    app: &Arc<AppState>,
    client_id: &str,
    text: &str,
    peers: &PeerMap,
) -> anyhow::Result<()> {
    let msg: ClientMessage = serde_json::from_str(text)?;
    tracing::debug!("client {} message: {:?}", client_id, msg);

    match msg {
        ClientMessage::Attach {
            session,
            cols,
            rows,
        } => handle_attach(app, client_id, session, cols, rows).await?,

        ClientMessage::Input { data, session } => {
            if let Some(session_name) = session {
                if let Err(e) = app.engine.write_input(&session_name, &data).await {
                    tracing::warn!("write to session '{}' failed: {}", session_name, e);
                }
            } else {
                tracing::warn!("no session specified for input from client '{}'", client_id);
            }
        }

        ClientMessage::Resize {
            cols,
            rows,
            session,
        } => {
            if let Some(session_name) = session {
                app.engine.resize(&session_name, cols, rows).await;
            }
        }

        ClientMessage::Detach { session } => {
            if let Some(session_name) = &session {
                app.stream_clients
                    .detach_session(client_id, session_name)
                    .await;
            } else {
                app.stream_clients.detach(client_id).await;
            }
            // Attachment state is tracked only in ClientTracker
        }

        ClientMessage::P2pSignal { data } => {
            handle_p2p_signal(app, client_id, data, peers).await;
        }
    }

    Ok(())
}

/// Attach the client to an existing session.
async fn handle_attach(
    app: &Arc<AppState>,
    client_id: &str,
    session: String,
    cols: u16,
    rows: u16,
) -> anyhow::Result<()> {
    match app.engine.get_session_buffer(&session).await {
        Ok(buffer) => {
            // Resize the session to match the client's terminal dimensions
            app.engine.resize(&session, cols, rows).await;
            app.stream_clients.attach(client_id, session.clone()).await;
            app.stream_clients
                .send_to(client_id, ServerMessage::Attached { session, buffer })
                .await;
        }
        Err(e) => {
            app.stream_clients
                .send_to(
                    client_id,
                    ServerMessage::Error {
                        message: e.to_string(),
                    },
                )
                .await;
        }
    }

    Ok(())
}

/// Handle WebRTC P2P signaling (offer/answer/ICE candidates).
async fn handle_p2p_signal(
    app: &Arc<AppState>,
    client_id: &str,
    data: serde_json::Value,
    peers: &PeerMap,
) {
    let signal_type = data.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let is_offer = signal_type == "offer";

    if is_offer {
        let mut map = peers.lock().await;
        if let Some(old) = map.remove(client_id) {
            old.peer.destroy().await;
            app.stream_clients.clear_p2p_sender(client_id).await;
        }

        match ServerPeer::new().await {
            Ok((peer, mut event_rx)) => {
                let (event_tx, mut event_forward_rx) = mpsc::channel::<P2pEvent>(64);

                let clients = app.stream_clients.clone();
                let app_clone = app.clone();
                let cid = client_id.to_string();
                tokio::spawn(async move {
                    loop {
                        let event = tokio::select! {
                            Some(e) = event_rx.recv() => e,
                            Some(e) = event_forward_rx.recv() => e,
                            else => break,
                        };

                        match event {
                            P2pEvent::Signal(signal_data) => {
                                clients
                                    .send_to(&cid, ServerMessage::P2pSignal { data: signal_data })
                                    .await;
                            }
                            P2pEvent::Ready(dc) => {
                                tracing::info!("P2P DataChannel ready for client {}", cid);
                                clients.set_p2p_sender(&cid, dc).await;
                                clients.send_to(&cid, ServerMessage::P2pReady).await;
                            }
                            P2pEvent::Data(text) => {
                                handle_p2p_data(&app_clone, &text).await;
                            }
                            P2pEvent::Closed => {
                                tracing::info!("P2P DataChannel closed for client {}", cid);
                                clients.clear_p2p_sender(&cid).await;
                                clients.send_to(&cid, ServerMessage::P2pClosed).await;
                                break;
                            }
                        }
                    }
                });

                if let Err(e) = peer.handle_signal(&data, &event_tx).await {
                    tracing::warn!("P2P signal error: {}", e);
                }

                map.insert(client_id.to_string(), PeerState { peer, event_tx });
            }
            Err(e) => {
                tracing::warn!("Failed to create WebRTC peer: {}", e);
                app.stream_clients
                    .send_to(client_id, ServerMessage::P2pUnavailable)
                    .await;
            }
        }
    } else {
        // ICE candidate — feed to existing peer
        let map = peers.lock().await;
        if let Some(peer_state) = map.get(client_id) {
            if let Err(e) = peer_state
                .peer
                .handle_signal(&data, &peer_state.event_tx)
                .await
            {
                tracing::warn!("P2P signal error: {}", e);
            }
        }
    }
}

/// Handle a message received over the P2P DataChannel.
async fn handle_p2p_data(app: &Arc<AppState>, text: &str) {
    let parsed = match serde_json::from_str::<serde_json::Value>(text) {
        Ok(v) => v,
        Err(_) => return,
    };

    let msg_type = parsed.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match msg_type {
        "input" => {
            if let Some(input_data) = parsed.get("data").and_then(|v| v.as_str()) {
                let session = parsed.get("session").and_then(|v| v.as_str());
                if let Some(session_name) = session {
                    let _ = app.engine.write_input(session_name, input_data).await;
                }
            }
        }
        "resize" => {
            let cols = parsed.get("cols").and_then(|v| v.as_u64()).unwrap_or(120) as u16;
            let rows = parsed.get("rows").and_then(|v| v.as_u64()).unwrap_or(40) as u16;
            let session = parsed.get("session").and_then(|v| v.as_str());
            if let Some(session_name) = session {
                app.engine.resize(session_name, cols, rows).await;
            }
        }
        _ => {
            tracing::debug!("unknown DC message type: {}", msg_type);
        }
    }
}
