//! File cache module using moka for caching open file handles.
//!
//! Files are stored in cache with a time-to-idle expiration policy.
//! When a file is opened for read and a write operation comes in,
//! the file is automatically reopened with read-write access.
//!
//! The cache uses `nfs_fh3`-equivalent handles (u64) as keys, and
//! resolves full paths lazily only when opening the file.

mod file;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub use file::CachedFile;
use moka::future::Cache;
use nfs3_server::nfs3_types::nfs3::nfsstat3;
use nfs3_server::vfs::FileHandleU64;
use tracing::warn;

use crate::mirror::map_io_error;

#[expect(clippy::needless_pass_by_value)]
fn map_nfsstat_arc(e: Arc<nfsstat3>) -> nfsstat3 {
    *e
}

/// File cache using moka with time-to-idle expiration.
///
/// Files are cached based on their file handle (`nfs_fh3` equivalent) and will be
/// automatically evicted if not accessed within the configured TTL.
/// Paths are resolved lazily only when opening a file.
#[derive(Clone)]
pub struct FileCache {
    cache: Cache<FileHandleU64, Arc<CachedFile>>,
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
            .support_invalidation_closures()
            .build();

        Self { cache }
    }

    /// Gets a file handle for reading. If not cached, resolves the path and opens the file.
    ///
    /// The path resolver is only called if the file is not already cached.
    pub async fn get_for_read(
        &self,
        handle: FileHandleU64,
        path: impl FnOnce() -> Result<PathBuf, nfsstat3>,
    ) -> Result<Arc<CachedFile>, nfsstat3> {
        self.cache
            .entry(handle)
            .or_try_insert_with(async {
                let path = path()?;
                CachedFile::open_read(path).await.map_err(map_io_error)
            })
            .await
            .map(moka::Entry::into_value)
            .map_err(map_nfsstat_arc)
    }

    /// Gets a file handle for writing. If cached as read-only, upgrades to read-write.
    ///
    /// The path resolver is only called if the file is not already cached.
    pub async fn get_for_write(
        &self,
        handle: FileHandleU64,
        path: impl FnOnce() -> Result<PathBuf, nfsstat3>,
    ) -> Result<Arc<CachedFile>, nfsstat3> {
        let entry = self
            .cache
            .entry(handle)
            .or_try_insert_with(async {
                let path = path()?;
                CachedFile::open_read_write(path)
                    .await
                    .map_err(map_io_error)
            })
            .await
            .map_err(map_nfsstat_arc)?;

        // If this was a cache hit, we may need to upgrade from read-only to read-write
        let cached = entry.into_value();
        if !cached.is_read_write().await {
            cached.upgrade_to_read_write().await.map_err(map_io_error)?;
        }
        Ok(cached)
    }

    /// Invalidates a cached file entry without flushing or syncing. Used when a file is removed.
    pub async fn invalidate_for_remove(&self, handle: FileHandleU64) {
        if let Some(cached) = self.cache.remove(&handle).await {
            cached.dont_flush_on_drop().await;
        }
    }

    /// Invalidates a cached file entry with flushing and syncing. Used when a file is renamed.
    pub async fn invalidate_for_rename(&self, handle: FileHandleU64) {
        if let Some(cached) = self.cache.remove(&handle).await {
            if let Err(e) = cached.flush().await {
                warn!(
                    "failed to flush file on invalidate for rename: {} - {e}",
                    cached.path().display()
                );
            }
            cached.dont_flush_on_drop().await; // Ensure we don't flush in background on drop
        }
    }

    /// Invalidates all cached file entries under a directory prefix.
    /// Used when a directory is being renamed. Files are flushed via their Drop implementation.
    pub async fn invalidate_dir_for_rename(&self, dir_path: &Path) {
        let dir_path = dir_path.to_path_buf();
        // Use invalidate_entries_if to invalidate all entries under the directory
        // The Drop implementation on FileHandle will flush files with FlushAndSync marker
        let _ = self
            .cache
            .invalidate_entries_if(move |_, cached| cached.path().starts_with(&dir_path));

        // Run pending tasks to ensure invalidations are processed
        self.cache.run_pending_tasks().await;
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
        let handle = FileHandleU64::new(1);
        let cached = cache
            .get_for_read(handle, || Ok(path))
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
        let handle = FileHandleU64::new(1);

        // First, get for read
        let cached = cache
            .get_for_read(handle, || Ok(path))
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
        let handle = FileHandleU64::new(1);
        let path_clone = path.clone();

        let cached1 = cache
            .get_for_read(handle, || Ok(path))
            .await
            .expect("failed to get for read");
        let cached2 = cache
            .get_for_read(handle, || Ok(path_clone))
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
        let handle = FileHandleU64::new(1);

        let _cached = cache
            .get_for_read(handle, || Ok(path))
            .await
            .expect("failed to get for read");

        // Run pending tasks to ensure entry is registered
        cache.cache.run_pending_tasks().await;
        assert_eq!(cache.entry_count(), 1);

        cache.invalidate_for_remove(handle).await;

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
        let handle = FileHandleU64::new(1);
        let path_clone = path.clone();

        // Get directly for write
        let cached = cache
            .get_for_write(handle, || Ok(path_clone))
            .await
            .expect("failed to get for write");
        assert!(cached.is_read_write().await);

        cached.write(0, b"updated").await.expect("failed to write");

        // Verify the write
        let content = tokio::fs::read(&path).await.expect("failed to read");
        assert_eq!(&content[..7], b"updated");
    }
}
