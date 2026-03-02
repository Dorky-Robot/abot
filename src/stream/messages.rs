use serde::{Deserialize, Serialize};

/// Messages from browser client to server (flat protocol)
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
    #[serde(rename = "attach")]
    Attach {
        session: String,
        #[serde(default = "default_cols")]
        cols: u16,
        #[serde(default = "default_rows")]
        rows: u16,
    },

    #[serde(rename = "input")]
    Input {
        data: String,
        #[serde(default)]
        session: Option<String>,
    },

    #[serde(rename = "resize")]
    Resize {
        #[serde(default = "default_cols")]
        cols: u16,
        #[serde(default = "default_rows")]
        rows: u16,
        #[serde(default)]
        session: Option<String>,
    },

    #[serde(rename = "detach")]
    Detach {
        #[serde(default)]
        session: Option<String>,
    },

    #[serde(rename = "p2p-signal")]
    P2pSignal { data: serde_json::Value },
}

fn default_cols() -> u16 {
    120
}
fn default_rows() -> u16 {
    40
}

/// Messages from server to browser client
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ServerMessage {
    #[serde(rename = "attached")]
    Attached { session: String, buffer: String },

    #[serde(rename = "output")]
    Output {
        data: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        session: Option<String>,
    },

    #[serde(rename = "exit")]
    Exit {
        code: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        session: Option<String>,
    },

    #[serde(rename = "session-removed")]
    SessionRemoved { session: String },

    #[serde(rename = "p2p-signal")]
    P2pSignal { data: serde_json::Value },

    #[serde(rename = "p2p-ready")]
    P2pReady,

    #[serde(rename = "p2p-closed")]
    P2pClosed,

    #[serde(rename = "p2p.unavailable")]
    P2pUnavailable,

    #[serde(rename = "server-draining")]
    ServerDraining,

    #[serde(rename = "error")]
    Error { message: String },
}
