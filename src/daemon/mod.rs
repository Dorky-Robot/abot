pub mod ipc;
pub mod pty;
pub mod ring_buffer;
pub mod session;

use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::{broadcast, Mutex};

use self::ipc::DaemonRequest;
use self::session::Session;

pub struct DaemonState {
    pub sessions: Mutex<HashMap<String, Session>>,
    pub data_dir: PathBuf,
    /// Broadcast channel for session output events (sent to all connected servers)
    pub output_tx: broadcast::Sender<ipc::OutputEvent>,
}

pub async fn run(data_dir: &Path) -> Result<()> {
    let sock_path = data_dir.join("daemon.sock");
    let pid_path = data_dir.join("daemon.pid");

    // Clean up stale socket
    if sock_path.exists() {
        let _ = std::fs::remove_file(&sock_path);
    }

    // Write PID file
    std::fs::write(&pid_path, std::process::id().to_string())?;

    let listener = UnixListener::bind(&sock_path)?;

    // Set socket permissions to 0o600
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o600))?;
    }

    tracing::info!("daemon listening on {:?}", sock_path);

    let (output_tx, _) = broadcast::channel(4096);

    let state = Arc::new(DaemonState {
        sessions: Mutex::new(HashMap::new()),
        data_dir: data_dir.to_path_buf(),
        output_tx,
    });

    loop {
        let (stream, _) = listener.accept().await?;
        let state = state.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_connection(state, stream).await {
                tracing::error!("daemon connection error: {}", e);
            }
        });
    }
}

async fn handle_connection(
    state: Arc<DaemonState>,
    stream: tokio::net::UnixStream,
) -> Result<()> {
    let (reader, writer) = stream.into_split();
    let reader = BufReader::new(reader);
    let writer = Arc::new(Mutex::new(writer));

    // Spawn a task to relay output events to this connection
    let mut output_rx = state.output_tx.subscribe();
    let relay_writer = writer.clone();
    let relay_handle = tokio::spawn(async move {
        loop {
            match output_rx.recv().await {
                Ok(event) => {
                    let json = match serde_json::to_string(&event) {
                        Ok(j) => j,
                        Err(_) => continue,
                    };
                    let mut w = relay_writer.lock().await;
                    if w.write_all(json.as_bytes()).await.is_err() {
                        break;
                    }
                    if w.write_all(b"\n").await.is_err() {
                        break;
                    }
                    let _ = w.flush().await;
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    let mut lines = reader.lines();
    while let Some(line) = lines.next_line().await? {
        if line.is_empty() {
            continue;
        }

        match serde_json::from_str::<DaemonRequest>(&line) {
            Ok(req) => {
                let response = ipc::handle_request(&state, req).await;
                if let Some(resp) = response {
                    let json = serde_json::to_string(&resp)?;
                    let mut w = writer.lock().await;
                    w.write_all(json.as_bytes()).await?;
                    w.write_all(b"\n").await?;
                    w.flush().await?;
                }
            }
            Err(e) => {
                tracing::warn!("invalid daemon request: {} — {}", e, line);
            }
        }
    }

    relay_handle.abort();
    Ok(())
}
