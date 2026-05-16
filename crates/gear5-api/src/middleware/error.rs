use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    pub fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "invalid or missing api key")
    }

    pub fn forbidden() -> Self {
        Self::new(StatusCode::FORBIDDEN, "missing required scope")
    }

    pub fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "not found")
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(json!({ "error": self.message }));
        let mut resp = (self.status, body).into_response();
        if self.status == StatusCode::TOO_MANY_REQUESTS {
            resp.headers_mut()
                .insert("retry-after", axum::http::HeaderValue::from_static("60"));
        }
        resp
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::RowNotFound => ApiError::not_found(),
            other => {
                tracing::error!(error = %other, "sqlx error");
                ApiError::internal("database error")
            }
        }
    }
}

impl From<gear5_core::Error> for ApiError {
    fn from(e: gear5_core::Error) -> Self {
        use gear5_core::Error::*;
        match e {
            NotFound => ApiError::not_found(),
            InvalidApiKey => ApiError::unauthorized(),
            other => {
                tracing::error!(error = %other, "core error");
                ApiError::internal("internal error")
            }
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        tracing::error!(error = %e, "anyhow error");
        ApiError::internal(e.to_string())
    }
}
