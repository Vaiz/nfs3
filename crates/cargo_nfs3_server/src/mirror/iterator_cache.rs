use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use nfs3_server::vfs::FileHandleU64;

/// Key to identify an iterator position in the cache
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IteratorKey {
    pub dir_id: FileHandleU64,
    pub cookie: u64,
}

/// Cache entry for iterator state
#[derive(Debug, Clone)]
pub struct CachedIteratorInfo {
    pub cached_at: Instant,
}

/// Simple iterator cache that tracks valid iterator positions
#[derive(Debug)]
pub struct IteratorCache {
    retention_period: Duration,
    // Map from (dir_id, cookie) to cached iterator info
    cache: RwLock<HashMap<IteratorKey, CachedIteratorInfo>>,
    max_cached_per_dir: u16,
}

impl IteratorCache {
    /// Create a new iterator cache
    #[must_use]
    pub fn new(retention_period: Duration, max_cached_per_dir: u16) -> Self {
        Self {
            retention_period,
            cache: RwLock::new(HashMap::new()),
            max_cached_per_dir,
        }
    }

    /// Check if an iterator position is cached and valid, and remove it if so
    /// Returns the cached info if it was found and valid, None otherwise
    pub fn pop_state(&self, dir_id: FileHandleU64, cookie: u64) -> Option<CachedIteratorInfo> {
        let mut cache = self.cache.write().expect("lock is poisoned");
        let key = IteratorKey { dir_id, cookie };

        // Check if entry exists and determine if it's valid
        let is_valid = cache.get(&key).is_some_and(|info| {
            let now = Instant::now();
            now - info.cached_at <= self.retention_period
        });

        // Remove the entry (if it exists) and return it only if it was valid
        cache.remove(&key).filter(|_| is_valid)
    }

    /// Cache an iterator state
    pub fn cache_state(&self, dir_id: FileHandleU64, cookie: u64, now: Instant) {
        let mut cache = self.cache.write().expect("lock is poisoned");
        let key = IteratorKey { dir_id, cookie };

        let info = CachedIteratorInfo { cached_at: now };

        cache.insert(key, info);
        self.trim_cache_for_dir(&mut cache, dir_id, now);
    }

    /// Clean up stale iterator states - should be called periodically
    pub fn cleanup(&self, now: Instant) {
        let mut cache = self.cache.write().expect("lock is poisoned");

        cache.retain(|_, info| now - info.cached_at <= self.retention_period);
    }

    /// Trim cache entries for a specific directory to respect limits
    fn trim_cache_for_dir(
        &self,
        cache: &mut HashMap<IteratorKey, CachedIteratorInfo>,
        dir_id: FileHandleU64,
        _now: Instant,
    ) {
        let mut dir_entries: Vec<_> = cache
            .iter()
            .filter(|(key, _)| key.dir_id == dir_id)
            .map(|(key, info)| (key.clone(), info.cached_at))
            .collect();

        if dir_entries.len() > self.max_cached_per_dir as usize {
            // Sort by cache time (oldest first)
            dir_entries.sort_by_key(|(_, cached_at)| *cached_at);

            // Remove oldest entries
            let to_remove = dir_entries.len() - self.max_cached_per_dir as usize;
            for (key, _) in dir_entries.into_iter().take(to_remove) {
                cache.remove(&key);
            }
        }
    }
}

/// Cleaner for iterator cache - runs periodically to clean up stale iterator states
pub struct IteratorCacheCleaner {
    cache: std::sync::Arc<IteratorCache>,
    interval: Duration,
    stop: std::sync::Arc<tokio::sync::Notify>,
}

impl IteratorCacheCleaner {
    pub fn new(cache: std::sync::Arc<IteratorCache>, interval: Duration) -> Self {
        Self {
            cache,
            interval,
            stop: std::sync::Arc::new(tokio::sync::Notify::new()),
        }
    }

    /// Start the cleaner task
    pub fn start(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(self.interval);

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let now = Instant::now();
                        self.cache.cleanup(now);
                    }
                    () = self.stop.notified() => {
                        break;
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iterator_cache_basic_operations() {
        let cache = IteratorCache::new(Duration::from_secs(60), 10);
        let dir_id = FileHandleU64::new(1);
        let cookie = 42;

        // Initially, no iterator should be cached
        assert!(cache.pop_state(dir_id, cookie).is_none());

        // Cache an iterator state
        let now = Instant::now();
        cache.cache_state(dir_id, cookie, now);

        // Now it should be cached and we can pop it
        let popped = cache.pop_state(dir_id, cookie);
        assert!(popped.is_some());

        // Should no longer be cached after popping
        assert!(cache.pop_state(dir_id, cookie).is_none());
    }

    #[test]
    fn test_cleanup_removes_stale_entries() {
        let cache = IteratorCache::new(Duration::from_millis(100), 10);
        let dir_id = FileHandleU64::new(1);
        let cookie = 42;
        let now = Instant::now();

        // Cache an entry
        cache.cache_state(dir_id, cookie, now);

        // Should be able to pop it immediately
        cache.cache_state(dir_id, cookie, now);
        assert!(cache.pop_state(dir_id, cookie).is_some());

        // Cache again and wait for it to become stale
        cache.cache_state(dir_id, cookie, now);
        std::thread::sleep(Duration::from_millis(200));
        cache.cleanup(Instant::now());

        // Should be cleaned up and not available for popping
        assert!(cache.pop_state(dir_id, cookie).is_none());
    }

    #[test]
    fn test_pop_state_removes_stale_entries() {
        let cache = IteratorCache::new(Duration::from_millis(50), 10);
        let dir_id = FileHandleU64::new(1);
        let cookie = 42;
        let now = Instant::now();

        // Cache an entry
        cache.cache_state(dir_id, cookie, now);

        // Wait for it to become stale
        std::thread::sleep(Duration::from_millis(100));

        // pop_state should return None for stale entries and remove them
        assert!(cache.pop_state(dir_id, cookie).is_none());

        // Subsequent calls should also return None (entry was removed)
        assert!(cache.pop_state(dir_id, cookie).is_none());
    }
}
