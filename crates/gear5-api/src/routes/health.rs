use crate::middleware::error::ApiError;
use crate::openapi::schemas::ErrorBody;
use crate::state::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use sqlx::Row;
use utoipa::ToSchema;

#[utoipa::path(
    get,
    path = "/health",
    tag = "health",
    responses(
        (status = 200, description = "Server is up", body = String),
    ),
)]
pub async fn liveness() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ScrapeHealth {
    pub last_run_id: Option<i64>,
    pub last_status: Option<String>,
    pub last_started_at: Option<DateTime<Utc>>,
    pub last_finished_at: Option<DateTime<Utc>>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub consecutive_failures: i64,
    pub stale: bool,
}

#[utoipa::path(
    get,
    path = "/health/scrape",
    tag = "health",
    responses(
        (status = 200, description = "Scrape pipeline healthy", body = ScrapeHealth),
        (status = 503, description = "Scrape stale (>stale_after_hours) or >=3 consecutive failures", body = ScrapeHealth),
        (status = 500, body = ErrorBody),
    ),
)]
pub async fn scrape_health(State(s): State<AppState>) -> Result<Response, ApiError> {
    let row = sqlx::query(
        r#"
        SELECT id, status, started_at, finished_at, error
        FROM scrape_runs
        ORDER BY id DESC
        LIMIT 1
        "#,
    )
    .fetch_optional(&s.pool)
    .await?;

    let last_success_at: Option<DateTime<Utc>> = sqlx::query(
        r#"
        SELECT finished_at
        FROM scrape_runs
        WHERE status = 'success'
        ORDER BY id DESC
        LIMIT 1
        "#,
    )
    .fetch_optional(&s.pool)
    .await?
    .and_then(|r| {
        r.try_get::<Option<DateTime<Utc>>, _>("finished_at")
            .ok()
            .flatten()
    });

    let consecutive_failures: i64 = sqlx::query(
        r#"
        WITH ordered AS (
            SELECT status, ROW_NUMBER() OVER (ORDER BY id DESC) AS rn
            FROM scrape_runs
            WHERE status IN ('success','partial','failed')
        ),
        run AS (
            SELECT rn FROM ordered WHERE status IN ('success','partial')
            ORDER BY rn LIMIT 1
        )
        SELECT COALESCE((SELECT rn - 1 FROM run), (SELECT count(*) FROM ordered)) AS streak
        "#,
    )
    .fetch_one(&s.pool)
    .await?
    .try_get::<Option<i64>, _>("streak")?
    .unwrap_or(0);

    let mut health = ScrapeHealth {
        last_run_id: None,
        last_status: None,
        last_started_at: None,
        last_finished_at: None,
        last_success_at,
        last_error: None,
        consecutive_failures,
        stale: false,
    };
    if let Some(r) = row {
        health.last_run_id = Some(r.try_get("id")?);
        health.last_status = Some(r.try_get("status")?);
        health.last_started_at = Some(r.try_get("started_at")?);
        health.last_finished_at = r.try_get("finished_at")?;
        health.last_error = r.try_get("error")?;
    }

    let stale_after = Duration::hours(s.cfg.scrape.stale_after_hours);
    health.stale = match health.last_success_at {
        Some(t) => Utc::now().signed_duration_since(t) > stale_after,
        None => true,
    };

    let bad = health.stale || health.consecutive_failures >= 3;
    let status = if bad {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };
    Ok((status, Json(health)).into_response())
}
