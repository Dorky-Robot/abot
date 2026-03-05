use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{broadcast, oneshot, Mutex};

/// Client for communicating with the daemon over Unix socket NDJSON protocol.
/// Automatically reconnects if the daemon connection drops.
pub struct DaemonClient {
    writer: Arc<Mutex<Option<tokio::io::WriteHalf<UnixStream>>>>,
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

        let writer = Arc::new(Mutex::new(Some(writer)));

        // Spawn reader task with reconnection
        let reader_pending = pending.clone();
        let reader_output_tx = output_tx.clone();
        let reader_writer = writer.clone();
        let sock_path_owned = sock_path.to_path_buf();
        tokio::spawn(Self::reader_loop(
            reader,
            reader_pending.clone(),
            reader_output_tx,
            reader_writer,
            sock_path_owned,
        ));

        Ok(Self {
            writer,
            pending,
            output_rx,
            output_tx,
        })
    }

    /// Reader loop that processes daemon messages and reconnects on disconnect.
    async fn reader_loop(
        initial_reader: tokio::io::ReadHalf<UnixStream>,
        pending: Arc<Mutex<HashMap<String, oneshot::Sender<Value>>>>,
        output_tx: broadcast::Sender<Value>,
        writer: Arc<Mutex<Option<tokio::io::WriteHalf<UnixStream>>>>,
        sock_path: PathBuf,
    ) {
        // Process the initial connection
        Self::process_reader(initial_reader, &pending, &output_tx).await;

        // Reconnection loop
        let mut backoff_ms = 500u64;
        loop {
            tracing::warn!(
                "daemon connection lost, reconnecting in {}ms...",
                backoff_ms
            );

            // Clear the writer so send_raw fails fast
            {
                let mut w = writer.lock().await;
                *w = None;
            }

            // Fail all pending RPCs so callers don't hang
            {
                let mut p = pending.lock().await;
                p.clear();
            }

            tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;

            match UnixStream::connect(&sock_path).await {
                Ok(stream) => {
                    tracing::info!("reconnected to daemon");
                    let (reader, new_writer) = tokio::io::split(stream);

                    // Replace the writer
                    {
                        let mut w = writer.lock().await;
                        *w = Some(new_writer);
                    }

                    backoff_ms = 500; // reset backoff
                    Self::process_reader(reader, &pending, &output_tx).await;
                    // If we reach here, the connection dropped again — loop
                }
                Err(e) => {
                    tracing::warn!("daemon reconnect failed: {}", e);
                    backoff_ms = (backoff_ms * 2).min(10_000); // cap at 10s
                }
            }
        }
    }

    /// Read NDJSON lines from the daemon, dispatching RPCs and broadcasts.
    async fn process_reader(
        reader: tokio::io::ReadHalf<UnixStream>,
        pending: &Arc<Mutex<HashMap<String, oneshot::Sender<Value>>>>,
        output_tx: &broadcast::Sender<Value>,
    ) {
        let mut lines = BufReader::new(reader).lines();
        while let Some(line) = lines.next_line().await.unwrap_or(None) {
            if line.is_empty() {
                continue;
            }

            match serde_json::from_str::<Value>(&line) {
                Ok(msg) => {
                    if let Some(id) = msg.get("id").and_then(|v| v.as_str()) {
                        let mut pending = pending.lock().await;
                        if let Some(tx) = pending.remove(id) {
                            let _ = tx.send(msg);
                        }
                    } else {
                        let _ = output_tx.send(msg);
                    }
                }
                Err(e) => {
                    tracing::warn!("invalid daemon message: {} — {}", e, line);
                }
            }
        }
    }

    /// Send an RPC request and wait for response.
    /// Accepts any Serialize type — injects a unique `id` for RPC correlation.
    pub async fn rpc(&self, msg: impl Serialize) -> Result<Value> {
        self.rpc_with_timeout(msg, std::time::Duration::from_secs(5))
            .await
    }

    /// RPC with a custom timeout — use for slow operations (Docker start, image pull).
    pub async fn rpc_with_timeout(
        &self,
        msg: impl Serialize,
        timeout: std::time::Duration,
    ) -> Result<Value> {
        let id = uuid::Uuid::new_v4().to_string();
        let mut msg = serde_json::to_value(msg)?;
        msg["id"] = serde_json::json!(id);

        let (tx, rx) = oneshot::channel();

        {
            let mut pending = self.pending.lock().await;
            pending.insert(id.clone(), tx);
        }

        self.send_raw(&msg).await?;

        // Wait with timeout
        match tokio::time::timeout(timeout, rx).await {
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

    /// Send a fire-and-forget message. Accepts any Serialize type.
    pub async fn send(&self, msg: &impl Serialize) -> Result<()> {
        let value = serde_json::to_value(msg)?;
        self.send_raw(&value).await
    }

    /// Subscribe to daemon broadcast events
    pub fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.output_tx.subscribe()
    }

    /// Check if the daemon is alive and responsive.
    pub async fn ping(&self) -> bool {
        use crate::daemon::ipc::DaemonRequest;
        self.rpc(DaemonRequest::Ping { id: String::new() })
            .await
            .is_ok()
    }

    async fn send_raw(&self, msg: &Value) -> Result<()> {
        let json = serde_json::to_string(msg)?;
        let mut guard = self.writer.lock().await;
        let writer = guard
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("daemon not connected"))?;
        writer.write_all(json.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;
        Ok(())
    }
}
