use crate::middleware::error::ApiError;
use crate::middleware::ReadAuth;
use crate::state::AppState;
use axum::extract::State;
use axum::Json;
use gear5_core::model::CardSet;

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
