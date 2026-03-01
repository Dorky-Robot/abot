use std::collections::{HashMap, HashSet};
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
    /// Sessions this client is attached to (supports multi-facet)
    attached_sessions: HashSet<String>,
    p2p_sender: Option<Arc<RTCDataChannel>>,
    /// Credential ID from the session cookie (for revocation)
    credential_id: Option<String>,
}

impl Default for ClientTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientTracker {
    pub fn new() -> Self {
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn add(
        &self,
        client_id: String,
        tx: mpsc::Sender<ServerMessage>,
        credential_id: Option<String>,
    ) {
        let mut clients = self.clients.write().await;
        clients.insert(
            client_id,
            ClientInfo {
                tx,
                attached_sessions: HashSet::new(),
                p2p_sender: None,
                credential_id,
            },
        );
    }

    pub async fn remove(&self, client_id: &str) {
        let mut clients = self.clients.write().await;
        clients.remove(client_id);
    }

    /// Attach a client to a session (additive — supports multiple sessions)
    pub async fn attach(&self, client_id: &str, session_id: String) {
        let mut clients = self.clients.write().await;
        if let Some(info) = clients.get_mut(client_id) {
            info.attached_sessions.insert(session_id);
        }
    }

    /// Detach a client from a specific session
    pub async fn detach_session(&self, client_id: &str, session_id: &str) {
        let mut clients = self.clients.write().await;
        if let Some(info) = clients.get_mut(client_id) {
            info.attached_sessions.remove(session_id);
        }
    }

    /// Detach a client from all sessions
    pub async fn detach(&self, client_id: &str) {
        let mut clients = self.clients.write().await;
        if let Some(info) = clients.get_mut(client_id) {
            info.attached_sessions.clear();
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

    /// Check if a client is attached to a specific session
    pub async fn is_attached(&self, client_id: &str, session_id: &str) -> bool {
        let clients = self.clients.read().await;
        clients
            .get(client_id)
            .map(|info| info.attached_sessions.contains(session_id))
            .unwrap_or(false)
    }

    /// Send a message to a specific client, preferring DataChannel when available
    pub async fn send_to_prefer_p2p(&self, client_id: &str, msg: ServerMessage) {
        let clients = self.clients.read().await;
        if let Some(info) = clients.get(client_id) {
            if let Some(ref dc) = info.p2p_sender {
                let json = serde_json::to_string(&msg).unwrap_or_default();
                if dc.send_text(&json).await.is_ok() {
                    return;
                }
            }
            let _ = info.tx.send(msg).await;
        }
    }

    /// Close all WebSocket connections for a given credential ID.
    /// Drops the sender channels which causes the WS send loop to terminate.
    /// Returns the IDs of removed clients.
    pub async fn close_by_credential(&self, credential_id: &str) -> Vec<String> {
        let mut clients = self.clients.write().await;
        let to_remove: Vec<String> = clients
            .iter()
            .filter(|(_, info)| info.credential_id.as_deref() == Some(credential_id))
            .map(|(id, _)| id.clone())
            .collect();
        for id in &to_remove {
            clients.remove(id);
        }
        to_remove
    }
}
