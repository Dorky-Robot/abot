use axum::extract::ws::{Message, WebSocket};
use axum::extract::{ConnectInfo, State, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

use super::messages::{ClientMessage, ServerMessage};
use super::p2p::{P2pEvent, ServerPeer};
use crate::auth::middleware;
use crate::auth::state;
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

    // Auth check
    let authenticated = if is_local {
        true
    } else {
        let cookie = headers.get("cookie").and_then(|v| v.to_str().ok());
        if let Some(token) = middleware::get_session_token(cookie) {
            let db = app.auth.db.lock().unwrap();
            state::validate_session(&db, &token).unwrap_or(false)
        } else {
            false
        }
    };

    if !authenticated {
        return axum::http::StatusCode::UNAUTHORIZED.into_response();
    }

    // Origin check for non-localhost — reject missing Origin to prevent CSWSH
    if !is_local {
        let origin = match origin {
            Some(o) => o,
            None => return axum::http::StatusCode::FORBIDDEN.into_response(),
        };
        let allowed = host.map(|h| {
            let expected = format!("https://{}", h);
            let expected_http = format!("http://{}", h);
            origin == expected || origin == expected_http
        }).unwrap_or(false);

        if !allowed {
            return axum::http::StatusCode::FORBIDDEN.into_response();
        }
    }

    ws.on_upgrade(move |socket| handle_socket(socket, app))
}

/// Per-client P2P peer state, shared between message handler and event task
type PeerMap = Arc<Mutex<HashMap<String, PeerState>>>;

struct PeerState {
    peer: ServerPeer,
    event_tx: mpsc::Sender<P2pEvent>,
}

async fn handle_socket(socket: WebSocket, app: Arc<AppState>) {
    let client_id = uuid::Uuid::new_v4().to_string();
    let (mut ws_sink, mut ws_stream) = socket.split();

    // Channel for sending messages to this client
    let (tx, mut rx) = mpsc::channel::<ServerMessage>(256);
    app.stream_clients.add(client_id.clone(), tx).await;

    // P2P peers for this client connection
    let peers: PeerMap = Arc::new(Mutex::new(HashMap::new()));

    // Subscribe to daemon broadcast events
    let mut daemon_rx = app.daemon_client.subscribe();
    let clients = app.stream_clients.clone();

    // Task: relay daemon events → client (prefer DataChannel for output)
    let relay_handle = tokio::spawn(async move {
        loop {
            match daemon_rx.recv().await {
                Ok(msg) => {
                    if let Some(msg_type) = msg.get("type").and_then(|v| v.as_str()) {
                        match msg_type {
                            "output" => {
                                if let (Some(session), Some(data)) = (
                                    msg.get("session").and_then(|v| v.as_str()),
                                    msg.get("data").and_then(|v| v.as_str()),
                                ) {
                                    clients
                                        .broadcast_to_session_prefer_p2p(
                                            session,
                                            ServerMessage::SessionOutput {
                                                id: session.to_string(),
                                                data: data.to_string(),
                                            },
                                        )
                                        .await;
                                }
                            }
                            "exit" => {
                                if let (Some(session), Some(code)) = (
                                    msg.get("session").and_then(|v| v.as_str()),
                                    msg.get("code").and_then(|v| v.as_u64()),
                                ) {
                                    clients
                                        .broadcast_to_session(
                                            session,
                                            ServerMessage::SessionExit {
                                                id: session.to_string(),
                                                code: code as u32,
                                            },
                                        )
                                        .await;
                                }
                            }
                            "session-removed" => {
                                if let Some(session) =
                                    msg.get("session").and_then(|v| v.as_str())
                                {
                                    clients
                                        .broadcast_all(ServerMessage::SessionRemoved {
                                            id: session.to_string(),
                                        })
                                        .await;
                                }
                            }
                            _ => {}
                        }
                    }
                }
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
                if let Err(e) =
                    handle_client_message(&app, &client_id, &text, &peers).await
                {
                    tracing::warn!("client message error: {}", e);
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
    tracing::info!("client {} message: {:?}", client_id, msg);

    match msg {
        ClientMessage::SessionCreate { kind, config } => {
            let name = config
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("main")
                .to_string();

            let resp = app
                .daemon_client
                .rpc(json!({
                    "type": "create-session",
                    "name": name,
                    "cols": 120,
                    "rows": 40,
                }))
                .await?;

            if resp.get("error").is_some() {
                app.stream_clients
                    .send_to(
                        client_id,
                        ServerMessage::Error {
                            message: resp["error"].as_str().unwrap_or("unknown").to_string(),
                        },
                    )
                    .await;
            } else {
                app.stream_clients
                    .send_to(
                        client_id,
                        ServerMessage::SessionCreated {
                            id: name,
                            kind,
                        },
                    )
                    .await;
            }
        }

        ClientMessage::SessionAttach { id, viewport } => {
            let (cols, rows) = viewport
                .map(|v| (v.cols, v.rows))
                .unwrap_or((120, 40));

            let resp = app
                .daemon_client
                .rpc(json!({
                    "type": "attach",
                    "clientId": client_id,
                    "session": id,
                    "cols": cols,
                    "rows": rows,
                }))
                .await?;

            if let Some(error) = resp.get("error").and_then(|v| v.as_str()) {
                app.stream_clients
                    .send_to(
                        client_id,
                        ServerMessage::Error {
                            message: error.to_string(),
                        },
                    )
                    .await;
            } else {
                let buffer = resp
                    .get("buffer")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                app.stream_clients.attach(client_id, id.clone()).await;
                app.stream_clients
                    .send_to(
                        client_id,
                        ServerMessage::SessionAttached { id, buffer },
                    )
                    .await;
            }
        }

        ClientMessage::SessionInput { id: _, data } => {
            app.daemon_client
                .send(&json!({
                    "type": "input",
                    "clientId": client_id,
                    "data": data,
                }))
                .await?;
        }

        ClientMessage::SessionResize { id: _, cols, rows } => {
            app.daemon_client
                .send(&json!({
                    "type": "resize",
                    "clientId": client_id,
                    "cols": cols,
                    "rows": rows,
                }))
                .await?;
        }

        ClientMessage::SessionDetach { id: _ } => {
            app.stream_clients.detach(client_id).await;
            app.daemon_client
                .send(&json!({
                    "type": "detach",
                    "clientId": client_id,
                }))
                .await?;
        }

        ClientMessage::SessionDestroy { id } => {
            app.daemon_client
                .rpc(json!({
                    "type": "delete-session",
                    "name": id,
                }))
                .await?;
        }

        ClientMessage::SessionList => {
            let resp = app
                .daemon_client
                .rpc(json!({
                    "type": "list-sessions",
                }))
                .await?;

            let sessions = resp
                .get("sessions")
                .cloned()
                .unwrap_or(serde_json::json!([]));

            app.stream_clients
                .send_to(
                    client_id,
                    ServerMessage::SessionList {
                        sessions: if let serde_json::Value::Array(arr) = sessions {
                            arr
                        } else {
                            vec![]
                        },
                    },
                )
                .await;
        }

        ClientMessage::P2pSignal { data } => {
            let signal_type = data.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let is_offer = signal_type == "offer";

            // On new offer, destroy old peer and create fresh one
            if is_offer {
                let mut map = peers.lock().await;
                if let Some(old) = map.remove(client_id) {
                    old.peer.destroy().await;
                    app.stream_clients.clear_p2p_sender(client_id).await;
                }

                match ServerPeer::new().await {
                    Ok((peer, mut event_rx)) => {
                        // Create a tx for handle_signal to send events through
                        let (event_tx, mut event_forward_rx) = mpsc::channel::<P2pEvent>(64);

                        // Forward events from the peer's built-in channel and handle_signal's tx
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
                                            .send_to(
                                                &cid,
                                                ServerMessage::P2pSignal { data: signal_data },
                                            )
                                            .await;
                                    }
                                    P2pEvent::Ready(dc) => {
                                        tracing::info!("P2P DataChannel ready for client {}", cid);
                                        clients.set_p2p_sender(&cid, dc).await;
                                        clients.send_to(&cid, ServerMessage::P2pReady).await;
                                    }
                                    P2pEvent::Data(text) => {
                                        // Parse JSON input from DataChannel and forward to daemon
                                        if let Ok(parsed) =
                                            serde_json::from_str::<serde_json::Value>(&text)
                                        {
                                            let msg_type = parsed
                                                .get("type")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");
                                            match msg_type {
                                                "session.input" => {
                                                    if let Some(input_data) = parsed
                                                        .get("data")
                                                        .and_then(|v| v.as_str())
                                                    {
                                                        let _ = app_clone
                                                            .daemon_client
                                                            .send(&json!({
                                                                "type": "input",
                                                                "clientId": cid,
                                                                "data": input_data,
                                                            }))
                                                            .await;
                                                    }
                                                }
                                                "session.resize" => {
                                                    let cols = parsed
                                                        .get("cols")
                                                        .and_then(|v| v.as_u64())
                                                        .unwrap_or(120)
                                                        as u16;
                                                    let rows = parsed
                                                        .get("rows")
                                                        .and_then(|v| v.as_u64())
                                                        .unwrap_or(40)
                                                        as u16;
                                                    let _ = app_clone
                                                        .daemon_client
                                                        .send(&json!({
                                                            "type": "resize",
                                                            "clientId": cid,
                                                            "cols": cols,
                                                            "rows": rows,
                                                        }))
                                                        .await;
                                                }
                                                _ => {
                                                    tracing::debug!(
                                                        "unknown DC message type: {}",
                                                        msg_type
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    P2pEvent::Closed => {
                                        tracing::info!(
                                            "P2P DataChannel closed for client {}",
                                            cid
                                        );
                                        clients.clear_p2p_sender(&cid).await;
                                        clients
                                            .send_to(&cid, ServerMessage::P2pClosed)
                                            .await;
                                        break;
                                    }
                                }
                            }
                        });

                        // Feed the offer signal
                        if let Err(e) = peer.handle_signal(&data, &event_tx).await {
                            tracing::warn!("P2P signal error: {}", e);
                        }

                        map.insert(
                            client_id.to_string(),
                            PeerState {
                                peer,
                                event_tx,
                            },
                        );
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
                    if let Err(e) =
                        peer_state.peer.handle_signal(&data, &peer_state.event_tx).await
                    {
                        tracing::warn!("P2P signal error: {}", e);
                    }
                }
            }
        }
    }

    Ok(())
}
