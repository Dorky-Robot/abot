use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{broadcast, oneshot, Mutex};

/// Client for communicating with the daemon over Unix socket NDJSON protocol
pub struct DaemonClient {
    writer: Arc<Mutex<tokio::io::WriteHalf<UnixStream>>>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<Value>>>>,
    /// Broadcast channel for daemon output events (relayed to browser clients)
    pub output_rx: broadcast::Receiver<Value>,
    output_tx: broadcast::Sender<Value>,
}

impl DaemonClient {
    pub async fn connect(sock_path: &Path) -> Result<Self> {
        let stream = UnixStream::connect(sock_path).await?;
        let (reader, writer) = tokio::io::split(stream);

        let pending: Arc<Mutex<HashMap<String, oneshot::Sender<Value>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let (output_tx, output_rx) = broadcast::channel(4096);

        // Spawn reader task
        let reader_pending = pending.clone();
        let reader_output_tx = output_tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            while let Some(line) = lines.next_line().await.unwrap_or(None) {
                if line.is_empty() {
                    continue;
                }

                match serde_json::from_str::<Value>(&line) {
                    Ok(msg) => {
                        // If message has an "id" field, it's an RPC response
                        if let Some(id) = msg.get("id").and_then(|v| v.as_str()) {
                            let mut pending = reader_pending.lock().await;
                            if let Some(tx) = pending.remove(id) {
                                let _ = tx.send(msg);
                            }
                        } else {
                            // Broadcast event (output, exit, session-removed)
                            let _ = reader_output_tx.send(msg);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("invalid daemon message: {} — {}", e, line);
                    }
                }
            }
            tracing::warn!("daemon connection closed");
        });

        Ok(Self {
            writer: Arc::new(Mutex::new(writer)),
            pending,
            output_rx,
            output_tx,
        })
    }

    /// Send an RPC request and wait for response
    pub async fn rpc(&self, mut msg: Value) -> Result<Value> {
        let id = uuid::Uuid::new_v4().to_string();
        msg["id"] = serde_json::json!(id);

        let (tx, rx) = oneshot::channel();

        {
            let mut pending = self.pending.lock().await;
            pending.insert(id.clone(), tx);
        }

        self.send_raw(&msg).await?;

        // Wait with timeout
        match tokio::time::timeout(std::time::Duration::from_secs(5), rx).await {
            Ok(Ok(resp)) => Ok(resp),
            Ok(Err(_)) => {
                let mut pending = self.pending.lock().await;
                pending.remove(&id);
                anyhow::bail!("daemon RPC cancelled")
            }
            Err(_) => {
                let mut pending = self.pending.lock().await;
                pending.remove(&id);
                anyhow::bail!("daemon RPC timeout")
            }
        }
    }

    /// Send a fire-and-forget message
    pub async fn send(&self, msg: &Value) -> Result<()> {
        self.send_raw(msg).await
    }

    /// Subscribe to daemon broadcast events
    pub fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.output_tx.subscribe()
    }

    async fn send_raw(&self, msg: &Value) -> Result<()> {
        let json = serde_json::to_string(msg)?;
        let mut writer = self.writer.lock().await;
        writer.write_all(json.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;
        Ok(())
    }
}
