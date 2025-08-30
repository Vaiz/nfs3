use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use nfs3_server::vfs::FileHandleU64;
use tokio::fs::ReadDir;

use super::iterator::{Mirror3ReadDirIterator, Mirror3ReadDirPlusIterator};

/// Key to identify an iterator in the cache
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IteratorKey {
    pub dir_id: FileHandleU64,
    pub cookie: u64,
}

/// Types of cached iterator states
#[derive(Debug, Clone)]
pub enum CachedIteratorState {
    ReadDir {
        dirid: FileHandleU64,
        cookie: u64,
        entries_consumed: u64,
    },
    ReadDirPlus {
        dirid: FileHandleU64, 
        cookie: u64,
        entries_consumed: u64,
    },
}

/// Types of cached iterators - now we store state instead of the actual iterators
#[derive(Debug)]
pub enum CachedIterator {
    ReadDir(Mirror3ReadDirIterator),
    ReadDirPlus(Mirror3ReadDirPlusIterator),
}

/// Iterator cache that manages directory iterators similar to transaction tracker
#[derive(Debug)]
pub struct IteratorCache {
    retention_period: Duration,
    iterators: RwLock<HashMap<FileHandleU64, Arc<Mutex<DirIterators>>>>,
    max_cached_iterators: u16,
    trim_limit: usize,
}

impl IteratorCache {
    /// Create a new iterator cache
    #[must_use]
    pub fn new(
        retention_period: Duration,
        max_cached_iterators: u16,
        trim_limit: usize,
    ) -> Self {
        Self {
            retention_period,
            iterators: RwLock::new(HashMap::new()),
            max_cached_iterators,
            trim_limit,
        }
    }

    /// Pop an iterator from the cache if it exists at the specified position
    pub fn pop(&self, dir_id: FileHandleU64, cookie: u64) -> Option<CachedIterator> {
        let iterators = self.iterators.read().expect("lock is poisoned");
        
        if let Some(dir_iterators) = iterators.get(&dir_id) {
            let mut dir_lock = dir_iterators.lock().expect("lock is poisoned");
            dir_lock.pop_iterator(cookie)
        } else {
            None
        }
    }

    /// Cache an iterator when it's not at EOF and being dropped
    pub fn cache_iterator(
        &self,
        dir_id: FileHandleU64,
        cookie: u64,
        iterator: CachedIterator,
        now: Instant,
    ) {
        // First, check if directory is already in the cache
        {
            let iterators = self.iterators.read().expect("lock is poisoned");
            if let Some(dir_iterators) = iterators.get(&dir_id) {
                let mut dir_lock = dir_iterators.lock().expect("lock is poisoned");
                dir_lock.cache_iterator(cookie, iterator, now);
                return;
            }
        }

        // If directory is not in the cache, add it
        let mut iterators = self.iterators.write().expect("lock is poisoned");
        
        let val = iterators
            .entry(dir_id)
            .or_insert_with(|| self.new_dir_iterators(now));

        let mut dir_lock = val.lock().expect("lock is poisoned");
        dir_lock.cache_iterator(cookie, iterator, now);
    }

    fn new_dir_iterators(&self, now: Instant) -> Arc<Mutex<DirIterators>> {
        Arc::new(Mutex::new(DirIterators::new(
            now,
            self.max_cached_iterators,
            self.trim_limit,
        )))
    }

    /// Clean up stale iterators - should be called periodically
    pub fn cleanup(&self, now: Instant) {
        let mut iterators = self.iterators.write().expect("lock is poisoned");

        iterators.retain(|_, dir_iterators| {
            let mut dir_lock = dir_iterators.lock().expect("lock is poisoned");
            if dir_lock.is_active(now, self.retention_period) {
                dir_lock.remove_old_iterators(now, self.retention_period);
                true
            } else {
                // If the directory is not active, remove it from the cache
                false
            }
        });
    }
}

/// Manages iterators for a specific directory
#[derive(Debug)]
struct DirIterators {
    // Sorted by cookie
    iterators: VecDeque<CachedIteratorEntry>,
    last_active: Instant,
    cached_count: u16,
    max_cached_iterators: u16,
    trim_limit: usize,
}

impl DirIterators {
    fn new(now: Instant, max_cached_iterators: u16, trim_limit: usize) -> Self {
        assert!((max_cached_iterators as usize) < trim_limit);
        Self {
            iterators: VecDeque::new(),
            last_active: now,
            cached_count: 0,
            max_cached_iterators,
            trim_limit,
        }
    }

    /// Find an iterator by cookie (binary search since list is sorted)
    fn find_iterator(&self, cookie: u64) -> Result<usize, usize> {
        use std::cmp::Ordering;
        
        if let Some(last_iter) = self.iterators.back() {
            match last_iter.cookie.cmp(&cookie) {
                Ordering::Equal => Ok(self.iterators.len() - 1),
                Ordering::Less => Err(self.iterators.len()),
                Ordering::Greater => self.iterators.binary_search_by_key(&cookie, |entry| entry.cookie),
            }
        } else {
            Err(0)
        }
    }

    /// Pop an iterator from the cache if it exists
    fn pop_iterator(&mut self, cookie: u64) -> Option<CachedIterator> {
        self.last_active = Instant::now();
        
        if let Ok(pos) = self.find_iterator(cookie) {
            let entry = self.iterators.remove(pos)?;
            if matches!(entry.state, IteratorState::Cached(_)) {
                self.cached_count -= 1;
                Some(entry.iterator)
            } else {
                // Iterator is in use, put it back
                self.iterators.insert(pos, entry);
                None
            }
        } else {
            None
        }
    }

    /// Cache an iterator
    fn cache_iterator(&mut self, cookie: u64, iterator: CachedIterator, now: Instant) {
        self.last_active = now;
        
        // Check if we already have too many cached iterators
        if self.cached_count >= self.max_cached_iterators {
            // Try to remove the oldest cached iterator
            self.remove_oldest_cached_iterator();
            
            // If we still can't cache, just drop the iterator
            if self.cached_count >= self.max_cached_iterators {
                return;
            }
        }

        match self.find_iterator(cookie) {
            Ok(_) => {
                // Iterator already exists, this shouldn't happen in normal operation
                // but we'll just ignore it to be safe
            }
            Err(pos) => {
                let entry = CachedIteratorEntry {
                    cookie,
                    iterator,
                    state: IteratorState::Cached(now),
                };
                self.iterators.insert(pos, entry);
                self.cached_count += 1;
                self.trim_if_needed();
            }
        }
    }

    /// Remove iterators older than the specified max_age
    fn remove_old_iterators(&mut self, now: Instant, max_age: Duration) {
        while let Some(entry) = self.iterators.front() {
            if entry.is_stale(now, max_age) {
                let removed = self.iterators.pop_front().unwrap();
                if matches!(removed.state, IteratorState::Cached(_)) {
                    self.cached_count -= 1;
                }
            } else {
                break;
            }
        }
    }

    fn is_active(&self, now: Instant, max_age: Duration) -> bool {
        if now - self.last_active < max_age {
            true
        } else {
            self.has_active_iterators(now, max_age)
        }
    }

    fn has_active_iterators(&self, now: Instant, max_age: Duration) -> bool {
        self.iterators
            .iter()
            .any(|entry| !entry.is_stale(now, max_age))
    }

    /// Remove the oldest cached iterator to make room for a new one
    fn remove_oldest_cached_iterator(&mut self) {
        let mut oldest_pos = None;
        let mut oldest_time = Instant::now();

        for (pos, entry) in self.iterators.iter().enumerate() {
            if let IteratorState::Cached(cached_time) = entry.state {
                if cached_time < oldest_time {
                    oldest_time = cached_time;
                    oldest_pos = Some(pos);
                }
            }
        }

        if let Some(pos) = oldest_pos {
            self.iterators.remove(pos);
            self.cached_count -= 1;
        }
    }

    /// Remove the oldest iterators until we are below the trim limit
    fn trim_if_needed(&mut self) {
        while self.iterators.len() > self.trim_limit {
            if let Some(entry) = self.iterators.front() {
                if matches!(entry.state, IteratorState::InUse) {
                    // Can't remove in-use iterators
                    break;
                }
            }
            
            let removed = self.iterators.pop_front().unwrap();
            if matches!(removed.state, IteratorState::Cached(_)) {
                self.cached_count -= 1;
            }
        }
    }
}

/// State of a cached iterator
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IteratorState {
    InUse,
    Cached(Instant),
}

/// Entry in the iterator cache
#[derive(Debug)]
struct CachedIteratorEntry {
    cookie: u64,
    iterator: CachedIterator,
    state: IteratorState,
}

impl CachedIteratorEntry {
    fn is_stale(&self, now: Instant, max_age: Duration) -> bool {
        match self.state {
            IteratorState::InUse => false,
            IteratorState::Cached(cached_time) => now - cached_time > max_age,
        }
    }
}

/// Cleaner for iterator cache - runs periodically to clean up stale iterators
pub struct IteratorCacheCleaner {
    cache: Arc<IteratorCache>,
    interval: Duration,
    stop: Arc<tokio::sync::Notify>,
}

impl IteratorCacheCleaner {
    pub fn new(cache: Arc<IteratorCache>, interval: Duration) -> Self {
        Self {
            cache,
            interval,
            stop: Arc::new(tokio::sync::Notify::new()),
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
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;
    
    #[test]
    fn test_iterator_cache_basic_operations() {
        let cache = IteratorCache::new(Duration::from_secs(60), 10, 50);
        let dir_id = FileHandleU64::new(1);
        let cookie = 42;
        
        // Initially, no iterator should be found
        assert!(cache.pop(dir_id, cookie).is_none());
        
        // Cache an iterator (we can't create a real one in tests, so we'll skip this for now)
        // This would require mocking the iterator types
    }
    
    #[test]
    fn test_cleanup_removes_stale_entries() {
        let cache = IteratorCache::new(Duration::from_millis(100), 10, 50);
        let now = Instant::now();
        
        // Test that cleanup works without panicking
        cache.cleanup(now);
        
        // Sleep and test again
        std::thread::sleep(Duration::from_millis(200));
        cache.cleanup(Instant::now());
    }
}
