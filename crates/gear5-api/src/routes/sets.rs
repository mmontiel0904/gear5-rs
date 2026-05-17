use crate::middleware::error::ApiError;
use crate::middleware::ReadAuth;
use crate::openapi::schemas::ErrorBody;
use crate::state::AppState;
use axum::extract::State;
use axum::Json;
use gear5_core::model::CardSet;

#[utoipa::path(
    get,
    path = "/sets",
    tag = "catalog",
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "All known sets ordered by id", body = Vec<CardSet>),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 429, body = ErrorBody),
    ),
)]
pub async fn list_sets(
    State(s): State<AppState>,
    _: ReadAuth,
) -> Result<Json<Vec<CardSet>>, ApiError> {
    let rows: Vec<CardSet> = sqlx::query_as(
        r#"
        SELECT id, source_series, name, display_label, created_at, updated_at
        FROM sets
        ORDER BY id
        "#,
    )
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(rows))
}
