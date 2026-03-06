pub mod abots;
pub mod anthropic_oauth;
pub mod assets;
pub mod browse;
pub mod config;
pub mod kubos;
pub mod router;
pub mod sessions;
pub mod shortcuts;

use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

use crate::auth;
use crate::engine::Engine;
use crate::stream;

pub struct AppState {
    pub auth: auth::AuthState,
    pub engine: Arc<Engine>,
    pub stream_clients: stream::clients::ClientTracker,
    pub data_dir: std::path::PathBuf,
}

pub async fn run(addr: &str, data_dir: &Path) -> Result<()> {
    // Initialize SQLite database
    let db_path = data_dir.join("abot.db");
    let db = auth::state::init_db(&db_path)?;

    // Create the engine (replaces daemon)
    let engine = Engine::new(data_dir).await?;

    let state = Arc::new(AppState {
        auth: auth::AuthState::new(db, addr)?,
        engine: engine.clone(),
        stream_clients: stream::clients::ClientTracker::new(),
        data_dir: data_dir.to_path_buf(),
    });

    // Push stored token/key to engine at startup
    {
        let token = {
            let db = state.auth.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
            auth::state::get_anthropic_api_key(&db).ok().flatten()
        };
        if let Some(t) = token {
            let env = anthropic_oauth::build_env_map(Some(&t));
            anthropic_oauth::push_env_to_engine(&state, env).await;
            tracing::info!("pushed stored credentials to engine");
        }
    }

    let app = router::build(state.clone());

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("server listening on {}", addr);

    // Write server PID file
    let pid_path = data_dir.join("server.pid");
    std::fs::write(&pid_path, std::process::id().to_string())?;

    // Graceful shutdown: wait for SIGTERM, save scrollback, broadcast drain, then exit
    let drain_state = state.clone();
    let shutdown_engine = engine.clone();
    let shutdown = async move {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm =
            signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");
        let mut sigint = signal(SignalKind::interrupt()).expect("failed to install SIGINT handler");

        tokio::select! {
            _ = sigterm.recv() => {
                tracing::info!("SIGTERM received, saving scrollback and draining");
            }
            _ = sigint.recv() => {
                tracing::info!("SIGINT received, saving scrollback and draining");
            }
        }

        shutdown_engine.save_all_scrollback().await;
        drain_state
            .stream_clients
            .broadcast_all(stream::messages::ServerMessage::ServerDraining)
            .await;
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    };

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
    .await?;

    // Clean up PID file
    let _ = std::fs::remove_file(&pid_path);
    tracing::info!("server stopped");

    Ok(())
}
