use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use nfs3_server::vfs::FileHandleU64;

/// Cache entry for iterator state
#[derive(Debug, Clone)]
pub struct CachedIteratorInfo {
    pub cookie: u64,
    pub cached_at: Instant,
}

/// Simple iterator cache that tracks valid iterator positions
#[derive(Debug)]
pub struct IteratorCache {
    retention_period: Duration,
    // Map from dir_id to vector of cached iterators
    cache: RwLock<HashMap<FileHandleU64, Vec<CachedIteratorInfo>>>,
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

    /// Check if an iterator position is cached and remove it if so
    /// Returns the cached info if it was found, None otherwise
    pub fn pop_state(&self, dir_id: FileHandleU64, cookie: u64) -> Option<CachedIteratorInfo> {
        let mut cache = self.cache.write().expect("lock is poisoned");

        cache.get_mut(&dir_id).and_then(|iterators| {
            iterators
                .iter()
                .position(|info| info.cookie == cookie)
                .map(|index| iterators.swap_remove(index))
        })
    }

    /// Cache an iterator state
    pub fn cache_state(&self, dir_id: FileHandleU64, cookie: u64, now: Instant) {
        let mut cache = self.cache.write().expect("lock is poisoned");

        let info = CachedIteratorInfo {
            cookie,
            cached_at: now,
        };

        cache.entry(dir_id).or_default().push(info);
        self.trim_cache_for_dir(&mut cache, dir_id);
    }

    /// Clean up stale iterator states - should be called periodically
    pub fn cleanup(&self, now: Instant) {
        let mut cache = self.cache.write().expect("lock is poisoned");

        cache.retain(|_, iterators| {
            iterators.retain(|info| now - info.cached_at <= self.retention_period);
            !iterators.is_empty() // Remove dirs that have no iterators left
        });
    }

    /// Trim cache entries for a specific directory to respect limits
    fn trim_cache_for_dir(
        &self,
        cache: &mut HashMap<FileHandleU64, Vec<CachedIteratorInfo>>,
        dir_id: FileHandleU64,
    ) {
        if let Some(iterators) = cache.get_mut(&dir_id) {
            if iterators.len() > self.max_cached_per_dir as usize {
                // Sort by cache time (oldest first)
                iterators.sort_by_key(|info| info.cached_at);

                // Remove oldest entries by truncating the vector
                let to_keep = self.max_cached_per_dir as usize;
                iterators.drain(0..iterators.len() - to_keep);
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
    fn test_pop_state_returns_entry_regardless_of_staleness() {
        let cache = IteratorCache::new(Duration::from_millis(50), 10);
        let dir_id = FileHandleU64::new(1);
        let cookie = 42;
        let now = Instant::now();

        // Cache an entry
        cache.cache_state(dir_id, cookie, now);

        // Wait for it to become stale
        std::thread::sleep(Duration::from_millis(100));

        // pop_state should return the entry even if it's stale (staleness is only for cleanup)
        assert!(cache.pop_state(dir_id, cookie).is_some());

        // Subsequent calls should return None (entry was removed)
        assert!(cache.pop_state(dir_id, cookie).is_none());
    }

    #[test]
    fn test_cleanup_removes_stale_entries_properly() {
        let cache = IteratorCache::new(Duration::from_millis(50), 10);
        let dir_id = FileHandleU64::new(1);
        let cookie = 42;
        let now = Instant::now();

        // Cache an entry
        cache.cache_state(dir_id, cookie, now);

        // Verify it can be popped before cleanup
        cache.cache_state(dir_id, cookie, now);
        assert!(cache.pop_state(dir_id, cookie).is_some());

        // Cache again and wait for it to become stale
        cache.cache_state(dir_id, cookie, now);
        std::thread::sleep(Duration::from_millis(100));

        // Cleanup should remove stale entries
        cache.cleanup(Instant::now());

        // After cleanup, the stale entry should be gone
        assert!(cache.pop_state(dir_id, cookie).is_none());
    }
}
