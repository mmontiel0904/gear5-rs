use chrono::{DateTime, Utc};
use lru::LruCache;
use parking_lot::Mutex;
use std::num::NonZeroUsize;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Snapshot of the API-key row we care about for authorising a request.
/// Anything that can change in the DB while the cache is hot (revocation, scope edit) is
/// bounded by `AuthCache::ttl`.
#[derive(Debug, Clone)]
pub struct AuthCacheEntry {
    pub id: Uuid,
    pub name: String,
    pub scopes: Vec<String>,
    pub rate_limit_rpm: i32,
    pub key_expires_at: Option<DateTime<Utc>>,
    cached_at: Instant,
}

impl AuthCacheEntry {
    pub fn new(
        id: Uuid,
        name: String,
        scopes: Vec<String>,
        rate_limit_rpm: i32,
        key_expires_at: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            id,
            name,
            scopes,
            rate_limit_rpm,
            key_expires_at,
            cached_at: Instant::now(),
        }
    }
}

/// LRU cache keyed by `sha256(plaintext token)` (the full hash, not the DB lookup prefix).
/// Cache hits skip the argon2id verification path entirely.
///
/// Staleness window for revocation/rotation is bounded by `ttl`. CLI-initiated revokes do not
/// propagate to a running API process; they take effect within one TTL window.
pub struct AuthCache {
    inner: Mutex<LruCache<[u8; 32], AuthCacheEntry>>,
    ttl: Duration,
}

impl AuthCache {
    pub fn new(capacity: NonZeroUsize, ttl: Duration) -> Self {
        Self {
            inner: Mutex::new(LruCache::new(capacity)),
            ttl,
        }
    }

    #[allow(dead_code)]
    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    /// Returns the cached entry only if it is within TTL and the key has not expired.
    pub fn get(&self, token_hash: &[u8; 32]) -> Option<AuthCacheEntry> {
        let mut guard = self.inner.lock();
        let entry = guard.get(token_hash)?.clone();
        if entry.cached_at.elapsed() > self.ttl {
            guard.pop(token_hash);
            return None;
        }
        if let Some(exp) = entry.key_expires_at {
            if exp <= Utc::now() {
                guard.pop(token_hash);
                return None;
            }
        }
        Some(entry)
    }

    pub fn insert(&self, token_hash: [u8; 32], entry: AuthCacheEntry) {
        self.inner.lock().put(token_hash, entry);
    }

    #[allow(dead_code)]
    pub fn invalidate(&self, token_hash: &[u8; 32]) {
        self.inner.lock().pop(token_hash);
    }

    pub fn invalidate_all(&self) {
        self.inner.lock().clear();
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }
}
