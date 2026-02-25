use std::sync::Arc;
use tokio::sync::mpsc;
use webrtc::api::APIBuilder;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;

/// Events emitted by a server-side WebRTC peer
pub enum P2pEvent {
    /// SDP answer or ICE candidate to send back to browser
    Signal(serde_json::Value),
    /// DataChannel connected and ready
    Ready(Arc<RTCDataChannel>),
    /// Terminal input received over DataChannel
    Data(String),
    /// DataChannel closed or errored
    Closed,
}

/// Server-side WebRTC peer for one client connection
pub struct ServerPeer {
    peer: Arc<RTCPeerConnection>,
}

impl ServerPeer {
    /// Create a new server peer (answerer role).
    /// Returns the peer and a channel that emits P2pEvents.
    pub async fn new() -> anyhow::Result<(Self, mpsc::Receiver<P2pEvent>)> {
        let api = APIBuilder::new().build();

        let config = RTCConfiguration {
            // No ICE servers needed for localhost/LAN
            ..Default::default()
        };

        let peer = Arc::new(api.new_peer_connection(config).await?);
        let (tx, rx) = mpsc::channel::<P2pEvent>(64);

        // ICE candidate callback — send candidates back to browser
        let tx_ice = tx.clone();
        peer.on_ice_candidate(Box::new(move |candidate| {
            let tx = tx_ice.clone();
            Box::pin(async move {
                if let Some(candidate) = candidate {
                    if let Ok(init) = candidate.to_json() {
                        let signal = serde_json::json!({
                            "type": "candidate",
                            "candidate": {
                                "candidate": init.candidate,
                                "sdpMid": init.sdp_mid,
                                "sdpMLineIndex": init.sdp_mline_index,
                                "usernameFragment": init.username_fragment,
                            }
                        });
                        let _ = tx.send(P2pEvent::Signal(signal)).await;
                    }
                }
            })
        }));

        // DataChannel callback — browser creates DC, we receive it here
        let tx_dc = tx.clone();
        peer.on_data_channel(Box::new(move |dc: Arc<RTCDataChannel>| {
            let tx = tx_dc.clone();
            Box::pin(async move {
                let dc_open = dc.clone();
                let tx_open = tx.clone();
                dc.on_open(Box::new(move || {
                    let dc = dc_open.clone();
                    let tx = tx_open.clone();
                    Box::pin(async move {
                        let _ = tx.send(P2pEvent::Ready(dc)).await;
                    })
                }));

                let tx_msg = tx.clone();
                dc.on_message(Box::new(move |msg| {
                    let tx = tx_msg.clone();
                    Box::pin(async move {
                        if let Ok(text) = String::from_utf8(msg.data.to_vec()) {
                            let _ = tx.send(P2pEvent::Data(text)).await;
                        }
                    })
                }));

                let tx_close = tx.clone();
                dc.on_close(Box::new(move || {
                    let tx = tx_close.clone();
                    Box::pin(async move {
                        let _ = tx.send(P2pEvent::Closed).await;
                    })
                }));

                let tx_err = tx;
                dc.on_error(Box::new(move |err| {
                    tracing::warn!("DataChannel error: {}", err);
                    let tx = tx_err.clone();
                    Box::pin(async move {
                        let _ = tx.send(P2pEvent::Closed).await;
                    })
                }));
            })
        }));

        Ok((Self { peer }, rx))
    }

    /// Handle an incoming signal from the browser (offer or ICE candidate).
    pub async fn handle_signal(
        &self,
        data: &serde_json::Value,
        tx: &mpsc::Sender<P2pEvent>,
    ) -> anyhow::Result<()> {
        let signal_type = data.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match signal_type {
            "offer" => {
                let sdp = data
                    .get("sdp")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let offer = RTCSessionDescription::offer(sdp)?;
                self.peer.set_remote_description(offer).await?;

                let answer = self.peer.create_answer(None).await?;
                self.peer.set_local_description(answer.clone()).await?;

                let signal = serde_json::json!({
                    "type": "answer",
                    "sdp": answer.sdp,
                });
                let _ = tx.send(P2pEvent::Signal(signal)).await;
            }
            "candidate" => {
                if let Some(candidate_obj) = data.get("candidate") {
                    let candidate = candidate_obj
                        .get("candidate")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    let sdp_mid = candidate_obj
                        .get("sdpMid")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    let sdp_mline_index = candidate_obj
                        .get("sdpMLineIndex")
                        .and_then(|v| v.as_u64())
                        .map(|n| n as u16);

                    let username_fragment = candidate_obj
                        .get("usernameFragment")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    let init = RTCIceCandidateInit {
                        candidate,
                        sdp_mid,
                        sdp_mline_index,
                        username_fragment,
                    };
                    self.peer.add_ice_candidate(init).await?;
                }
            }
            _ => {
                tracing::debug!("unknown P2P signal type: {}", signal_type);
            }
        }

        Ok(())
    }

    /// Close the peer connection
    pub async fn destroy(&self) {
        let _ = self.peer.close().await;
    }
}
