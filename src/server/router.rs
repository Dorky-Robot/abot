use axum::routing::{delete, get, post, put};
use axum::Router;
use std::sync::Arc;

use super::abots;
use super::anthropic_oauth;
use super::assets;
use super::browse;
use super::config as config_routes;
use super::kubos;
use super::sessions;
use super::shortcuts;
use super::AppState;
use crate::auth::handlers as auth_handlers;
use crate::stream::handler as stream_handler;

pub fn build(state: Arc<AppState>) -> Router {
    Router::new()
        // Pages
        .route("/", get(assets::index))
        .route("/login", get(assets::login))
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
        .route("/auth/tokens/{id}", delete(auth_handlers::delete_token))
        // WebSocket stream
        .route("/stream", get(stream_handler::ws_upgrade))
        // Session CRUD
        .route("/sessions", get(sessions::list_sessions))
        .route("/sessions", post(sessions::create_session))
        .route("/sessions/{name}", get(sessions::get_session))
        .route("/sessions/{name}", put(sessions::rename_session))
        .route("/sessions/{name}", delete(sessions::delete_session))
        // Per-session credentials
        .route(
            "/sessions/{name}/credentials",
            post(sessions::set_session_credentials),
        )
        .route(
            "/sessions/{name}/credentials/status",
            get(sessions::session_credentials_status),
        )
        .route(
            "/sessions/{name}/credentials",
            delete(sessions::delete_session_credentials),
        )
        // Session open/save/close
        .route("/sessions/open", post(sessions::open_bundle))
        .route("/sessions/{name}/save", post(sessions::save_session))
        .route("/sessions/{name}/save-as", post(sessions::save_session_as))
        .route("/sessions/{name}/close", post(sessions::close_session))
        // Abots (known abots registry)
        .route("/abots", get(abots::list_abots))
        .route("/abots/{name}", get(abots::get_abot))
        .route("/abots/{name}", delete(abots::remove_abot))
        .route("/abots/{name}/dismiss", post(abots::dismiss_variant))
        .route("/abots/{name}/integrate", post(abots::integrate_variant))
        .route("/abots/{name}/discard", post(abots::discard_variant))
        // Kubos
        .route("/kubos", get(kubos::list_kubos))
        .route("/kubos", post(kubos::create_kubo))
        .route("/kubos/open", post(kubos::open_kubo))
        .route("/kubos/{name}/start", post(kubos::start_kubo))
        .route("/kubos/{name}/stop", post(kubos::stop_kubo))
        .route("/kubos/{name}/abots", post(kubos::add_abot_to_kubo))
        .route(
            "/kubos/{name}/abots/{abot}",
            delete(kubos::remove_abot_from_kubo),
        )
        // Shortcuts
        .route("/shortcuts", get(shortcuts::get_shortcuts))
        .route("/shortcuts", put(shortcuts::set_shortcuts))
        // Health
        .route(
            "/health",
            get(|| async { axum::Json(serde_json::json!({"ok": true})) }),
        )
        // File browser
        .route("/api/browse", get(browse::browse_dir))
        .route("/api/pick-directory", post(browse::pick_directory))
        .route("/api/pick-file", post(browse::pick_file))
        .route("/api/pick-save-location", post(browse::pick_save_location))
        // Config
        .route("/api/config", get(config_routes::get_config))
        .route(
            "/api/config/instance-name",
            put(config_routes::set_instance_name),
        )
        .route("/api/config/bundle-dir", put(config_routes::set_bundle_dir))
        .route("/api/config/kubos-dir", put(config_routes::set_kubos_dir))
        // Token/credential API aliases (legacy client uses /api/tokens, /api/credentials)
        .route("/api/tokens", get(auth_handlers::list_tokens))
        .route("/api/credentials", get(auth_handlers::list_credentials))
        .route(
            "/api/credentials/{id}",
            delete(auth_handlers::delete_credential),
        )
        // Anthropic API key
        .route("/api/anthropic/key", post(anthropic_oauth::save_key))
        .route(
            "/api/anthropic/key/status",
            get(anthropic_oauth::key_status),
        )
        .route("/api/anthropic/key", delete(anthropic_oauth::delete_key))
        // Stub endpoint for client compatibility
        .route(
            "/connect/info",
            get(|| async { axum::Json(serde_json::json!({})) }),
        )
        // Fallback: serve embedded assets at root paths (e.g. /lib/..., /vendor/...)
        .fallback(get(assets::serve_asset_root))
        .with_state(state)
}
