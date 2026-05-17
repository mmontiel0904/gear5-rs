use crate::middleware::error::ApiError;
use crate::middleware::AdminAuth;
use crate::openapi::schemas::ErrorBody;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use gear5_core::auth::{self, NewKeyInput};
use gear5_core::model::ApiKey;
use gear5_core::scraper::run_once;
use serde::{Deserialize, Serialize};
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
