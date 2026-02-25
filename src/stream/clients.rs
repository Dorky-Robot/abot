use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use webrtc::data_channel::RTCDataChannel;

use super::messages::ServerMessage;

/// Track connected browser clients and their session attachments
#[derive(Clone)]
pub struct ClientTracker {
    clients: Arc<RwLock<HashMap<String, ClientInfo>>>,
}

struct ClientInfo {
    tx: mpsc::Sender<ServerMessage>,
    attached_session: Option<String>,
    p2p_sender: Option<Arc<RTCDataChannel>>,
}

impl ClientTracker {
    pub fn new() -> Self {
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn add(&self, client_id: String, tx: mpsc::Sender<ServerMessage>) {
        let mut clients = self.clients.write().await;
        clients.insert(
            client_id,
            ClientInfo {
                tx,
                attached_session: None,
                p2p_sender: None,
            },
        );
    }

    pub async fn remove(&self, client_id: &str) {
        let mut clients = self.clients.write().await;
        clients.remove(client_id);
    }

    pub async fn attach(&self, client_id: &str, session_id: String) {
        let mut clients = self.clients.write().await;
        if let Some(info) = clients.get_mut(client_id) {
            info.attached_session = Some(session_id);
        }
    }

    pub async fn detach(&self, client_id: &str) {
        let mut clients = self.clients.write().await;
        if let Some(info) = clients.get_mut(client_id) {
            info.attached_session = None;
        }
    }

    /// Send a message to all clients attached to a specific session
    pub async fn broadcast_to_session(&self, session_id: &str, msg: ServerMessage) {
        let clients = self.clients.read().await;
        for info in clients.values() {
            if info.attached_session.as_deref() == Some(session_id) {
                let _ = info.tx.send(msg.clone()).await;
            }
        }
    }

    /// Send a message to all connected clients
    pub async fn broadcast_all(&self, msg: ServerMessage) {
        let clients = self.clients.read().await;
        for info in clients.values() {
            let _ = info.tx.send(msg.clone()).await;
        }
    }

    /// Send a message to a specific client
    pub async fn send_to(&self, client_id: &str, msg: ServerMessage) {
        let clients = self.clients.read().await;
        if let Some(info) = clients.get(client_id) {
            let _ = info.tx.send(msg).await;
        }
    }

    /// Store a DataChannel sender for a client (P2P connected)
    pub async fn set_p2p_sender(&self, client_id: &str, sender: Arc<RTCDataChannel>) {
        let mut clients = self.clients.write().await;
        if let Some(info) = clients.get_mut(client_id) {
            info.p2p_sender = Some(sender);
        }
    }

    /// Clear the DataChannel sender for a client (P2P disconnected)
    pub async fn clear_p2p_sender(&self, client_id: &str) {
        let mut clients = self.clients.write().await;
        if let Some(info) = clients.get_mut(client_id) {
            info.p2p_sender = None;
        }
    }

    /// Send output to all clients on a session, preferring DataChannel when available
    pub async fn broadcast_to_session_prefer_p2p(&self, session_id: &str, msg: ServerMessage) {
        let json = serde_json::to_string(&msg).unwrap_or_default();
        let clients = self.clients.read().await;
        for info in clients.values() {
            if info.attached_session.as_deref() == Some(session_id) {
                // Try DataChannel first
                if let Some(ref dc) = info.p2p_sender {
                    if dc.send_text(&json).await.is_ok() {
                        continue;
                    }
                }
                // Fall back to WebSocket
                let _ = info.tx.send(msg.clone()).await;
            }
        }
    }

    /// Send a message to all OTHER clients attached to the same session as the sender
    pub async fn relay_to_session_peers(
        &self,
        sender_id: &str,
        msg: ServerMessage,
    ) {
        let clients = self.clients.read().await;
        let sender_session = clients
            .get(sender_id)
            .and_then(|info| info.attached_session.clone());

        if let Some(session_id) = sender_session {
            for (id, info) in clients.iter() {
                if id != sender_id && info.attached_session.as_deref() == Some(&session_id) {
                    let _ = info.tx.send(msg.clone()).await;
                }
            }
        }
    }
}
