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
        let db = app
            .auth
            .db
            .lock()
            .map_err(|e| AppError::Internal(e.to_string()))?;
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
    let (user_id, user_name, existing_creds, found_token_id) = {
        let db = app
            .auth
            .db
            .lock()
            .map_err(|e| AppError::Internal(e.to_string()))?;
        let cred_count = state::credential_count(&db)?;

        // First registration: localhost only. Subsequent: need setup token or localhost.
        if cred_count == 0 && !is_local {
            return Err(AppError::Unauthorized);
        }
        let mut setup_token_id: Option<String> = None;
        if cred_count > 0 && !is_local {
            let setup_token = body.get("setupToken").and_then(|v| v.as_str());
            if setup_token.is_none() {
                return Err(AppError::Unauthorized);
            }
            let token_value = setup_token.unwrap();
            let token_rows = state::get_setup_tokens(&db)?;
            for row in &token_rows {
                if let Some(hash) = state::get_setup_token_hash(&db, &row.id)? {
                    if tokens::verify_token(token_value, &hash)? {
                        setup_token_id = Some(row.id.clone());
                        break;
                    }
                }
            }
            if setup_token_id.is_none() {
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
        (uid, uname, creds, setup_token_id)
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
    let reg_state_json = serde_json::to_value(&reg_state)
        .map_err(|e| AppError::Internal(format!("serialize error: {}", e)))?;
    let challenge_data = json!({
        "regState": reg_state_json,
        "setupTokenId": found_token_id,
    });
    app.auth
        .challenges
        .store(challenge_key, challenge_data)
        .await;

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

    // Extract regState and setupTokenId from challenge data
    let setup_token_id = state_json
        .get("setupTokenId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let reg_state_value = state_json.get("regState").cloned().unwrap_or(state_json);
    let reg_state: PasskeyRegistration = serde_json::from_value(reg_state_value)
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

    let session_token = tokens::generate_token();
    let csrf_token = tokens::generate_token();
    let expiry = chrono::Utc::now().timestamp() + 30 * 24 * 60 * 60;

    {
        let db = app
            .auth
            .db
            .lock()
            .map_err(|e| AppError::Internal(e.to_string()))?;
        state::add_credential(
            &db,
            &cred_id,
            &user_id,
            &public_key,
            0,
            device_id,
            device_name,
            user_agent,
            setup_token_id.as_deref(),
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
        let db = app
            .auth
            .db
            .lock()
            .map_err(|e| AppError::Internal(e.to_string()))?;
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
        return Err(AppError::BadRequest(format!(
            "too many failed attempts, try again in {} seconds",
            secs
        )));
    }

    let auth_result = match app
        .auth
        .webauthn
        .finish_passkey_authentication(&credential, &auth_state)
    {
        Ok(result) => result,
        Err(e) => {
            app.auth.lockout.record_failure("_global").await;
            return Err(AppError::BadRequest(format!(
                "authentication failed: {}",
                e
            )));
        }
    };

    let cred_id = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        auth_result.cred_id().as_ref(),
    );

    app.auth.lockout.record_success("_global").await;

    let session_token = tokens::generate_token();
    let csrf_token = tokens::generate_token();
    let expiry = chrono::Utc::now().timestamp() + 30 * 24 * 60 * 60;

    {
        let db = app
            .auth
            .db
            .lock()
            .map_err(|e| AppError::Internal(e.to_string()))?;
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
        let db = app
            .auth
            .db
            .lock()
            .map_err(|e| AppError::Internal(e.to_string()))?;
        state::delete_session(&db, &token)?;
    }

    let cookie = middleware::clear_session_cookie();
    Ok((
        [(axum::http::header::SET_COOKIE, cookie)],
        Json(json!({ "success": true })),
    ))
}

fn credential_to_json(c: &state::CredentialRow) -> serde_json::Value {
    json!({
        "id": c.id,
        "name": c.name,
        "createdAt": c.created_at,
        "lastUsedAt": c.last_used_at,
        "userAgent": c.user_agent,
    })
}

/// Revoke a credential: check not-last guard, delete sessions + credential.
fn revoke_credential(
    db: &rusqlite::Connection,
    credential_id: &str,
    is_local: bool,
) -> Result<(), AppError> {
    let cred_count = state::credential_count(db)?;
    if cred_count <= 1 && !is_local {
        return Err(AppError::Forbidden(
            "cannot delete last credential remotely".into(),
        ));
    }
    state::delete_sessions_for_credential(db, credential_id)?;
    state::delete_credential(db, credential_id)?;
    Ok(())
}

/// GET /auth/tokens — list setup tokens enriched with linked credentials
pub async fn list_tokens(
    State(app): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    middleware::require_auth(&app, &addr, &headers)?;
    let db = app
        .auth
        .db
        .lock()
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let token_rows = state::get_setup_tokens(&db)?;
    let all_creds = state::get_credentials(&db)?;

    // Build set of credential IDs that are linked to a token
    let mut linked_cred_ids = std::collections::HashSet::new();

    let mut tokens: Vec<serde_json::Value> = Vec::with_capacity(token_rows.len());
    for t in &token_rows {
        let cred = state::get_credential_for_token(&db, &t.id)?;
        if let Some(ref c) = cred {
            linked_cred_ids.insert(c.id.clone());
        }
        tokens.push(json!({
            "id": t.id,
            "name": t.name,
            "createdAt": t.created_at,
            "expiresAt": t.expires_at,
            "credential": cred.map(|c| credential_to_json(&c)),
        }));
    }

    // Orphaned credentials: have no matching setup token
    let orphaned: Vec<serde_json::Value> = all_creds
        .iter()
        .filter(|c| !linked_cred_ids.contains(&c.id) && c.setup_token_id.is_none())
        .map(credential_to_json)
        .collect();

    Ok(Json(json!({
        "tokens": tokens,
        "orphanedCredentials": orphaned,
    })))
}

/// POST /auth/tokens — create a setup token
pub async fn create_token(
    State(app): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    middleware::require_auth(&app, &addr, &headers)?;
    middleware::require_csrf(&app, &addr, &headers)?;
    let name = body
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("New device");

    let token_value = tokens::generate_token();
    let hash = tokens::hash_token(&token_value)?;
    let id = uuid::Uuid::new_v4().to_string();
    let expires_at = chrono::Utc::now().timestamp() + 24 * 60 * 60;

    let db = app
        .auth
        .db
        .lock()
        .map_err(|e| AppError::Internal(e.to_string()))?;
    state::add_setup_token(&db, &id, name, &hash, expires_at)?;

    Ok(Json(json!({
        "id": id,
        "token": token_value,
        "expiresAt": expires_at,
    })))
}

/// GET /api/credentials — list all credentials
pub async fn list_credentials(
    State(app): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    middleware::require_auth(&app, &addr, &headers)?;
    let db = app
        .auth
        .db
        .lock()
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let cred_rows = state::get_credentials(&db)?;
    let credentials: Vec<serde_json::Value> = cred_rows.iter().map(credential_to_json).collect();
    Ok(Json(json!({ "credentials": credentials })))
}

/// DELETE /auth/tokens/:id — cascade: delete linked credential + sessions + close WS
pub async fn delete_token(
    State(app): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let host = headers.get("host").and_then(|v| v.to_str().ok());
    let origin = headers.get("origin").and_then(|v| v.to_str().ok());
    let is_local = middleware::is_local_request(&addr, host, origin);
    middleware::require_auth(&app, &addr, &headers)?;
    middleware::require_csrf(&app, &addr, &headers)?;

    let credential_id_to_close = {
        let db = app
            .auth
            .db
            .lock()
            .map_err(|e| AppError::Internal(e.to_string()))?;

        // Find linked credential and cascade
        let cred_id = if let Some(cred) = state::get_credential_for_token(&db, &id)? {
            revoke_credential(&db, &cred.id, is_local)?;
            Some(cred.id)
        } else {
            None
        };
        state::delete_setup_token(&db, &id)?;
        cred_id
    }; // db lock dropped

    // Close WS connections outside db lock
    if let Some(cred_id) = credential_id_to_close {
        let removed = app.stream_clients.close_by_credential(&cred_id).await;
        if !removed.is_empty() {
            tracing::info!(
                "revoked credential {}: closed {} WS connections",
                cred_id,
                removed.len()
            );
        }
    }

    Ok(Json(json!({ "success": true })))
}

/// DELETE /api/credentials/:id — delete an orphaned credential + its sessions + close WS
pub async fn delete_credential(
    State(app): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let host = headers.get("host").and_then(|v| v.to_str().ok());
    let origin = headers.get("origin").and_then(|v| v.to_str().ok());
    let is_local = middleware::is_local_request(&addr, host, origin);
    middleware::require_auth(&app, &addr, &headers)?;
    middleware::require_csrf(&app, &addr, &headers)?;

    {
        let db = app
            .auth
            .db
            .lock()
            .map_err(|e| AppError::Internal(e.to_string()))?;
        revoke_credential(&db, &id, is_local)?;
    }

    let removed = app.stream_clients.close_by_credential(&id).await;
    if !removed.is_empty() {
        tracing::info!(
            "deleted credential {}: closed {} WS connections",
            id,
            removed.len()
        );
    }

    Ok(Json(json!({ "success": true })))
}
