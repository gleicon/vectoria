use crate::model::SearchResponse;
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

/// TTL-bounded in-memory cache for search results.
/// Eviction: expired entries purged on insert when at capacity; half cleared if still over.
pub struct QueryResultCache {
    store: RwLock<HashMap<String, CacheEntry>>,
    ttl: Duration,
    max_entries: usize,
}

struct CacheEntry {
    response: SearchResponse,
    expires_at: Instant,
}

impl QueryResultCache {
    pub fn new(ttl_secs: u64, max_entries: usize) -> Self {
        Self {
            store: RwLock::new(HashMap::new()),
            ttl: Duration::from_secs(ttl_secs),
            max_entries,
        }
    }

    pub fn get(&self, key: &str) -> Option<SearchResponse> {
        let store = self.store.read().unwrap();
        store.get(key).and_then(|e| {
            if Instant::now() < e.expires_at {
                Some(e.response.clone())
            } else {
                None
            }
        })
    }

    /// Invalidate all cached entries. Called after any override mutation so the next
    /// search re-applies the current override set rather than serving a stale slice.
    pub fn clear(&self) {
        self.store.write().unwrap().clear();
    }

    pub fn put(&self, key: String, response: SearchResponse) {
        let mut store = self.store.write().unwrap();
        let now = Instant::now();
        if store.len() >= self.max_entries {
            store.retain(|_, e| e.expires_at > now);
            if store.len() >= self.max_entries {
                let remove_count = store.len() / 2;
                let keys: Vec<String> = store.keys().take(remove_count).cloned().collect();
                for k in keys { store.remove(&k); }
            }
        }
        store.insert(key, CacheEntry {
            response,
            expires_at: now + self.ttl,
        });
    }

}
