use lru::LruCache;
use parking_lot::Mutex;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// LRU cache for autocomplete query results, keyed by the normalized query string
/// plus the requested result limit. Values are wrapped in `Arc` so callers can
/// hand back a cheap clone without touching the cache again.
///
/// Staleness window for catalog updates is bounded by `ttl`. The scraper runs at
/// most once a day, so a short TTL is enough to keep results fresh without
/// thrashing the cache.
pub struct SearchCache<V> {
    inner: Mutex<LruCache<CacheKey, Entry<V>>>,
    ttl: Duration,
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct CacheKey {
    needle: String,
    limit: i64,
}

struct Entry<V> {
    value: Arc<V>,
    cached_at: Instant,
}

impl<V> Entry<V> {
    fn new(value: Arc<V>) -> Self {
        Self {
            value,
            cached_at: Instant::now(),
        }
    }
}

impl<V> SearchCache<V> {
    pub fn new(capacity: NonZeroUsize, ttl: Duration) -> Self {
        Self {
            inner: Mutex::new(LruCache::new(capacity)),
            ttl,
        }
    }

    pub fn get(&self, needle: &str, limit: i64) -> Option<Arc<V>> {
        let key = CacheKey {
            needle: needle.to_string(),
            limit,
        };
        let mut guard = self.inner.lock();
        let entry = guard.get(&key)?;
        if entry.cached_at.elapsed() > self.ttl {
            guard.pop(&key);
            return None;
        }
        Some(entry.value.clone())
    }

    pub fn insert(&self, needle: &str, limit: i64, value: Arc<V>) {
        let key = CacheKey {
            needle: needle.to_string(),
            limit,
        };
        self.inner.lock().put(key, Entry::new(value));
    }

    #[allow(dead_code)]
    pub fn invalidate_all(&self) {
        self.inner.lock().clear();
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }
}
