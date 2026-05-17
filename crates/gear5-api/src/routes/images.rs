use crate::middleware::error::ApiError;
use crate::middleware::ReadAuth;
use crate::openapi::schemas::ErrorBody;
use crate::state::AppState;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::Response;
use tokio::fs;
use tokio_util::io::ReaderStream;

#[utoipa::path(
    get,
    path = "/images/{file}",
    tag = "images",
    security(("BearerAuth" = [])),
    params(
        ("file" = String, Path, description = "Versioned image filename, e.g. `OP01-001.260508.png`"),
    ),
    responses(
        (
            status = 200,
            description = "Card art (PNG bytes)",
            content_type = "image/png",
            body = inline(Vec<u8>),
        ),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 429, body = ErrorBody),
    ),
)]
pub async fn serve_image(
    State(s): State<AppState>,
    _: ReadAuth,
    Path(file): Path<String>,
) -> Result<Response, ApiError> {
    if file.is_empty() || file.contains('/') || file.contains('\\') || file.contains("..") {
        return Err(ApiError::not_found());
    }
    let path = s.cfg.images.dir.join(&file);
    let canonical_root = s
        .cfg
        .images
        .dir
        .canonicalize()
        .map_err(|e| ApiError::internal(format!("image dir: {e}")))?;
    let canonical_path = path.canonicalize().map_err(|_| ApiError::not_found())?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(ApiError::not_found());
    }

    let f = fs::File::open(&canonical_path)
        .await
        .map_err(|_| ApiError::not_found())?;
    let stream = ReaderStream::new(f);
    let body = Body::from_stream(stream);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "image/png")
        .header(header::CACHE_CONTROL, "public, max-age=86400, immutable")
        .body(body)
        .map_err(|e| ApiError::internal(e.to_string()))
}
