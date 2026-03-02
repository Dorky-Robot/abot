use axum::http::HeaderMap;
use std::net::SocketAddr;

use super::state;
use crate::error::AppError;
use crate::server::AppState;

/// Check if a request originates from localhost.
/// Must validate socket addr + Host header + Origin header (if present).
/// Prevents tunnel traffic from bypassing auth (ngrok forwards from loopback).
pub fn is_local_request(
    remote_addr: &SocketAddr,
    host: Option<&str>,
    origin: Option<&str>,
) -> bool {
    // 1. Socket address must be loopback
    if !remote_addr.ip().is_loopback() {
        return false;
    }

    // 2. Host header must be localhost
    if let Some(host) = host {
        let host_without_port = host.split(':').next().unwrap_or(host);
        if !is_localhost_host(host_without_port) {
            return false;
        }
    }

    // 3. Origin header (if present) must be localhost
    if let Some(origin) = origin {
        if let Some(host) = extract_host_from_origin(origin) {
            let host_without_port = host.split(':').next().unwrap_or(host);
            if !is_localhost_host(host_without_port) {
                return false;
            }
        }
    }

    true
}

pub(crate) fn is_localhost_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1")
}

/// True when the Host header indicates a non-localhost origin (cookies need Secure flag).
pub(crate) fn is_secure_host(host: Option<&str>) -> bool {
    host.map(|h| {
        let h = h.split(':').next().unwrap_or(h);
        !is_localhost_host(h)
    })
    .unwrap_or(false)
}

/// Verify that a request is either local or carries a valid session cookie.
pub(crate) fn require_auth(
    app: &AppState,
    addr: &SocketAddr,
    headers: &HeaderMap,
) -> Result<(), AppError> {
    let host = headers.get("host").and_then(|v| v.to_str().ok());
    let origin = headers.get("origin").and_then(|v| v.to_str().ok());
    let is_local = is_local_request(addr, host, origin);

    if !is_local {
        let cookie = headers.get("cookie").and_then(|v| v.to_str().ok());
        let authenticated = if let Some(token) = get_session_token(cookie) {
            let db = app
                .auth
                .db
                .lock()
                .map_err(|e| AppError::Internal(e.to_string()))?;
            state::validate_auth_grant(&db, &token)?
        } else {
            false
        };
        if !authenticated {
            return Err(AppError::Unauthorized);
        }
    }

    Ok(())
}

fn extract_host_from_origin(origin: &str) -> Option<&str> {
    // Origin format: "http://localhost:6969" or "https://example.com"
    origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"))
}

/// Parse session token from cookie header
pub fn get_session_token(cookie_header: Option<&str>) -> Option<String> {
    let header = cookie_header?;
    for part in header.split(';') {
        let trimmed = part.trim();
        if let Some(value) = trimmed.strip_prefix("abot_session=") {
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Build Set-Cookie header for session
pub fn session_cookie(token: &str, expiry_secs: i64, secure: bool) -> String {
    let mut cookie = format!(
        "abot_session={}; HttpOnly; SameSite=Lax; Path=/; Max-Age={}",
        token, expiry_secs
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

/// Build Set-Cookie header to clear session
pub fn clear_session_cookie() -> String {
    "abot_session=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0".to_string()
}

/// Validate CSRF token from request header against session
pub fn validate_csrf(csrf_header: Option<&str>, expected: &str) -> bool {
    match csrf_header {
        Some(token) => constant_time_eq(token.as_bytes(), expected.as_bytes()),
        None => false,
    }
}

/// Look up the CSRF token for the current session (from cookie).
/// Returns None for localhost or if no valid session exists.
pub fn get_session_csrf(app: &AppState, headers: &HeaderMap) -> Option<String> {
    let cookie = headers.get("cookie").and_then(|v| v.to_str().ok());
    let token = get_session_token(cookie)?;
    let db = app.auth.db.lock().ok()?;
    let row = state::get_auth_grant(&db, &token).ok()??;
    Some(row.csrf_token)
}

/// Validate CSRF for mutating requests. Call this from POST/PUT/DELETE handlers.
/// Skips validation for localhost requests.
pub fn require_csrf(
    app: &AppState,
    addr: &SocketAddr,
    headers: &HeaderMap,
) -> Result<(), AppError> {
    let host = headers.get("host").and_then(|v| v.to_str().ok());
    let origin = headers.get("origin").and_then(|v| v.to_str().ok());

    // Localhost is exempt from CSRF (same-origin by definition)
    if is_local_request(addr, host, origin) {
        return Ok(());
    }

    let csrf_header = headers.get("x-csrf-token").and_then(|v| v.to_str().ok());

    let expected = get_session_csrf(app, headers).ok_or(AppError::Unauthorized)?;

    if !validate_csrf(csrf_header, &expected) {
        return Err(AppError::Forbidden("invalid CSRF token".into()));
    }

    Ok(())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_localhost_detection() {
        let addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        assert!(is_local_request(&addr, Some("localhost:6969"), None));
        assert!(is_local_request(&addr, Some("127.0.0.1:6969"), None));
        assert!(!is_local_request(&addr, Some("example.ngrok.app"), None));
    }

    #[test]
    fn test_origin_check() {
        let addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        assert!(is_local_request(
            &addr,
            Some("localhost:6969"),
            Some("http://localhost:6969")
        ));
        assert!(!is_local_request(
            &addr,
            Some("localhost:6969"),
            Some("https://evil.com")
        ));
    }

    #[test]
    fn test_cookie_parsing() {
        assert_eq!(
            get_session_token(Some("abot_session=abc123; other=val")),
            Some("abc123".into())
        );
        assert_eq!(get_session_token(Some("other=val")), None);
        assert_eq!(get_session_token(None), None);
    }

    #[test]
    fn test_csrf_validation() {
        assert!(validate_csrf(Some("abc123"), "abc123"));
        assert!(!validate_csrf(Some("wrong"), "abc123"));
        assert!(!validate_csrf(None, "abc123"));
        assert!(!validate_csrf(Some("abc"), "abc123")); // different length
    }
}
