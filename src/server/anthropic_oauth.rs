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

/// POST /api/anthropic/key — Save API key or setup token, push to daemon
pub async fn save_key(
    _csrf: CsrfVerified,
    State(state): State<Arc<AppState>>,
    Json(body): Json<SaveKeyRequest>,
) -> Result<Json<StatusResponse>, AppError> {
    let key = body.api_key.trim().to_string();
    if key.is_empty() {
        return Err(AppError::BadRequest("token cannot be empty".into()));
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

    // Push to daemon — detect key type and set the right env var
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

/// DELETE /api/anthropic/key — Remove key/token
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

    // Remove all credential env vars from daemon
    let env = build_env_map(None);
    push_env_to_daemon(&state, env).await;

    Ok(Json(StatusResponse {
        status: "disconnected".into(),
    }))
}

// --- Helpers ---

/// Build the env map for daemon IPC. Detects token type:
/// - `sk-ant-*` → ANTHROPIC_API_KEY (API key billing)
/// - anything else → CLAUDE_CODE_OAUTH_TOKEN (subscription auth from `claude setup-token`)
pub(crate) fn build_env_map(token: Option<&str>) -> HashMap<String, Option<String>> {
    let mut env = HashMap::new();
    match token {
        Some(t) if t.starts_with("sk-ant-api") => {
            // API key (sk-ant-api...) — set ANTHROPIC_API_KEY, clear OAuth token
            env.insert("ANTHROPIC_API_KEY".into(), Some(t.to_string()));
            env.insert("CLAUDE_API_KEY".into(), Some(t.to_string()));
            env.insert("CLAUDE_CODE_OAUTH_TOKEN".into(), None);
        }
        Some(t) => {
            // OAuth setup token (sk-ant-oat... or anything else) — set CLAUDE_CODE_OAUTH_TOKEN
            env.insert("CLAUDE_CODE_OAUTH_TOKEN".into(), Some(t.to_string()));
            env.insert("ANTHROPIC_API_KEY".into(), None);
            env.insert("CLAUDE_API_KEY".into(), None);
        }
        None => {
            // Clear everything
            env.insert("ANTHROPIC_API_KEY".into(), None);
            env.insert("CLAUDE_API_KEY".into(), None);
            env.insert("CLAUDE_CODE_OAUTH_TOKEN".into(), None);
        }
    }
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
