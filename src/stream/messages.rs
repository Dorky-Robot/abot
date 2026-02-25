use serde::{Deserialize, Serialize};

/// Messages from browser client to server
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
    #[serde(rename = "session.create")]
    SessionCreate {
        kind: String,
        #[serde(default)]
        config: serde_json::Value,
    },

    #[serde(rename = "session.attach")]
    SessionAttach {
        id: String,
        #[serde(default)]
        viewport: Option<Viewport>,
    },

    #[serde(rename = "session.input")]
    SessionInput { id: String, data: String },

    #[serde(rename = "session.resize")]
    SessionResize { id: String, cols: u16, rows: u16 },

    #[serde(rename = "session.detach")]
    SessionDetach { id: String },

    #[serde(rename = "session.destroy")]
    SessionDestroy { id: String },

    #[serde(rename = "session.list")]
    SessionList,

    #[serde(rename = "p2p.signal")]
    P2pSignal { data: serde_json::Value },
}

#[derive(Debug, Deserialize)]
pub struct Viewport {
    pub cols: u16,
    pub rows: u16,
}

/// Messages from server to browser client
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ServerMessage {
    #[serde(rename = "session.created")]
    SessionCreated { id: String, kind: String },

    #[serde(rename = "session.attached")]
    SessionAttached { id: String, buffer: String },

    #[serde(rename = "session.output")]
    SessionOutput { id: String, data: String },

    #[serde(rename = "session.exit")]
    SessionExit { id: String, code: u32 },

    #[serde(rename = "session.removed")]
    SessionRemoved { id: String },

    #[serde(rename = "session.list")]
    SessionList {
        sessions: Vec<serde_json::Value>,
    },

    #[serde(rename = "p2p.signal")]
    P2pSignal { data: serde_json::Value },

    #[serde(rename = "p2p.ready")]
    P2pReady,

    #[serde(rename = "p2p.closed")]
    P2pClosed,

    #[serde(rename = "p2p.unavailable")]
    P2pUnavailable,

    #[serde(rename = "server.draining")]
    ServerDraining,

    #[serde(rename = "error")]
    Error { message: String },
}
