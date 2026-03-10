//! Download endpoints for kubos and abots as compressed archives.

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::header;
use axum::response::Response;
use std::sync::Arc;

use crate::auth::middleware::Authenticated;
use crate::error::AppError;
use crate::server::AppState;

/// GET /kubos/:name/download — download a kubo as a .tar.gz archive
pub async fn download_kubo(
    _auth: Authenticated,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Response, AppError> {
    let kubos_dir = app.data_dir.join("kubos");
    let kubo_dir = kubos_dir.join(format!("{}.kubo", name));

    if !kubo_dir.is_dir() {
        return Err(AppError::NotFound(format!("kubo '{}' not found", name)));
    }

    let archive_bytes = create_tar_gz(&kubo_dir, &format!("{}.kubo", name)).await?;
    let filename = format!("{}.kubo.tar.gz", name);

    Ok(archive_response(archive_bytes, &filename))
}

/// GET /abots/:name/download — download an abot as a .tar.gz archive
pub async fn download_abot(
    _auth: Authenticated,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Response, AppError> {
    let abots_dir = app.data_dir.join("abots");
    let abot_dir = abots_dir.join(format!("{}.abot", name));

    if !abot_dir.is_dir() {
        return Err(AppError::NotFound(format!("abot '{}' not found", name)));
    }

    let archive_bytes = create_tar_gz(&abot_dir, &format!("{}.abot", name)).await?;
    let filename = format!("{}.abot.tar.gz", name);

    Ok(archive_response(archive_bytes, &filename))
}

/// Create a tar.gz archive from a directory, using a blocking task.
async fn create_tar_gz(
    source_dir: &std::path::Path,
    archive_name: &str,
) -> Result<Vec<u8>, AppError> {
    let source = source_dir.to_path_buf();
    let name = archive_name.to_string();

    tokio::task::spawn_blocking(move || {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use tar::Builder;

        let buf = Vec::new();
        let encoder = GzEncoder::new(buf, Compression::default());
        let mut archive = Builder::new(encoder);

        archive
            .append_dir_all(&name, &source)
            .map_err(|e| AppError::Internal(format!("tar error: {}", e)))?;

        let encoder = archive
            .into_inner()
            .map_err(|e| AppError::Internal(format!("tar finalize error: {}", e)))?;

        encoder
            .finish()
            .map_err(|e| AppError::Internal(format!("gzip error: {}", e)))
    })
    .await
    .map_err(|e| AppError::Internal(format!("task join error: {}", e)))?
}

/// Build an HTTP response with the archive as a downloadable attachment.
fn archive_response(bytes: Vec<u8>, filename: &str) -> Response {
    Response::builder()
        .header(header::CONTENT_TYPE, "application/gzip")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename),
        )
        .body(Body::from(bytes))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_tar_gz_produces_valid_archive() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("test.abot");
        std::fs::create_dir_all(dir.join("home")).unwrap();
        std::fs::write(dir.join("manifest.json"), r#"{"name":"test"}"#).unwrap();
        std::fs::write(dir.join("home/hello.txt"), "hello world").unwrap();

        let bytes = create_tar_gz(&dir, "test.abot").await.unwrap();

        // Decompress and verify contents
        let decoder = flate2::read::GzDecoder::new(&bytes[..]);
        let mut archive = tar::Archive::new(decoder);

        let entries: Vec<String> = archive
            .entries()
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path().unwrap().to_string_lossy().to_string())
            .collect();

        assert!(entries.iter().any(|e| e.contains("manifest.json")));
        assert!(entries.iter().any(|e| e.contains("home/hello.txt")));
    }

    #[tokio::test]
    async fn test_create_tar_gz_nonexistent_dir_fails() {
        let result = create_tar_gz(std::path::Path::new("/nonexistent"), "test").await;
        assert!(result.is_err());
    }

    #[test]
    fn test_archive_response_headers() {
        let resp = archive_response(vec![1, 2, 3], "test.tar.gz");
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/gzip"
        );
        assert!(resp
            .headers()
            .get(header::CONTENT_DISPOSITION)
            .unwrap()
            .to_str()
            .unwrap()
            .contains("test.tar.gz"));
    }
}
