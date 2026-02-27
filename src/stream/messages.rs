use serde::{Deserialize, Serialize};

/// Messages from browser client to server
/// Supports both abot's namespaced protocol (session.input) and
/// flat protocol (input, attach, resize) for backwards compatibility
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
    // --- Namespaced protocol (abot native) ---

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

    // --- Flat protocol (backwards compatibility) ---

    /// Flat: { type: "input", data: "..." }
    #[serde(rename = "input")]
    FlatInput {
        data: String,
        /// Optional session name for multi-session routing
        #[serde(default)]
        session: Option<String>,
    },

    /// Flat: { type: "attach", session: "name", cols: N, rows: N }
    #[serde(rename = "attach")]
    FlatAttach {
        session: String,
        #[serde(default = "default_cols")]
        cols: u16,
        #[serde(default = "default_rows")]
        rows: u16,
    },

    /// Flat: { type: "resize", cols: N, rows: N }
    #[serde(rename = "resize")]
    FlatResize {
        #[serde(default = "default_cols")]
        cols: u16,
        #[serde(default = "default_rows")]
        rows: u16,
        /// Optional session name for multi-session routing
        #[serde(default)]
        session: Option<String>,
    },

    /// Detach from a specific session (facet close)
    #[serde(rename = "detach")]
    FlatDetach {
        #[serde(default)]
        session: Option<String>,
    },

    /// P2P signaling (flat protocol)
    #[serde(rename = "p2p-signal")]
    FlatP2pSignal { data: serde_json::Value },
}

fn default_cols() -> u16 {
    120
}
fn default_rows() -> u16 {
    40
}

#[derive(Debug, Deserialize)]
pub struct Viewport {
    pub cols: u16,
    pub rows: u16,
}

/// Messages from server to browser client
/// Supports both namespaced (session.output) and flat (output) variants
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ServerMessage {
    // --- Namespaced protocol (abot native) ---

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
    SessionListReply {
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

    #[serde(rename = "server-draining")]
    ServerDraining,

    #[serde(rename = "error")]
    Error { message: String },

    // --- Flat protocol (backwards compatibility) ---

    #[serde(rename = "attached")]
    FlatAttached {
        session: String,
        buffer: String,
    },

    #[serde(rename = "output")]
    FlatOutput {
        data: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        session: Option<String>,
    },

    #[serde(rename = "exit")]
    FlatExit { code: u32 },

    #[serde(rename = "session-removed")]
    FlatSessionRemoved { session: String },

    #[serde(rename = "p2p-signal")]
    FlatP2pSignal { data: serde_json::Value },

    #[serde(rename = "p2p-ready")]
    FlatP2pReady,

    #[serde(rename = "p2p-closed")]
    FlatP2pClosed,
}
