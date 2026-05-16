use crate::auth_cache::AuthCacheEntry;
use crate::middleware::error::ApiError;
use crate::state::AppState;
use axum::async_trait;
use axum::extract::{FromRef, FromRequestParts};
use axum::http::{request::Parts, StatusCode};
use gear5_core::auth::{self, Scope};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Clone, Debug)]
#[allow(dead_code)] // `name` is surfaced for future audit-log use.
pub struct AuthContext {
    pub id: Uuid,
    pub name: String,
    pub scopes: Vec<String>,
    pub rate_limit_rpm: i32,
}

impl AuthContext {
    fn from_cache(entry: AuthCacheEntry) -> Self {
        Self {
            id: entry.id,
            name: entry.name,
            scopes: entry.scopes,
            rate_limit_rpm: entry.rate_limit_rpm,
        }
    }
}

fn extract_bearer(parts: &Parts) -> Result<String, ApiError> {
    parts
        .headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(ApiError::unauthorized)
}

fn enforce_rate_limit(app: &AppState, ctx: &AuthContext) -> Result<(), ApiError> {
    let limiter = app.limiter_for(ctx.id, ctx.rate_limit_rpm);
    if limiter.check().is_err() {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "rate limit exceeded",
        ));
    }
    Ok(())
}

fn touch_last_used(app: &AppState, id: Uuid) {
    let pool = app.pool.clone();
    tokio::spawn(async move {
        if let Err(e) = auth::mark_used(&pool, id).await {
            tracing::debug!(error = %e, "mark_used failed");
        }
    });
}

async fn resolve_auth(app: &AppState, token: &str) -> Result<AuthContext, ApiError> {
    let token_hash: [u8; 32] = Sha256::digest(token.as_bytes()).into();

    // Fast path: warm cache hit. Skips argon2id entirely.
    if let Some(entry) = app.auth_cache.get(&token_hash) {
        return Ok(AuthContext::from_cache(entry));
    }

    // Slow path: indexed DB lookup, then argon2id verify on a blocking thread so we
    // never starve the tokio scheduler with CPU-bound work.
    let row = auth::lookup_active_row_by_prefix(&app.pool, token)
        .await
        .map_err(|_| ApiError::unauthorized())?;

    let phc = row.hash.clone();
    let token_for_verify = token.to_string();
    let ok = tokio::task::spawn_blocking(move || auth::verify_secret(&token_for_verify, &phc))
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "argon2 verify task panicked");
            ApiError::internal("verify task panicked")
        })?;
    if !ok {
        return Err(ApiError::unauthorized());
    }

    let entry = AuthCacheEntry::new(
        row.id,
        row.name.clone(),
        row.scopes.clone(),
        row.rate_limit_rpm,
        row.expires_at,
    );
    app.auth_cache.insert(token_hash, entry.clone());

    Ok(AuthContext::from_cache(entry))
}

#[async_trait]
impl<S> FromRequestParts<S> for AuthContext
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app = AppState::from_ref(state);
        let token = extract_bearer(parts)?;
        let ctx = resolve_auth(&app, &token).await?;
        enforce_rate_limit(&app, &ctx)?;
        touch_last_used(&app, ctx.id);
        Ok(ctx)
    }
}

#[allow(dead_code)] // wrapped AuthContext available to future routes (audit logs, etc.).
pub struct ReadAuth(pub AuthContext);
#[allow(dead_code)]
pub struct AdminAuth(pub AuthContext);

#[async_trait]
impl<S> FromRequestParts<S> for ReadAuth
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let ctx = AuthContext::from_request_parts(parts, state).await?;
        if !auth::has_scope(&ctx.scopes, Scope::Read) && !auth::has_scope(&ctx.scopes, Scope::Admin)
        {
            return Err(ApiError::forbidden());
        }
        Ok(ReadAuth(ctx))
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for AdminAuth
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let ctx = AuthContext::from_request_parts(parts, state).await?;
        if !auth::has_scope(&ctx.scopes, Scope::Admin) {
            return Err(ApiError::forbidden());
        }
        Ok(AdminAuth(ctx))
    }
}
