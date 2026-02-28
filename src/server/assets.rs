use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Redirect, Response};
use rust_embed::Embed;
use std::net::SocketAddr;
use std::sync::Arc;

use crate::auth::middleware;
use crate::server::AppState;

#[cfg(feature = "flutter")]
#[derive(Embed)]
#[folder = "flutter_client/build/web/"]
pub struct ClientAssets;

#[cfg(not(feature = "flutter"))]
#[derive(Embed)]
#[folder = "client/"]
pub struct ClientAssets;

/// Generate a CSRF token (random hex string)
fn generate_csrf_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 32] = rng.gen();
    hex::encode(bytes)
}

/// Inject CSRF meta tag into HTML content
fn inject_csrf_meta(html: &str) -> String {
    let token = generate_csrf_token();
    // Insert after <head> or at top of <head> section
    html.replace(
        "<head>",
        &format!("<head>\n  <meta name=\"csrf-token\" content=\"{}\">", token),
    )
}

/// Serve index.html with CSRF token injection.
fn serve_index_with_csrf() -> Option<Response> {
    ClientAssets::get("index.html").map(|file| {
        let html = String::from_utf8_lossy(&file.data).into_owned();
        let html = inject_csrf_meta(&html);
        Html(html).into_response()
    })
}

pub async fn index(
    State(app): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if middleware::require_auth(&app, &addr, &headers).is_err() {
        return Redirect::to("/login").into_response();
    }

    serve_index_with_csrf()
        .unwrap_or_else(|| Html("<h1>abot: client not found</h1>".to_string()).into_response())
}

pub async fn login() -> Response {
    #[cfg(feature = "flutter")]
    {
        // Flutter SPA handles login route internally
        serve_index_with_csrf().unwrap_or_else(|| StatusCode::NOT_FOUND.into_response())
    }
    #[cfg(not(feature = "flutter"))]
    {
        match ClientAssets::get("login.html") {
            Some(file) => Html(String::from_utf8_lossy(&file.data).into_owned()).into_response(),
            None => StatusCode::NOT_FOUND.into_response(),
        }
    }
}

/// Serve embedded assets at root paths (fallback handler)
/// Handles requests like /lib/foo.js, /vendor/xterm/xterm.esm.js, /design-tokens.css, etc.
pub async fn serve_asset_root(uri: Uri) -> Response {
    // Strip leading slash to match rust-embed paths
    let path = uri.path().trim_start_matches('/');

    if path.is_empty() {
        return StatusCode::NOT_FOUND.into_response();
    }

    // Block direct access to auth-protected HTML pages via fallback
    if path == "index.html" || path == "login.html" {
        return StatusCode::NOT_FOUND.into_response();
    }

    match ClientAssets::get(path) {
        Some(file) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();

            // Cache static assets for 1 day
            let cache_control = if path.starts_with("vendor/") || path.starts_with("lib/") {
                "public, max-age=86400"
            } else {
                "public, max-age=3600"
            };

            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, mime.to_string()),
                    (header::CACHE_CONTROL, cache_control.to_string()),
                ],
                file.data.to_vec(),
            )
                .into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
