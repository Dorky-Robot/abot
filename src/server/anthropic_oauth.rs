use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::auth::middleware::{Authenticated, CsrfVerified};
use crate::auth::state;
use crate::error::AppError;
use crate::server::AppState;

// Claude Code's OAuth constants (from its own PKCE flow)
const ANTHROPIC_AUTH_URL: &str = "https://claude.ai/oauth/authorize";
const ANTHROPIC_TOKEN_URL: &str = "https://claude.ai/oauth/token";
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const REDIRECT_URI: &str = "https://platform.claude.com/oauth/code/callback";
const SCOPES: &str =
    "org:create_api_key user:profile user:inference user:sessions:claude_code user:mcp_servers";

/// Max age for a PKCE challenge before it becomes stale (5 minutes).
const PKCE_TTL_SECS: i64 = 300;

/// Default token lifetime when `expires_in` is missing from the response.
const DEFAULT_EXPIRES_IN: i64 = 3600;

/// In-flight PKCE challenge (one pending flow at a time)
pub(crate) struct PkceChallenge {
    pub(crate) verifier: String,
    pub(crate) state: String,
    pub(crate) created_at: i64,
}

// --- Request / Response types ---

#[derive(Serialize)]
pub struct InitResponse {
    pub authorize_url: String,
}

#[derive(Deserialize)]
pub struct ExchangeRequest {
    pub code: String,
    pub state: String,
}

#[derive(Serialize)]
pub struct ExchangeResponse {
    pub status: String,
    pub expires_at: i64,
}

#[derive(Serialize)]
pub struct StatusResponse {
    pub status: String, // "connected", "expired", "disconnected"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<String>,
}

// Anthropic token endpoint response
#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: Option<i64>,
}

// --- Handlers ---

/// POST /api/anthropic/oauth/init
/// Generate PKCE challenge and return the authorize URL.
pub async fn init_oauth(
    _csrf: CsrfVerified,
    State(state): State<Arc<AppState>>,
) -> Result<Json<InitResponse>, AppError> {
    let verifier = generate_pkce_verifier();
    let challenge = compute_pkce_challenge(&verifier);
    let oauth_state = generate_pkce_verifier(); // random state for CSRF protection

    // Store verifier + state for the exchange step
    {
        let mut pkce = state.pkce_challenge.lock().await;
        *pkce = Some(PkceChallenge {
            verifier,
            state: oauth_state.clone(),
            created_at: chrono::Utc::now().timestamp(),
        });
    }

    let authorize_url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&code_challenge={}&code_challenge_method=S256&state={}",
        ANTHROPIC_AUTH_URL,
        percent_encode(CLIENT_ID),
        percent_encode(REDIRECT_URI),
        percent_encode(SCOPES),
        percent_encode(&challenge),
        percent_encode(&oauth_state),
    );

    Ok(Json(InitResponse { authorize_url }))
}

/// POST /api/anthropic/oauth/exchange
/// Exchange authorization code for tokens, store, and push to daemon.
pub async fn exchange_code(
    _csrf: CsrfVerified,
    State(state): State<Arc<AppState>>,
    Json(body): Json<ExchangeRequest>,
) -> Result<Json<ExchangeResponse>, AppError> {
    // Take the PKCE challenge (one-time use)
    let pkce = {
        let mut guard = state.pkce_challenge.lock().await;
        guard
            .take()
            .ok_or_else(|| AppError::BadRequest("no pending OAuth flow — call init first".into()))?
    };

    // Validate state parameter (CSRF protection)
    if pkce.state != body.state {
        return Err(AppError::BadRequest("invalid state parameter".into()));
    }

    // Validate TTL
    let now = chrono::Utc::now().timestamp();
    if now - pkce.created_at > PKCE_TTL_SECS {
        return Err(AppError::BadRequest(
            "OAuth flow expired — please try again".into(),
        ));
    }

    // Exchange code for tokens
    let params = [
        ("grant_type", "authorization_code"),
        ("client_id", CLIENT_ID),
        ("code", &body.code),
        ("redirect_uri", REDIRECT_URI),
        ("code_verifier", &pkce.verifier),
    ];

    let resp = state
        .http_client
        .post(ANTHROPIC_TOKEN_URL)
        .form(&params)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("token exchange request failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        tracing::error!("Anthropic token exchange failed ({status}): {body_text}");
        return Err(AppError::BadRequest(format!(
            "Anthropic token exchange failed ({status})"
        )));
    }

    let token_resp: TokenResponse = resp
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("failed to parse token response: {e}")))?;

    let expires_in = token_resp.expires_in.unwrap_or(DEFAULT_EXPIRES_IN);
    let expires_at = chrono::Utc::now().timestamp() + expires_in;

    // Store in DB
    {
        let db = state
            .auth
            .db
            .lock()
            .map_err(|e| AppError::Internal(e.to_string()))?;
        state::upsert_anthropic_oauth(
            &db,
            &token_resp.access_token,
            &token_resp.refresh_token,
            SCOPES,
            expires_at,
        )?;
    }

    // Push to daemon
    let env = build_env_map(
        Some(&token_resp.access_token),
        Some(&token_resp.refresh_token),
        Some(SCOPES),
    );
    push_env_to_daemon(&state, env).await;

    Ok(Json(ExchangeResponse {
        status: "connected".into(),
        expires_at,
    }))
}

/// GET /api/anthropic/oauth/status
pub async fn oauth_status(
    _auth: Authenticated,
    State(state): State<Arc<AppState>>,
) -> Result<Json<StatusResponse>, AppError> {
    let db = state
        .auth
        .db
        .lock()
        .map_err(|e| AppError::Internal(e.to_string()))?;

    match state::get_anthropic_oauth(&db)? {
        Some(row) => {
            let now = chrono::Utc::now().timestamp();
            let status = if row.expires_at > now {
                "connected"
            } else {
                "expired"
            };
            Ok(Json(StatusResponse {
                status: status.into(),
                expires_at: Some(row.expires_at),
                scopes: Some(row.scopes),
            }))
        }
        None => Ok(Json(StatusResponse {
            status: "disconnected".into(),
            expires_at: None,
            scopes: None,
        })),
    }
}

/// DELETE /api/anthropic/oauth
pub async fn disconnect_oauth(
    _csrf: CsrfVerified,
    State(state): State<Arc<AppState>>,
) -> Result<Json<StatusResponse>, AppError> {
    {
        let db = state
            .auth
            .db
            .lock()
            .map_err(|e| AppError::Internal(e.to_string()))?;
        state::delete_anthropic_oauth(&db)?;
    }

    // Push empty env to daemon (remove tokens)
    let env = build_env_map(None, None, None);
    push_env_to_daemon(&state, env).await;

    Ok(Json(StatusResponse {
        status: "disconnected".into(),
        expires_at: None,
        scopes: None,
    }))
}

// --- Token refresh ---

/// Attempt to refresh the Anthropic OAuth token. Returns the new expiry on success.
pub async fn refresh_token_if_needed(state: &Arc<AppState>) -> Option<i64> {
    let row = {
        let db = state.auth.db.lock().ok()?;
        state::get_anthropic_oauth(&db).ok()??
    };

    let now = chrono::Utc::now().timestamp();
    // Refresh if expiring within 1 hour
    if row.expires_at - now > 3600 {
        return None;
    }

    tracing::info!(
        "refreshing Anthropic OAuth token (expires in {}s)",
        row.expires_at - now
    );

    let params = [
        ("grant_type", "refresh_token"),
        ("client_id", CLIENT_ID),
        ("refresh_token", row.refresh_token.as_str()),
    ];

    let resp = match state
        .http_client
        .post(ANTHROPIC_TOKEN_URL)
        .form(&params)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("token refresh request failed: {e}");
            return None;
        }
    };

    if !resp.status().is_success() {
        tracing::error!("token refresh failed: {}", resp.status());
        return None;
    }

    let token_resp: TokenResponse = match resp.json().await {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("failed to parse refresh response: {e}");
            return None;
        }
    };

    let expires_in = token_resp.expires_in.unwrap_or(DEFAULT_EXPIRES_IN);
    let expires_at = now + expires_in;

    // Update DB
    {
        let db = match state.auth.db.lock() {
            Ok(db) => db,
            Err(_) => return None,
        };
        if let Err(e) = state::upsert_anthropic_oauth(
            &db,
            &token_resp.access_token,
            &token_resp.refresh_token,
            SCOPES,
            expires_at,
        ) {
            tracing::error!("failed to store refreshed token: {e}");
            return None;
        }
    }

    // Push to daemon
    let env = build_env_map(
        Some(&token_resp.access_token),
        Some(&token_resp.refresh_token),
        Some(SCOPES),
    );
    push_env_to_daemon(state, env).await;

    tracing::info!(
        "Anthropic OAuth token refreshed, expires in {}s",
        expires_in
    );
    Some(expires_at)
}

// --- Helpers ---

/// Build the env map for daemon IPC. None values remove the key.
pub(crate) fn build_env_map(
    access_token: Option<&str>,
    refresh_token: Option<&str>,
    scopes: Option<&str>,
) -> HashMap<String, Option<String>> {
    let mut env = HashMap::new();
    env.insert(
        "CLAUDE_CODE_OAUTH_TOKEN".into(),
        access_token.map(String::from),
    );
    env.insert(
        "CLAUDE_CODE_OAUTH_REFRESH_TOKEN".into(),
        refresh_token.map(String::from),
    );
    env.insert("CLAUDE_CODE_OAUTH_SCOPES".into(), scopes.map(String::from));
    env
}

/// Push environment update to daemon via IPC (best-effort).
pub(crate) async fn push_env_to_daemon(state: &AppState, env: HashMap<String, Option<String>>) {
    use crate::daemon::ipc::DaemonRequest;
    let req = DaemonRequest::UpdateAgentEnv {
        id: uuid::Uuid::new_v4().to_string(),
        env,
    };
    if let Err(e) = state.daemon_client.rpc(req).await {
        tracing::warn!("failed to push env to daemon: {e}");
    }
}

/// Generate a PKCE code verifier (32 random bytes, base64url-encoded).
fn generate_pkce_verifier() -> String {
    use rand::Rng;
    let bytes: Vec<u8> = (0..32).map(|_| rand::thread_rng().gen()).collect();
    base64_url_encode(&bytes)
}

/// Compute S256 PKCE challenge from verifier.
fn compute_pkce_challenge(verifier: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(verifier.as_bytes());
    base64_url_encode(&hash)
}

fn base64_url_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}

fn percent_encode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}
