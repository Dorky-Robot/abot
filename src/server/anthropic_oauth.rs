use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::auth::middleware::{Authenticated, CsrfVerified};
use crate::auth::state;
use crate::error::AppError;
use crate::server::AppState;

// --- Request / Response types ---

#[derive(Deserialize)]
pub struct SaveKeyRequest {
    pub api_key: String,
}

#[derive(Serialize)]
pub struct StatusResponse {
    pub status: String, // "connected" or "disconnected"
}

// --- Handlers ---

/// POST /api/anthropic/key — Save API key, push to daemon
pub async fn save_key(
    _csrf: CsrfVerified,
    State(state): State<Arc<AppState>>,
    Json(body): Json<SaveKeyRequest>,
) -> Result<Json<StatusResponse>, AppError> {
    let key = body.api_key.trim().to_string();
    if key.is_empty() {
        return Err(AppError::BadRequest("API key cannot be empty".into()));
    }

    // Store in DB
    {
        let db = state
            .auth
            .db
            .lock()
            .map_err(|e| AppError::Internal(e.to_string()))?;
        state::upsert_anthropic_api_key(&db, &key)?;
    }

    // Push to daemon
    let env = build_env_map(Some(&key));
    push_env_to_daemon(&state, env).await;

    Ok(Json(StatusResponse {
        status: "connected".into(),
    }))
}

/// GET /api/anthropic/key/status — Check if key is stored
pub async fn key_status(
    _auth: Authenticated,
    State(state): State<Arc<AppState>>,
) -> Result<Json<StatusResponse>, AppError> {
    let db = state
        .auth
        .db
        .lock()
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let has_key = state::get_anthropic_api_key(&db)?.is_some();
    Ok(Json(StatusResponse {
        status: if has_key { "connected" } else { "disconnected" }.into(),
    }))
}

/// DELETE /api/anthropic/key — Remove API key
pub async fn delete_key(
    _csrf: CsrfVerified,
    State(state): State<Arc<AppState>>,
) -> Result<Json<StatusResponse>, AppError> {
    {
        let db = state
            .auth
            .db
            .lock()
            .map_err(|e| AppError::Internal(e.to_string()))?;
        state::delete_anthropic_api_key(&db)?;
    }

    // Remove from daemon
    let env = build_env_map(None);
    push_env_to_daemon(&state, env).await;

    Ok(Json(StatusResponse {
        status: "disconnected".into(),
    }))
}

// --- Helpers ---

/// Build the env map for daemon IPC. None removes the keys.
/// Sets both ANTHROPIC_API_KEY (for direct API use) and CLAUDE_API_KEY (for Claude Code).
pub(crate) fn build_env_map(api_key: Option<&str>) -> HashMap<String, Option<String>> {
    let mut env = HashMap::new();
    let val = api_key.map(String::from);
    env.insert("ANTHROPIC_API_KEY".into(), val.clone());
    env.insert("CLAUDE_API_KEY".into(), val);
    env
}

/// Push environment update to daemon via IPC (best-effort).
pub(crate) async fn push_env_to_daemon(state: &AppState, env: HashMap<String, Option<String>>) {
    use crate::daemon::ipc::DaemonRequest;
    let req = DaemonRequest::UpdateAgentEnv {
        id: String::new(),
        env,
    };
    if let Err(e) = state.daemon_client.rpc(req).await {
        tracing::warn!("failed to push env to daemon: {e}");
    }
}
