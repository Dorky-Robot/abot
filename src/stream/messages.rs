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

    #[serde(rename = "p2p-unavailable")]
    P2pUnavailable,

    #[serde(rename = "server-draining")]
    ServerDraining,

    #[serde(rename = "error")]
    Error { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_attach_deserialize() {
        let json = r#"{"type":"attach","session":"main","cols":80,"rows":24}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::Attach {
                session,
                cols,
                rows,
            } => {
                assert_eq!(session, "main");
                assert_eq!(cols, 80);
                assert_eq!(rows, 24);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_client_attach_defaults() {
        let json = r#"{"type":"attach","session":"main"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::Attach { cols, rows, .. } => {
                assert_eq!(cols, 120);
                assert_eq!(rows, 40);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_client_input_with_session() {
        let json = r#"{"type":"input","data":"ls\n","session":"dev"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::Input { data, session } => {
                assert_eq!(data, "ls\n");
                assert_eq!(session, Some("dev".to_string()));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_client_input_without_session() {
        let json = r#"{"type":"input","data":"x"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::Input { session, .. } => {
                assert_eq!(session, None);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_client_detach_optional_session() {
        let json = r#"{"type":"detach"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::Detach { session } => assert_eq!(session, None),
            _ => panic!("wrong variant"),
        }

        let json = r#"{"type":"detach","session":"s1"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::Detach { session } => assert_eq!(session, Some("s1".to_string())),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_server_output_serializes() {
        let msg = ServerMessage::Output {
            data: "hello".to_string(),
            session: Some("main".to_string()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"output""#));
        assert!(json.contains(r#""session":"main""#));
    }

    #[test]
    fn test_server_output_skips_none_session() {
        let msg = ServerMessage::Output {
            data: "x".to_string(),
            session: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(!json.contains("session"));
    }

    #[test]
    fn test_server_error_serializes() {
        let msg = ServerMessage::Error {
            message: "bad".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"error""#));
        assert!(json.contains(r#""message":"bad""#));
    }

    #[test]
    fn test_server_p2p_ready_serializes() {
        let msg = ServerMessage::P2pReady;
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"p2p-ready""#));
    }

    #[test]
    fn test_unknown_type_fails_deserialization() {
        let json = r#"{"type":"nonexistent","data":"x"}"#;
        let result = serde_json::from_str::<ClientMessage>(json);
        assert!(result.is_err());
    }
}
