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

/// Type of iterator
#[derive(Debug, Clone, PartialEq)]
pub enum IteratorType {
    ReadDir,
    ReadDirPlus,
}

/// Cache entry for iterator state
#[derive(Debug, Clone)]
pub struct CachedIteratorInfo {
    pub dir_id: FileHandleU64,
    pub cookie: u64,
    pub iterator_type: IteratorType,
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

    /// Check if an iterator position is cached and valid
    pub fn has_cached(&self, dir_id: FileHandleU64, cookie: u64) -> bool {
        let cache = self.cache.read().expect("lock is poisoned");
        let key = IteratorKey { dir_id, cookie };
        
        if let Some(info) = cache.get(&key) {
            // Check if it's still valid (not stale)
            let now = Instant::now();
            now - info.cached_at <= self.retention_period
        } else {
            false
        }
    }

    /// Cache an iterator state
    pub fn cache_state(
        &self,
        dir_id: FileHandleU64,
        cookie: u64,
        iterator_type: IteratorType,
        now: Instant,
    ) {
        let mut cache = self.cache.write().expect("lock is poisoned");
        let key = IteratorKey { dir_id, cookie };
        
        let info = CachedIteratorInfo {
            dir_id,
            cookie,
            iterator_type,
            cached_at: now,
        };
        
        cache.insert(key, info);
        self.trim_cache_for_dir(&mut cache, dir_id, now);
    }

    /// Remove iterator state from cache and return it if it existed
    pub fn remove_state(&self, dir_id: FileHandleU64, cookie: u64) -> Option<CachedIteratorInfo> {
        let mut cache = self.cache.write().expect("lock is poisoned");
        let key = IteratorKey { dir_id, cookie };
        cache.remove(&key)
    }

    /// Clean up stale iterator states - should be called periodically
    pub fn cleanup(&self, now: Instant) {
        let mut cache = self.cache.write().expect("lock is poisoned");
        
        cache.retain(|_, info| {
            now - info.cached_at <= self.retention_period
        });
    }

    /// Trim cache entries for a specific directory to respect limits
    fn trim_cache_for_dir(
        &self,
        cache: &mut HashMap<IteratorKey, CachedIteratorInfo>,
        dir_id: FileHandleU64,
        now: Instant,
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
                    _ = self.stop.notified() => {
                        break;
                    }
                }
            }
        })
    }

    /// Stop the cleaner
    pub fn stop(&self) {
        self.stop.notify_one();
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
        assert!(!cache.has_cached(dir_id, cookie));
        
        // Cache an iterator state
        let now = Instant::now();
        cache.cache_state(dir_id, cookie, IteratorType::ReadDir, now);
        
        // Now it should be cached
        assert!(cache.has_cached(dir_id, cookie));
        
        // Remove it
        let removed = cache.remove_state(dir_id, cookie);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().iterator_type, IteratorType::ReadDir);
        
        // Should no longer be cached
        assert!(!cache.has_cached(dir_id, cookie));
    }
    
    #[test]
    fn test_cleanup_removes_stale_entries() {
        let cache = IteratorCache::new(Duration::from_millis(100), 10);
        let dir_id = FileHandleU64::new(1);
        let cookie = 42;
        let now = Instant::now();
        
        // Cache an entry
        cache.cache_state(dir_id, cookie, IteratorType::ReadDir, now);
        assert!(cache.has_cached(dir_id, cookie));
        
        // Sleep and cleanup
        std::thread::sleep(Duration::from_millis(200));
        cache.cleanup(Instant::now());
        
        // Should be cleaned up
        assert!(!cache.has_cached(dir_id, cookie));
    }
}
