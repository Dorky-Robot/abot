use axum::extract::{ConnectInfo, Path, State};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use webauthn_rs::prelude::*;

use super::middleware;
use super::state;
use super::tokens;
use crate::error::AppError;
use crate::server::AppState;

/// GET /auth/status — is the system set up? what access method?
pub async fn status(
    State(app): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let host = headers.get("host").and_then(|v| v.to_str().ok());
    let origin = headers.get("origin").and_then(|v| v.to_str().ok());
    let is_local = middleware::is_local_request(&addr, host, origin);

    let (cred_count, session_valid) = {
        let db = app.auth.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
        let count = state::credential_count(&db)?;

        let cookie = headers.get("cookie").and_then(|v| v.to_str().ok());
        let valid = if let Some(token) = middleware::get_session_token(cookie) {
            state::validate_session(&db, &token)?
        } else {
            false
        };
        (count, valid)
    };

    let authenticated = is_local || session_valid;

    Ok(Json(json!({
        "setup": cred_count > 0,
        "accessMethod": if is_local { "localhost" } else { "internet" },
        "authenticated": authenticated,
    })))
}

/// POST /auth/register/options — get WebAuthn registration challenge
pub async fn register_options(
    State(app): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let host = headers.get("host").and_then(|v| v.to_str().ok());
    let origin = headers.get("origin").and_then(|v| v.to_str().ok());
    let is_local = middleware::is_local_request(&addr, host, origin);

    // All DB work in a sync block — drop lock before any .await
    let (user_id, user_name, existing_creds) = {
        let db = app.auth.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
        let cred_count = state::credential_count(&db)?;

        // First registration: localhost only. Subsequent: need setup token or localhost.
        if cred_count > 0 && !is_local {
            let setup_token = body.get("setupToken").and_then(|v| v.as_str());
            if setup_token.is_none() {
                return Err(AppError::Unauthorized);
            }
            let token_value = setup_token.unwrap();
            let token_rows = state::get_setup_tokens(&db)?;
            let mut found = false;
            for row in &token_rows {
                if let Some(hash) = state::get_setup_token_hash(&db, &row.id)? {
                    if tokens::verify_token(token_value, &hash)? {
                        found = true;
                        break;
                    }
                }
            }
            if !found {
                return Err(AppError::Unauthorized);
            }
        }

        let (uid, uname) = match state::get_user(&db)? {
            Some((id, name)) => (id, name),
            None => {
                let id = uuid::Uuid::new_v4().to_string();
                let name = "admin".to_string();
                state::create_user(&db, &id, &name)?;
                (id, name)
            }
        };

        let creds = state::get_credentials(&db)?;
        (uid, uname, creds)
    }; // db lock dropped here

    let exclude: Vec<Passkey> = existing_creds
        .iter()
        .filter_map(|c| serde_json::from_slice(&c.public_key).ok())
        .collect();

    let user_unique_id = Uuid::parse_str(&user_id).unwrap_or_else(|_| Uuid::new_v4());

    let exclude_creds: Option<Vec<CredentialID>> = if exclude.is_empty() {
        None
    } else {
        Some(exclude.iter().map(|p| p.cred_id().clone()).collect())
    };

    let (ccr, reg_state) = app
        .auth
        .webauthn
        .start_passkey_registration(user_unique_id, &user_name, &user_name, exclude_creds)
        .map_err(|e| AppError::Internal(format!("webauthn error: {}", e)))?;

    // Store challenge state with per-attempt key (async, db lock is already dropped)
    let challenge_id = uuid::Uuid::new_v4().to_string();
    let challenge_key = format!("reg:{}", challenge_id);
    let state_json = serde_json::to_value(&reg_state)
        .map_err(|e| AppError::Internal(format!("serialize error: {}", e)))?;
    app.auth.challenges.store(challenge_key, state_json).await;

    Ok(Json(json!({
        "options": ccr,
        "userId": user_id,
        "challengeId": challenge_id,
    })))
}

/// POST /auth/register/verify — complete WebAuthn registration
pub async fn register_verify(
    State(app): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<impl IntoResponse, AppError> {
    let user_id = body
        .get("userId")
        .and_then(|v| v.as_str())
        .ok_or(AppError::BadRequest("missing userId".into()))?
        .to_string();

    let credential: RegisterPublicKeyCredential = serde_json::from_value(
        body.get("credential")
            .cloned()
            .ok_or(AppError::BadRequest("missing credential".into()))?,
    )
    .map_err(|e| AppError::BadRequest(format!("invalid credential: {}", e)))?;

    // Consume per-attempt challenge (async, before db lock)
    let challenge_id = body
        .get("challengeId")
        .and_then(|v| v.as_str())
        .ok_or(AppError::BadRequest("missing challengeId".into()))?;
    let challenge_key = format!("reg:{}", challenge_id);
    let state_json = app
        .auth
        .challenges
        .consume(&challenge_key)
        .await
        .ok_or(AppError::BadRequest("invalid or expired challenge".into()))?;

    let reg_state: PasskeyRegistration = serde_json::from_value(state_json)
        .map_err(|e| AppError::Internal(format!("deserialize state: {}", e)))?;

    let passkey = app
        .auth
        .webauthn
        .finish_passkey_registration(&credential, &reg_state)
        .map_err(|e| AppError::BadRequest(format!("registration failed: {}", e)))?;

    let cred_id = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        passkey.cred_id().as_ref(),
    );
    let public_key = serde_json::to_vec(&passkey)
        .map_err(|e| AppError::Internal(format!("serialize passkey: {}", e)))?;

    let device_name = body.get("deviceName").and_then(|v| v.as_str());
    let device_id = body.get("deviceId").and_then(|v| v.as_str());
    let user_agent = headers.get("user-agent").and_then(|v| v.to_str().ok());
    let setup_token_id = body.get("setupTokenId").and_then(|v| v.as_str());

    let session_token = tokens::generate_token();
    let csrf_token = tokens::generate_token();
    let expiry = chrono::Utc::now().timestamp() + 30 * 24 * 60 * 60;

    {
        let db = app.auth.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
        state::add_credential(
            &db,
            &cred_id,
            &user_id,
            &public_key,
            0,
            device_id,
            device_name,
            user_agent,
            setup_token_id,
        )?;
        state::create_session(&db, &session_token, &cred_id, &csrf_token, expiry)?;
    }

    let host = headers.get("host").and_then(|v| v.to_str().ok());
    let is_secure = middleware::is_secure_host(host);
    let cookie = middleware::session_cookie(&session_token, 30 * 24 * 60 * 60, is_secure);

    Ok((
        [(axum::http::header::SET_COOKIE, cookie)],
        Json(json!({
            "success": true,
            "csrfToken": csrf_token,
        })),
    ))
}

/// POST /auth/login/options — get WebAuthn authentication challenge
pub async fn login_options(
    State(app): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let passkeys: Vec<Passkey> = {
        let db = app.auth.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
        let cred_rows = state::get_credentials(&db)?;
        if cred_rows.is_empty() {
            return Err(AppError::BadRequest("not set up".into()));
        }
        cred_rows
            .iter()
            .filter_map(|c| serde_json::from_slice(&c.public_key).ok())
            .collect()
    };

    let (rcr, auth_state) = app
        .auth
        .webauthn
        .start_passkey_authentication(&passkeys)
        .map_err(|e| AppError::Internal(format!("webauthn error: {}", e)))?;

    let challenge_id = uuid::Uuid::new_v4().to_string();
    let challenge_key = format!("login:{}", challenge_id);
    let state_json = serde_json::to_value(&auth_state)
        .map_err(|e| AppError::Internal(format!("serialize error: {}", e)))?;
    app.auth.challenges.store(challenge_key, state_json).await;

    Ok(Json(json!({ "options": rcr, "challengeId": challenge_id })))
}

/// POST /auth/login/verify — complete WebAuthn authentication
pub async fn login_verify(
    State(app): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<impl IntoResponse, AppError> {
    let credential: PublicKeyCredential = serde_json::from_value(
        body.get("credential")
            .cloned()
            .ok_or(AppError::BadRequest("missing credential".into()))?,
    )
    .map_err(|e| AppError::BadRequest(format!("invalid credential: {}", e)))?;

    // Consume per-attempt challenge (async)
    let challenge_id = body
        .get("challengeId")
        .and_then(|v| v.as_str())
        .ok_or(AppError::BadRequest("missing challengeId".into()))?;
    let challenge_key = format!("login:{}", challenge_id);
    let state_json = app
        .auth
        .challenges
        .consume(&challenge_key)
        .await
        .ok_or(AppError::BadRequest("invalid or expired challenge".into()))?;

    let auth_state: PasskeyAuthentication = serde_json::from_value(state_json)
        .map_err(|e| AppError::Internal(format!("deserialize state: {}", e)))?;

    // Check global lockout before attempting authentication
    let (locked, retry_after) = app.auth.lockout.is_locked("_global").await;
    if locked {
        let secs = retry_after.unwrap_or(60);
        return Err(AppError::BadRequest(format!("too many failed attempts, try again in {} seconds", secs)));
    }

    let auth_result = match app
        .auth
        .webauthn
        .finish_passkey_authentication(&credential, &auth_state)
    {
        Ok(result) => result,
        Err(e) => {
            app.auth.lockout.record_failure("_global").await;
            return Err(AppError::BadRequest(format!("authentication failed: {}", e)));
        }
    };

    let cred_id = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        auth_result.cred_id().as_ref(),
    );

    app.auth.lockout.record_success(&cred_id).await;

    let session_token = tokens::generate_token();
    let csrf_token = tokens::generate_token();
    let expiry = chrono::Utc::now().timestamp() + 30 * 24 * 60 * 60;

    {
        let db = app.auth.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
        state::update_credential_counter(&db, &cred_id, auth_result.counter())?;
        state::create_session(&db, &session_token, &cred_id, &csrf_token, expiry)?;
    }

    let host = headers.get("host").and_then(|v| v.to_str().ok());
    let is_secure = middleware::is_secure_host(host);
    let cookie = middleware::session_cookie(&session_token, 30 * 24 * 60 * 60, is_secure);

    Ok((
        [(axum::http::header::SET_COOKIE, cookie)],
        Json(json!({
            "success": true,
            "csrfToken": csrf_token,
        })),
    ))
}

/// POST /auth/logout
pub async fn logout(
    State(app): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    let cookie_header = headers.get("cookie").and_then(|v| v.to_str().ok());
    if let Some(token) = middleware::get_session_token(cookie_header) {
        let db = app.auth.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
        state::delete_session(&db, &token)?;
    }

    let cookie = middleware::clear_session_cookie();
    Ok((
        [(axum::http::header::SET_COOKIE, cookie)],
        Json(json!({ "success": true })),
    ))
}

/// GET /auth/tokens — list setup tokens
pub async fn list_tokens(
    State(app): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    middleware::require_auth(&app, &addr, &headers)?;
    let db = app.auth.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
    let token_rows = state::get_setup_tokens(&db)?;
    Ok(Json(json!({ "tokens": token_rows })))
}

/// POST /auth/tokens — create a setup token
pub async fn create_token(
    State(app): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    middleware::require_auth(&app, &addr, &headers)?;
    let name = body
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("New device");

    let token_value = tokens::generate_token();
    let hash = tokens::hash_token(&token_value)?;
    let id = uuid::Uuid::new_v4().to_string();
    let expires_at = chrono::Utc::now().timestamp() + 24 * 60 * 60;

    let db = app.auth.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
    state::add_setup_token(&db, &id, name, &hash, expires_at)?;

    Ok(Json(json!({
        "id": id,
        "token": token_value,
        "expiresAt": expires_at,
    })))
}

/// GET /api/credentials — list all credentials (for katulong client compatibility)
pub async fn list_credentials(
    State(app): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    middleware::require_auth(&app, &addr, &headers)?;
    let db = app.auth.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
    let cred_rows = state::get_credentials(&db)?;
    let credentials: Vec<serde_json::Value> = cred_rows
        .iter()
        .map(|c| {
            json!({
                "id": c.id,
                "name": c.name,
                "createdAt": c.created_at,
                "lastUsedAt": c.last_used_at,
                "userAgent": c.user_agent,
            })
        })
        .collect();
    Ok(Json(json!({ "credentials": credentials })))
}

/// DELETE /auth/tokens/:id
pub async fn delete_token(
    State(app): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    middleware::require_auth(&app, &addr, &headers)?;
    let db = app.auth.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
    state::delete_setup_token(&db, &id)?;
    Ok(Json(json!({ "success": true })))
}
