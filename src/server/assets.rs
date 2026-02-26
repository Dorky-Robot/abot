use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use rust_embed::Embed;
use std::net::SocketAddr;
use std::sync::Arc;

use crate::auth::middleware;
use crate::server::AppState;

#[derive(Embed)]
#[folder = "client/"]
pub struct ClientAssets;

pub async fn index(
    State(app): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if middleware::require_auth(&app, &addr, &headers).is_err() {
        return Redirect::to("/login").into_response();
    }

    match ClientAssets::get("index.html") {
        Some(file) => Html(String::from_utf8_lossy(&file.data).into_owned()).into_response(),
        None => Html("<h1>abot: client not found</h1>".to_string()).into_response(),
    }
}

pub async fn login() -> Response {
    match ClientAssets::get("login.html") {
        Some(file) => Html(String::from_utf8_lossy(&file.data).into_owned()).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

pub async fn serve_asset(
    axum::extract::Path(path): axum::extract::Path<String>,
) -> Response {
    match ClientAssets::get(&path) {
        Some(file) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime.to_string())],
                file.data.to_vec(),
            )
                .into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
