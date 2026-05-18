use crate::middleware::error::ApiError;
use crate::middleware::AdminAuth;
use crate::openapi::schemas::ErrorBody;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use gear5_core::auth::{self, NewKeyInput};
use gear5_core::model::ApiKey;
use gear5_core::scraper::run_once;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use utoipa::ToSchema;

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateKeyBody {
    pub name: String,
    /// Defaults to `["read"]` when omitted. Valid values: `read`, `admin`.
    #[serde(default)]
    pub scopes: Vec<String>,
    /// Requests per minute allowed for this key. Defaults to 120 when omitted.
    #[serde(default)]
    pub rate_limit_rpm: Option<i32>,
    /// Optional RFC3339 expiry timestamp.
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CreatedKeyResponse {
    pub id: uuid::Uuid,
    pub name: String,
    pub prefix: String,
    pub scopes: Vec<String>,
    pub rate_limit_rpm: i32,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    /// Plaintext key. Returned ONCE; not retrievable afterwards.
    pub plaintext: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ApiKeyView {
    pub id: uuid::Uuid,
    pub name: String,
    pub prefix: String,
    pub scopes: Vec<String>,
    pub rate_limit_rpm: i32,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl From<ApiKey> for ApiKeyView {
    fn from(k: ApiKey) -> Self {
        Self {
            id: k.id,
            name: k.name,
            prefix: k.prefix,
            scopes: k.scopes,
            rate_limit_rpm: k.rate_limit_rpm,
            created_at: k.created_at,
            last_used_at: k.last_used_at,
            expires_at: k.expires_at,
            revoked_at: k.revoked_at,
        }
    }
}

#[utoipa::path(
    post,
    path = "/admin/keys",
    tag = "admin",
    security(("BearerAuth" = [])),
    request_body = CreateKeyBody,
    responses(
        (status = 201, description = "Key issued. Plaintext shown ONCE.", body = CreatedKeyResponse),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 429, body = ErrorBody),
    ),
)]
pub async fn create_key(
    State(s): State<AppState>,
    _: AdminAuth,
    Json(body): Json<CreateKeyBody>,
) -> Result<(StatusCode, Json<CreatedKeyResponse>), ApiError> {
    let scopes = if body.scopes.is_empty() {
        vec!["read".to_string()]
    } else {
        body.scopes
    };
    let rate = body.rate_limit_rpm.unwrap_or(120).max(1);
    let generated = auth::create_key(
        &s.pool,
        NewKeyInput {
            name: body.name,
            scopes,
            rate_limit_rpm: rate,
            expires_at: body.expires_at,
        },
    )
    .await?;
    let record = generated.record;
    let resp = CreatedKeyResponse {
        id: record.id,
        name: record.name,
        prefix: record.prefix,
        scopes: record.scopes,
        rate_limit_rpm: record.rate_limit_rpm,
        created_at: record.created_at,
        expires_at: record.expires_at,
        plaintext: generated.plaintext,
    };
    Ok((StatusCode::CREATED, Json(resp)))
}

#[utoipa::path(
    get,
    path = "/admin/keys",
    tag = "admin",
    security(("BearerAuth" = [])),
    responses(
        (status = 200, body = Vec<ApiKeyView>),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 429, body = ErrorBody),
    ),
)]
pub async fn list_keys(
    State(s): State<AppState>,
    _: AdminAuth,
) -> Result<Json<Vec<ApiKeyView>>, ApiError> {
    let rows = auth::list_keys(&s.pool).await?;
    Ok(Json(rows.into_iter().map(ApiKeyView::from).collect()))
}

#[utoipa::path(
    delete,
    path = "/admin/keys/{id}",
    tag = "admin",
    security(("BearerAuth" = [])),
    params(
        ("id" = String, Path, description = "UUID or 16-hex lookup prefix of the key to revoke"),
    ),
    responses(
        (status = 200, description = "Revoked; auth cache flushed", body = ApiKeyView),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 429, body = ErrorBody),
    ),
)]
pub async fn revoke_key(
    State(s): State<AppState>,
    _: AdminAuth,
    Path(id_or_prefix): Path<String>,
) -> Result<Json<ApiKeyView>, ApiError> {
    let row = auth::revoke_key(&s.pool, &id_or_prefix).await?;
    // Drop every cached auth entry so the revoked key (and any other entry that may
    // have been edited in the same admin session) cannot continue to be served from the
    // cache. Revocations are rare; the cost is one cheap LruCache::clear under a Mutex.
    s.auth_cache.invalidate_all();
    Ok(Json(ApiKeyView::from(row)))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ScrapeTriggerResponse {
    pub run_id: i64,
    pub status: String,
    pub sets_total: i32,
    pub sets_ok: i32,
    pub cards_seen: i32,
    pub cards_inserted: i32,
    pub cards_updated: i32,
}

#[utoipa::path(
    post,
    path = "/admin/scrape/run",
    tag = "admin",
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Scrape completed (possibly partial)", body = ScrapeTriggerResponse),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 429, body = ErrorBody),
        (status = 500, description = "Scrape failed before completion", body = ErrorBody),
    ),
)]
pub async fn trigger_scrape(
    State(s): State<AppState>,
    _: AdminAuth,
) -> Result<Json<ScrapeTriggerResponse>, ApiError> {
    let _guard = s.scrape_lock.lock().await;
    let report = run_once(&s.pool, &s.http, &s.cfg.scrape, &s.cfg.images.dir).await?;
    Ok(Json(ScrapeTriggerResponse {
        run_id: report.run_id,
        status: report.status,
        sets_total: report.sets_total,
        sets_ok: report.sets_ok,
        cards_seen: report.cards_seen,
        cards_inserted: report.cards_inserted,
        cards_updated: report.cards_updated,
    }))
}

// ---------------------------------------------------------------------------
// Run history
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ListRunsQuery {
    /// Maximum number of runs to return. Clamped to 100. Defaults to 20.
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    20
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ScrapeRunView {
    pub id: i64,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    /// Duration in seconds. None when the run has not finished yet.
    pub duration_secs: Option<i64>,
    pub status: String,
    pub sets_total: Option<i32>,
    pub sets_ok: Option<i32>,
    pub cards_seen: Option<i32>,
    pub cards_inserted: Option<i32>,
    pub cards_updated: Option<i32>,
    pub cards_unchanged: i32,
    pub error: Option<String>,
}

#[utoipa::path(
    get,
    path = "/admin/scrape/runs",
    tag = "admin",
    security(("BearerAuth" = [])),
    params(
        ("limit" = Option<i64>, Query, description = "Max rows to return (default 20, max 100)"),
    ),
    responses(
        (status = 200, body = Vec<ScrapeRunView>),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
    ),
)]
pub async fn list_runs(
    State(s): State<AppState>,
    _: AdminAuth,
    Query(q): Query<ListRunsQuery>,
) -> Result<Json<Vec<ScrapeRunView>>, ApiError> {
    let limit = q.limit.clamp(1, 100);
    let rows = sqlx::query(
        r#"
        SELECT id, started_at, finished_at, status,
               sets_total, sets_ok,
               cards_seen, cards_inserted, cards_updated, cards_unchanged,
               error
        FROM scrape_runs
        ORDER BY id DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(&s.pool)
    .await?;

    let runs: Vec<ScrapeRunView> = rows
        .iter()
        .map(|r| {
            let started_at: DateTime<Utc> = r.try_get("started_at").unwrap();
            let finished_at: Option<DateTime<Utc>> = r.try_get("finished_at").unwrap_or(None);
            let duration_secs = finished_at
                .map(|f| f.signed_duration_since(started_at).num_seconds());
            ScrapeRunView {
                id: r.try_get("id").unwrap(),
                started_at,
                finished_at,
                duration_secs,
                status: r.try_get("status").unwrap(),
                sets_total: r.try_get("sets_total").unwrap_or(None),
                sets_ok: r.try_get("sets_ok").unwrap_or(None),
                cards_seen: r.try_get("cards_seen").unwrap_or(None),
                cards_inserted: r.try_get("cards_inserted").unwrap_or(None),
                cards_updated: r.try_get("cards_updated").unwrap_or(None),
                cards_unchanged: r.try_get("cards_unchanged").unwrap_or(0),
                error: r.try_get("error").unwrap_or(None),
            }
        })
        .collect();

    Ok(Json(runs))
}

// ---------------------------------------------------------------------------
// Active run progress
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, ToSchema)]
pub struct RunSetProgress {
    pub source_series: String,
    pub set_id: Option<String>,
    pub status: String,
    pub cards_seen: i32,
    pub cards_unchanged: i32,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    /// Duration in seconds for finished sets; elapsed seconds for running sets.
    pub duration_secs: i64,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ActiveRunResponse {
    pub run_id: i64,
    pub started_at: DateTime<Utc>,
    /// Elapsed seconds since the run started.
    pub age_secs: i64,
    pub sets_total: i32,
    pub sets_finished: i32,
    pub sets_ok: i32,
    pub sets_failed: i32,
    pub cards_seen: i32,
    pub cards_inserted: i32,
    pub cards_updated: i32,
    pub cards_unchanged: i32,
    pub sets: Vec<RunSetProgress>,
}

#[utoipa::path(
    get,
    path = "/admin/scrape/runs/active",
    tag = "admin",
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Active run with per-set progress", body = ActiveRunResponse),
        (status = 404, description = "No run is currently in progress"),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
    ),
)]
pub async fn active_run(
    State(s): State<AppState>,
    _: AdminAuth,
) -> Result<(StatusCode, Json<ActiveRunResponse>), ApiError> {
    // Find the most recent run that is still running.
    let run_row = sqlx::query(
        r#"
        SELECT id, started_at
        FROM scrape_runs
        WHERE status = 'running'
        ORDER BY id DESC
        LIMIT 1
        "#,
    )
    .fetch_optional(&s.pool)
    .await?;

    let run_row = match run_row {
        Some(r) => r,
        None => return Err(ApiError::not_found()),
    };

    let run_id: i64 = run_row.try_get("id")?;
    let started_at: DateTime<Utc> = run_row.try_get("started_at")?;
    let now = Utc::now();
    let age_secs = now.signed_duration_since(started_at).num_seconds();

    // Fetch per-set rows for this run.
    let set_rows = sqlx::query(
        r#"
        SELECT source_series, set_id, status,
               cards_seen, cards_unchanged,
               started_at, finished_at, error
        FROM scrape_run_sets
        WHERE run_id = $1
        ORDER BY started_at ASC
        "#,
    )
    .bind(run_id)
    .fetch_all(&s.pool)
    .await?;

    let mut sets_finished = 0i32;
    let mut sets_ok = 0i32;
    let mut sets_failed = 0i32;
    let mut cards_seen = 0i32;
    let mut cards_unchanged = 0i32;

    let sets: Vec<RunSetProgress> = set_rows
        .iter()
        .map(|r| {
            let set_started: DateTime<Utc> = r.try_get("started_at").unwrap();
            let set_finished: Option<DateTime<Utc>> = r.try_get("finished_at").unwrap_or(None);
            let status: String = r.try_get("status").unwrap();
            let cs: i32 = r.try_get("cards_seen").unwrap_or(0);
            let cu: i32 = r.try_get("cards_unchanged").unwrap_or(0);

            let duration_secs = match set_finished {
                Some(f) => f.signed_duration_since(set_started).num_seconds(),
                None => now.signed_duration_since(set_started).num_seconds(),
            };

            if status != "running" {
                sets_finished += 1;
            }
            if status == "ok" {
                sets_ok += 1;
            }
            if status == "failed" {
                sets_failed += 1;
            }
            cards_seen += cs;
            cards_unchanged += cu;

            RunSetProgress {
                source_series: r.try_get("source_series").unwrap(),
                set_id: r.try_get("set_id").unwrap_or(None),
                status,
                cards_seen: cs,
                cards_unchanged: cu,
                started_at: set_started,
                finished_at: set_finished,
                duration_secs,
                error: r.try_get("error").unwrap_or(None),
            }
        })
        .collect();

    // Derive inserted/updated from the parent run row (only partially populated while running,
    // but gives a live rolling count since close_run hasn't been called yet).
    // We read them fresh from scrape_runs to get whatever partial state was written.
    let run_counters = sqlx::query(
        r#"
        SELECT COALESCE(cards_inserted, 0)  AS cards_inserted,
               COALESCE(cards_updated, 0)   AS cards_updated
        FROM scrape_runs
        WHERE id = $1
        "#,
    )
    .bind(run_id)
    .fetch_one(&s.pool)
    .await?;

    let sets_total = sets.len() as i32;

    Ok((
        StatusCode::OK,
        Json(ActiveRunResponse {
            run_id,
            started_at,
            age_secs,
            sets_total,
            sets_finished,
            sets_ok,
            sets_failed,
            cards_seen,
            cards_inserted: run_counters.try_get("cards_inserted").unwrap_or(0),
            cards_updated: run_counters.try_get("cards_updated").unwrap_or(0),
            cards_unchanged,
            sets,
        }),
    ))
}
