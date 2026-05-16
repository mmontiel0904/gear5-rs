// Integration tests for AuthCache. Pulls the binary crate's auth_cache module via the
// `include!` pattern would not work; instead, the test depends on the binary's public
// surface only through this test target compiled against the same source tree.

use std::num::NonZeroUsize;
use std::thread;
use std::time::Duration;

// Re-declare the module under test by including the source. The binary crate does not
// expose its modules to integration tests, so this is the lightest way to exercise the
// cache without spinning up a real server.
#[path = "../src/auth_cache.rs"]
#[allow(dead_code)]
mod auth_cache;

use auth_cache::{AuthCache, AuthCacheEntry};
use chrono::{Duration as ChronoDuration, Utc};
use uuid::Uuid;

fn make_entry() -> AuthCacheEntry {
    AuthCacheEntry::new(
        Uuid::new_v4(),
        "tester".into(),
        vec!["read".into()],
        120,
        None,
    )
}

#[test]
fn cache_returns_inserted_entry() {
    let cache = AuthCache::new(NonZeroUsize::new(8).unwrap(), Duration::from_secs(60));
    let hash = [1u8; 32];
    cache.insert(hash, make_entry());
    let got = cache.get(&hash).expect("hit");
    assert_eq!(got.scopes, vec!["read".to_string()]);
}

#[test]
fn cache_expires_after_ttl() {
    let cache = AuthCache::new(NonZeroUsize::new(8).unwrap(), Duration::from_millis(50));
    let hash = [2u8; 32];
    cache.insert(hash, make_entry());
    assert!(cache.get(&hash).is_some());
    thread::sleep(Duration::from_millis(120));
    assert!(cache.get(&hash).is_none(), "entry should have expired");
}

#[test]
fn cache_invalidate_all_clears_entries() {
    let cache = AuthCache::new(NonZeroUsize::new(8).unwrap(), Duration::from_secs(60));
    cache.insert([3u8; 32], make_entry());
    cache.insert([4u8; 32], make_entry());
    assert_eq!(cache.len(), 2);
    cache.invalidate_all();
    assert_eq!(cache.len(), 0);
    assert!(cache.get(&[3u8; 32]).is_none());
}

#[test]
fn cache_respects_capacity_via_lru_eviction() {
    let cache = AuthCache::new(NonZeroUsize::new(2).unwrap(), Duration::from_secs(60));
    cache.insert([10u8; 32], make_entry()); // oldest
    cache.insert([11u8; 32], make_entry());
    cache.insert([12u8; 32], make_entry()); // pushes [10] out
    assert!(cache.get(&[10u8; 32]).is_none(), "LRU should evict oldest");
    assert!(cache.get(&[11u8; 32]).is_some());
    assert!(cache.get(&[12u8; 32]).is_some());
}

#[test]
fn cache_drops_keys_past_expiry() {
    let cache = AuthCache::new(NonZeroUsize::new(4).unwrap(), Duration::from_secs(60));
    let hash = [5u8; 32];
    let past = Utc::now() - ChronoDuration::seconds(1);
    let entry = AuthCacheEntry::new(
        Uuid::new_v4(),
        "expired".into(),
        vec!["read".into()],
        120,
        Some(past),
    );
    cache.insert(hash, entry);
    assert!(cache.get(&hash).is_none(), "expired keys must not return");
}
