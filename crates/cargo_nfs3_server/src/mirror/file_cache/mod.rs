//! File cache module using moka for caching open file handles.
//!
//! Files are stored in cache with a time-to-idle expiration policy.
//! When a file is opened for read and a write operation comes in,
//! the file is automatically reopened with read-write access.

mod file;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use file::CachedFile;
use moka::future::Cache;
use nfs3_server::nfs3_types::nfs3::nfsstat3;
use tracing::warn;

use crate::mirror::{map_io_error, map_io_error_arc};

/// File cache using moka with time-to-idle expiration.
///
/// Files are cached based on their path and will be automatically
/// evicted if not accessed within the configured TTL.
#[derive(Clone)]
pub struct FileCache {
    cache: Cache<PathBuf, Arc<CachedFile>>,
}

impl std::fmt::Debug for FileCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileCache")
            .field("entry_count", &self.cache.entry_count())
            .finish()
    }
}

impl FileCache {
    /// Creates a new file cache with the specified time-to-idle duration.
    ///
    /// # Arguments
    ///
    /// * `time_to_idle` - Duration after which an unused file will be evicted
    /// * `max_capacity` - Maximum number of files to keep in cache
    pub fn new(time_to_idle: Duration, max_capacity: u64) -> Self {
        let cache = Cache::builder()
            .max_capacity(max_capacity)
            .time_to_idle(time_to_idle)
            .build();

        Self { cache }
    }

    /// Gets a file handle for reading. If not cached, opens the file.
    pub async fn get_for_read(&self, path: PathBuf) -> Result<Arc<CachedFile>, nfsstat3> {
        self.cache
            .entry(path.clone())
            .or_try_insert_with(async { CachedFile::open_read(path.clone()).await })
            .await
            .map(|e| e.into_value())
            .map_err(|e| {
                warn!("failed to open file for read. Path: {}", path.display());
                map_io_error_arc(e)
            })
    }

    /// Gets a file handle for writing. If cached as read-only, upgrades to read-write.
    pub async fn get_for_write(&self, path: PathBuf) -> Result<Arc<CachedFile>, nfsstat3> {
        if let Some(cached) = self.cache.get(&path).await {
            if !cached.is_read_write().await {
                cached.upgrade_to_read_write().await.map_err(map_io_error)?;
            }
            return Ok(cached);
        }

        self.cache
            .entry(path.clone())
            .or_try_insert_with(async { CachedFile::open_read_write(path.clone()).await })
            .await
            .map(|e| e.into_value())
            .map_err(|e| {
                warn!("failed to open file for write. Path: {}", path.display());
                map_io_error_arc(e)
            })
    }

    /// Invalidates a cached file entry. Call this when a file is deleted or renamed.
    pub async fn invalidate(&self, path: &Path) {
        self.cache.invalidate(path).await;
    }

    /// Returns the number of files currently in the cache.
    #[cfg(test)]
    pub fn entry_count(&self) -> u64 {
        self.cache.entry_count()
    }
}

#[cfg(test)]
mod tests {
    use tempfile::NamedTempFile;

    use super::*;

    #[tokio::test]
    async fn test_read_file() {
        let temp_file = NamedTempFile::new().expect("failed to create temp file");
        let path = temp_file.path().to_path_buf();

        // Write some content
        let content = b"Hello, World!";
        tokio::fs::write(&path, content)
            .await
            .expect("failed to write");

        let cache = FileCache::new(Duration::from_secs(60), 100);
        let cached = cache
            .get_for_read(path)
            .await
            .expect("failed to get for read");

        assert!(!cached.is_read_write().await);

        let (data, eof) = cached.read(0, 13).await.expect("failed to read");
        assert_eq!(data, content);
        assert!(eof);
    }

    #[tokio::test]
    async fn test_write_upgrades_mode() {
        let temp_file = NamedTempFile::new().expect("failed to create temp file");
        let path = temp_file.path().to_path_buf();

        // Create the file
        tokio::fs::write(&path, b"initial")
            .await
            .expect("failed to write");

        let cache = FileCache::new(Duration::from_secs(60), 100);

        // First, get for read
        let cached = cache
            .get_for_read(path)
            .await
            .expect("failed to get for read");
        assert!(!cached.is_read_write().await);

        // Now write - should upgrade to ReadWrite
        cached.write(0, b"updated").await.expect("failed to write");
        assert!(cached.is_read_write().await);
    }

    #[tokio::test]
    async fn test_cache_reuses_handle() {
        let temp_file = NamedTempFile::new().expect("failed to create temp file");
        let path = temp_file.path().to_path_buf();

        tokio::fs::write(&path, b"test")
            .await
            .expect("failed to write");

        let cache = FileCache::new(Duration::from_secs(60), 100);

        let cached1 = cache
            .get_for_read(path.clone())
            .await
            .expect("failed to get for read");
        let cached2 = cache
            .get_for_read(path)
            .await
            .expect("failed to get for read");

        // Should be the same Arc
        assert!(Arc::ptr_eq(&cached1, &cached2));
    }

    #[tokio::test]
    async fn test_invalidate() {
        let temp_file = NamedTempFile::new().expect("failed to create temp file");
        let path = temp_file.path().to_path_buf();

        tokio::fs::write(&path, b"test")
            .await
            .expect("failed to write");

        let cache = FileCache::new(Duration::from_secs(60), 100);

        let _cached = cache
            .get_for_read(path.clone())
            .await
            .expect("failed to get for read");

        // Run pending tasks to ensure entry is registered
        cache.cache.run_pending_tasks().await;
        assert_eq!(cache.entry_count(), 1);

        cache.invalidate(&path).await;

        // Run pending tasks to process the invalidation
        cache.cache.run_pending_tasks().await;

        assert_eq!(cache.entry_count(), 0);
    }

    #[tokio::test]
    async fn test_get_for_write_directly() {
        let temp_file = NamedTempFile::new().expect("failed to create temp file");
        let path = temp_file.path().to_path_buf();

        tokio::fs::write(&path, b"initial")
            .await
            .expect("failed to write");

        let cache = FileCache::new(Duration::from_secs(60), 100);

        // Get directly for write
        let cached = cache
            .get_for_write(path.clone())
            .await
            .expect("failed to get for write");
        assert!(cached.is_read_write().await);

        cached.write(0, b"updated").await.expect("failed to write");

        // Verify the write
        let content = tokio::fs::read(&path).await.expect("failed to read");
        assert_eq!(&content[..7], b"updated");
    }
}
