use axum::routing::{get, post};
use axum::Router;
use std::sync::Arc;

use super::assets;
use super::AppState;
use crate::auth::handlers as auth_handlers;
use crate::stream::handler as stream_handler;

pub fn build(state: Arc<AppState>) -> Router {
    Router::new()
        // Asset serving
        .route("/", get(assets::index))
        .route("/login", get(assets::login))
        .route("/assets/{*path}", get(assets::serve_asset))
        // Auth endpoints
        .route("/auth/status", get(auth_handlers::status))
        .route(
            "/auth/register/options",
            post(auth_handlers::register_options),
        )
        .route(
            "/auth/register/verify",
            post(auth_handlers::register_verify),
        )
        .route("/auth/login/options", post(auth_handlers::login_options))
        .route("/auth/login/verify", post(auth_handlers::login_verify))
        .route("/auth/logout", post(auth_handlers::logout))
        // Setup tokens
        .route("/auth/tokens", get(auth_handlers::list_tokens))
        .route("/auth/tokens", post(auth_handlers::create_token))
        .route("/auth/tokens/{id}", axum::routing::delete(auth_handlers::delete_token))
        // WebSocket stream
        .route("/stream", get(stream_handler::ws_upgrade))
        .with_state(state)
}
