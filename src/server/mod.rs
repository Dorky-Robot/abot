pub mod assets;
pub mod daemon_client;
pub mod router;

use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

use crate::auth;
use crate::stream;

pub struct AppState {
    pub auth: auth::AuthState,
    pub daemon_client: daemon_client::DaemonClient,
    pub stream_clients: stream::clients::ClientTracker,
    pub data_dir: std::path::PathBuf,
}

pub async fn run(addr: &str, data_dir: &Path) -> Result<()> {
    // Initialize SQLite database
    let db_path = data_dir.join("abot.db");
    let db = auth::state::init_db(&db_path)?;

    // Connect to daemon
    let sock_path = data_dir.join("daemon.sock");
    let daemon_client = daemon_client::DaemonClient::connect(&sock_path).await?;

    let state = Arc::new(AppState {
        auth: auth::AuthState::new(db, addr)?,
        daemon_client,
        stream_clients: stream::clients::ClientTracker::new(),
        data_dir: data_dir.to_path_buf(),
    });

    let app = router::build(state.clone());

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("server listening on {}", addr);

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;

    Ok(())
}
