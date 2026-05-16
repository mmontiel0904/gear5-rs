use crate::middleware::error::ApiError;
use crate::state::AppState;
use axum::async_trait;
use axum::extract::{FromRef, FromRequestParts};
use axum::http::{request::Parts, StatusCode};
use gear5_core::auth::{self, Scope};
use gear5_core::model::ApiKey;
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
    fn from_row(row: ApiKey) -> Self {
        Self {
            id: row.id,
            name: row.name,
            scopes: row.scopes,
            rate_limit_rpm: row.rate_limit_rpm,
        }
    }
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
        let token = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .ok_or_else(ApiError::unauthorized)?
            .trim()
            .to_string();

        let row = auth::lookup_for_verify(&app.pool, &token)
            .await
            .map_err(|_| ApiError::unauthorized())?;

        let ctx = AuthContext::from_row(row);

        // Per-key rate limit.
        let limiter = app.limiter_for(ctx.id, ctx.rate_limit_rpm);
        if limiter.check().is_err() {
            return Err(ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "rate limit exceeded",
            ));
        }

        // Fire-and-forget last_used_at touch.
        {
            let pool = app.pool.clone();
            let id = ctx.id;
            tokio::spawn(async move {
                if let Err(e) = auth::mark_used(&pool, id).await {
                    tracing::debug!(error = %e, "mark_used failed");
                }
            });
        }

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
