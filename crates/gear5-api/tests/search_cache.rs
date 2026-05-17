use std::num::NonZeroUsize;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[path = "../src/search_cache.rs"]
#[allow(dead_code)]
mod search_cache;

use search_cache::SearchCache;

type V = Vec<String>;

fn make(items: &[&str]) -> Arc<V> {
    Arc::new(items.iter().map(|s| s.to_string()).collect())
}

#[test]
fn cache_returns_inserted_entry() {
    let cache: SearchCache<V> =
        SearchCache::new(NonZeroUsize::new(8).unwrap(), Duration::from_secs(60));
    cache.insert("lu", 10, make(&["Luffy"]));
    let got = cache.get("lu", 10).expect("hit");
    assert_eq!(*got, vec!["Luffy".to_string()]);
}

#[test]
fn cache_miss_on_different_key() {
    let cache: SearchCache<V> =
        SearchCache::new(NonZeroUsize::new(8).unwrap(), Duration::from_secs(60));
    cache.insert("lu", 10, make(&["Luffy"]));
    assert!(cache.get("lu", 5).is_none(), "limit is part of the key");
    assert!(cache.get("lo", 10).is_none(), "needle is part of the key");
}

#[test]
fn cache_expires_after_ttl() {
    let cache: SearchCache<V> =
        SearchCache::new(NonZeroUsize::new(8).unwrap(), Duration::from_millis(50));
    cache.insert("lu", 10, make(&["Luffy"]));
    assert!(cache.get("lu", 10).is_some());
    thread::sleep(Duration::from_millis(120));
    assert!(cache.get("lu", 10).is_none(), "entry should have expired");
}

#[test]
fn cache_respects_lru_capacity() {
    let cache: SearchCache<V> =
        SearchCache::new(NonZeroUsize::new(2).unwrap(), Duration::from_secs(60));
    cache.insert("a", 10, make(&["A"]));
    cache.insert("b", 10, make(&["B"]));
    cache.insert("c", 10, make(&["C"]));
    assert!(cache.get("a", 10).is_none(), "LRU should evict oldest");
    assert!(cache.get("b", 10).is_some());
    assert!(cache.get("c", 10).is_some());
}

#[test]
fn cache_invalidate_all_clears_entries() {
    let cache: SearchCache<V> =
        SearchCache::new(NonZeroUsize::new(8).unwrap(), Duration::from_secs(60));
    cache.insert("a", 10, make(&["A"]));
    cache.insert("b", 10, make(&["B"]));
    assert_eq!(cache.len(), 2);
    cache.invalidate_all();
    assert_eq!(cache.len(), 0);
    assert!(cache.get("a", 10).is_none());
}
