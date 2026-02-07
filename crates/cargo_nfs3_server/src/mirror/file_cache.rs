//! File cache module using moka for caching open file handles.
//!
//! Files are stored in cache with a time-to-idle expiration policy.
//! When a file is opened for read and a write operation comes in,
//! the file is automatically reopened with read-write access.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use moka::future::Cache;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};
use tokio::sync::RwLock;

/// A file handle with its access mode. The file object is contained within the enum variant.
#[derive(Debug)]
pub enum FileHandle {
    /// File is open for reading only.
    Read(File),
    /// File is open for both reading and writing.
    ReadWrite(File),
}

impl FileHandle {
    /// Returns whether this handle is in read-write mode.
    const fn is_read_write(&self) -> bool {
        matches!(self, Self::ReadWrite(_))
    }

    /// Gets a mutable reference to the underlying file.
    const fn file_mut(&mut self) -> &mut File {
        match self {
            Self::Read(f) | Self::ReadWrite(f) => f,
        }
    }
}

/// A cached file handle with its current access mode.
#[derive(Debug)]
pub struct CachedFile {
    path: PathBuf,
    handle: RwLock<FileHandle>,
}

impl CachedFile {
    /// Creates a new cached file opened in read mode.
    async fn open_read(path: PathBuf) -> std::io::Result<Self> {
        let file = File::open(&path).await?;
        Ok(Self {
            path,
            handle: RwLock::new(FileHandle::Read(file)),
        })
    }

    /// Creates a new cached file opened in read-write mode.
    async fn open_read_write(path: PathBuf) -> std::io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .await?;
        Ok(Self {
            path,
            handle: RwLock::new(FileHandle::ReadWrite(file)),
        })
    }

    /// Returns whether this file is in read-write mode.
    pub async fn is_read_write(&self) -> bool {
        self.handle.read().await.is_read_write()
    }

    /// Ensures the file is open for reading and performs a read operation.
    ///
    /// Returns the data read and whether EOF was reached.
    pub async fn read(&self, offset: u64, count: u32) -> std::io::Result<(Vec<u8>, bool)> {
        let mut handle_guard = self.handle.write().await;
        let file = handle_guard.file_mut();

        let len = file.metadata().await?.len();
        if offset >= len || count == 0 {
            return Ok((Vec::new(), u64::from(count) >= len));
        }

        let count = u64::from(count).min(len - offset);
        file.seek(SeekFrom::Start(offset)).await?;

        let mut buf = vec![0; usize::try_from(count).unwrap_or(0)];
        file.read_exact(&mut buf).await?;

        Ok((buf, offset + count >= len))
    }

    /// Ensures the file is open for writing and performs a write operation.
    ///
    /// If the file is currently open for read only, it will be reopened
    /// with read-write access.
    pub async fn write(&self, offset: u64, data: &[u8]) -> std::io::Result<()> {
        // Check if we need to upgrade to read-write mode
        if !self.handle.read().await.is_read_write() {
            self.upgrade_to_read_write().await?;
        }

        let mut handle_guard = self.handle.write().await;
        let file = handle_guard.file_mut();

        file.seek(SeekFrom::Start(offset)).await?;
        file.write_all(data).await?;
        file.flush().await?;

        Ok(())
    }

    /// Upgrades the file from read mode to read-write mode.
    pub async fn upgrade_to_read_write(&self) -> std::io::Result<()> {
        let mut handle_guard = self.handle.write().await;

        // Check if already in read-write mode
        if handle_guard.is_read_write() {
            return Ok(());
        }

        // Reopen with read-write access
        let new_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.path)
            .await?;

        *handle_guard = FileHandle::ReadWrite(new_file);

        Ok(())
    }
}

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
    pub async fn get_for_read(&self, path: &Path) -> std::io::Result<Arc<CachedFile>> {
        let path_buf = path.to_path_buf();

        // Try to get from cache first
        if let Some(cached) = self.cache.get(&path_buf).await {
            return Ok(cached);
        }

        // Not in cache, open and insert
        let cached_file = Arc::new(CachedFile::open_read(path_buf.clone()).await?);
        self.cache.insert(path_buf, Arc::clone(&cached_file)).await;
        Ok(cached_file)
    }

    /// Gets a file handle for writing. If cached as read-only, upgrades to read-write.
    pub async fn get_for_write(&self, path: &Path) -> std::io::Result<Arc<CachedFile>> {
        let path_buf = path.to_path_buf();

        // Try to get from cache first
        if let Some(cached) = self.cache.get(&path_buf).await {
            // Ensure it's in read-write mode
            if !cached.is_read_write().await {
                cached.upgrade_to_read_write().await?;
            }
            return Ok(cached);
        }

        // Not in cache, open directly in read-write mode
        let cached_file = Arc::new(CachedFile::open_read_write(path_buf.clone()).await?);
        self.cache.insert(path_buf, Arc::clone(&cached_file)).await;
        Ok(cached_file)
    }

    /// Invalidates a cached file entry. Call this when a file is deleted or renamed.
    pub async fn invalidate(&self, path: &Path) {
        self.cache.invalidate(&path.to_path_buf()).await;
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
            .get_for_read(&path)
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
            .get_for_read(&path)
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
            .get_for_read(&path)
            .await
            .expect("failed to get for read");
        let cached2 = cache
            .get_for_read(&path)
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
            .get_for_read(&path)
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
            .get_for_write(&path)
            .await
            .expect("failed to get for write");
        assert!(cached.is_read_write().await);

        cached.write(0, b"updated").await.expect("failed to write");

        // Verify the write
        let content = tokio::fs::read(&path).await.expect("failed to read");
        assert_eq!(&content[..7], b"updated");
    }
}
