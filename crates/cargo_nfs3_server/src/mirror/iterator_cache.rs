use std::collections::HashMap;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use nfs3_server::vfs::FileHandleU64;
use tokio::fs::ReadDir;

/// Cache entry for iterator state with the actual `ReadDir` object
#[derive(Debug)]
pub struct CachedIteratorInfo {
    pub cookie: u64,
    pub cached_at: Instant,
    pub read_dir: ReadDir,
}

/// Simple iterator cache that tracks valid iterator positions
#[derive(Debug)]
pub struct IteratorCache {
    retention_period: Duration,
    // Map from dir_id to vector of cached iterators
    cache: RwLock<HashMap<FileHandleU64, Vec<CachedIteratorInfo>>>,
    max_cached_per_dir: u16,
    /// Atomic counter for generating unique cookies
    cookie_counter: AtomicU32,
}

impl IteratorCache {
    /// Create a new iterator cache
    #[must_use]
    pub fn new(retention_period: Duration, max_cached_per_dir: u16) -> Self {
        Self {
            retention_period,
            cache: RwLock::new(HashMap::new()),
            max_cached_per_dir,
            cookie_counter: AtomicU32::new(0),
        }
    }

    /// Generates a unique base cookie for an iterator
    pub fn generate_base_cookie(&self) -> u64 {
        let counter = self.cookie_counter.fetch_add(1, Ordering::SeqCst);
        // Base uses upper 32 bits
        (u64::from(counter)) << 32
    }

    /// Check if an iterator position is cached and remove it if so
    pub fn pop_state(&self, dir_id: FileHandleU64, cookie: u64) -> Option<CachedIteratorInfo> {
        let mut cache = self.cache.write().expect("lock is poisoned");

        cache.get_mut(&dir_id).and_then(|iterators| {
            iterators
                .iter()
                .position(|info| info.cookie == cookie)
                .map(|index| iterators.swap_remove(index))
        })
    }

    /// Cache an iterator state with `ReadDir` object
    pub fn cache_state(&self, dir_id: FileHandleU64, cookie: u64, read_dir: ReadDir, now: Instant) {
        let mut cache = self.cache.write().expect("lock is poisoned");

        let info = CachedIteratorInfo {
            cookie,
            cached_at: now,
            read_dir,
        };

        cache.entry(dir_id).or_default().push(info);
        self.trim_cache_for_dir(&mut cache, dir_id);
    }

    /// Clean up stale iterator states - should be called periodically
    pub fn cleanup(&self, now: Instant) {
        let mut cache = self.cache.write().expect("lock is poisoned");

        cache.retain(|_, iterators| {
            iterators.retain(|info| now - info.cached_at <= self.retention_period);
            !iterators.is_empty()
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
                iterators.sort_by_key(|info| info.cached_at);
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
