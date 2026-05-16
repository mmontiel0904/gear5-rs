use crate::auth_cache::AuthCache;
use gear5_core::config::Config;
use gear5_core::scraper::HttpClient;
use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Quota, RateLimiter};
use parking_lot::RwLock;
use sqlx::PgPool;
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::Arc;
use uuid::Uuid;

pub type DirectLimiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub cfg: Config,
    pub http: HttpClient,
    pub limiters: Arc<RwLock<HashMap<Uuid, Arc<DirectLimiter>>>>,
    pub scrape_lock: Arc<tokio::sync::Mutex<()>>,
    pub auth_cache: Arc<AuthCache>,
}

impl AppState {
    pub fn limiter_for(&self, id: Uuid, rpm: i32) -> Arc<DirectLimiter> {
        if let Some(found) = self.limiters.read().get(&id).cloned() {
            return found;
        }
        let mut w = self.limiters.write();
        if let Some(found) = w.get(&id).cloned() {
            return found;
        }
        let rpm = rpm.max(1) as u32;
        let quota = Quota::per_minute(NonZeroU32::new(rpm).unwrap_or(NonZeroU32::new(1).unwrap()));
        let limiter = Arc::new(RateLimiter::direct(quota));
        w.insert(id, limiter.clone());
        limiter
    }
}
