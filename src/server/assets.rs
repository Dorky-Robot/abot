use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "client/"]
pub struct ClientAssets;

pub async fn index() -> Html<String> {
    match ClientAssets::get("index.html") {
        Some(file) => Html(String::from_utf8_lossy(&file.data).to_string()),
        None => Html("<h1>abot: client not found</h1>".to_string()),
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
