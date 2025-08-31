#![allow(clippy::unwrap_used)] // TODO: Replace unwraps with proper error handling

mod iterator;
mod iterator_cache;
mod symbols_cache;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use iterator::MirrorFsIterator;
use iterator_cache::{IteratorCache, IteratorCacheCleaner};
use nfs3_server::fs_util::metadata_to_fattr3;
use nfs3_server::nfs3_types::nfs3::{fattr3, filename3, nfspath3, nfsstat3};
use nfs3_server::vfs::{FileHandleU64, NfsReadFileSystem, ReadDirIterator, ReadDirPlusIterator};
use symbols_cache::SymbolsCache;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};

use crate::string_ext::{FromOsString, IntoOsString};

#[derive(Debug)]
pub struct Fs {
    root: PathBuf,
    cache: Arc<SymbolsCache>,
    iterator_cache: Arc<IteratorCache>,
    _cleaner_handle: Option<tokio::task::JoinHandle<()>>,
}

impl Fs {
    pub fn new(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref().to_path_buf();
        let cache = Arc::new(SymbolsCache::new(root.clone()));

        // Create iterator cache with reasonable defaults:
        // - 60 seconds retention period
        // - Maximum 20 cached iterators per directory
        let iterator_cache = Arc::new(IteratorCache::new(Duration::from_secs(60), 20));

        // Start the cleaner task to periodically clean up stale cache entries
        let cleaner = IteratorCacheCleaner::new(
            Arc::clone(&iterator_cache),
            Duration::from_secs(30), // Clean every 30 seconds
        );
        let cleaner_handle = cleaner.start();

        Self {
            root,
            cache,
            iterator_cache,
            _cleaner_handle: Some(cleaner_handle),
        }
    }

    fn path(&self, id: FileHandleU64) -> Result<PathBuf, nfsstat3> {
        let relative_path = self.cache.handle_to_path(id)?;
        Ok(self.root.join(relative_path))
    }

    async fn read(
        &self,
        path: PathBuf,
        start: u64,
        count: u32,
    ) -> std::io::Result<(Vec<u8>, bool)> {
        let mut f = File::open(&path).await?;
        let len = f.metadata().await?.len();
        if start >= len || count == 0 {
            return Ok((Vec::new(), u64::from(count) >= len));
        }

        let count = u64::from(count).min(len - start);
        f.seek(SeekFrom::Start(start)).await?;

        let mut buf = vec![0; usize::try_from(count).unwrap_or(0)];
        f.read_exact(&mut buf).await?;

        Ok((buf, start + count >= len))
    }

    async fn get_or_create_iterator(
        &self,
        dirid: FileHandleU64,
        cookie: u64,
    ) -> Result<MirrorFsIterator, nfsstat3> {
        // Always create an iterator - it will handle caching and validation internally
        MirrorFsIterator::new(
            self.root.clone(),
            Arc::clone(&self.cache),
            Arc::clone(&self.iterator_cache),
            dirid,
            cookie,
        )
        .await
    }
}

impl NfsReadFileSystem for Fs {
    type Handle = FileHandleU64;

    fn root_dir(&self) -> Self::Handle {
        SymbolsCache::ROOT_ID
    }

    async fn lookup(
        &self,
        dirid: &Self::Handle,
        filename: &filename3<'_>,
    ) -> Result<Self::Handle, nfsstat3> {
        self.cache.lookup_by_id(*dirid, filename.as_os_str(), true)
    }

    async fn getattr(&self, id: &Self::Handle) -> Result<fattr3, nfsstat3> {
        let path = self.path(*id)?;
        let metadata = tokio::fs::symlink_metadata(&path)
            .await
            .map_err(map_io_error)?;

        Ok(metadata_to_fattr3(id.as_u64(), &metadata))
    }

    async fn read(
        &self,
        id: &Self::Handle,
        offset: u64,
        count: u32,
    ) -> Result<(Vec<u8>, bool), nfsstat3> {
        let path = self.path(*id)?;
        self.read(path, offset, count).await.map_err(map_io_error)
    }

    async fn readdir(
        &self,
        dirid: &Self::Handle,
        cookie: u64,
    ) -> Result<impl ReadDirIterator, nfsstat3> {
        self.get_or_create_iterator(*dirid, cookie).await
    }

    async fn readdirplus(
        &self,
        dirid: &Self::Handle,
        cookie: u64,
    ) -> Result<impl ReadDirPlusIterator<Self::Handle>, nfsstat3> {
        self.get_or_create_iterator(*dirid, cookie).await
    }

    async fn readlink(&self, id: &Self::Handle) -> Result<nfspath3<'_>, nfsstat3> {
        let path = self.path(*id)?;
        match tokio::fs::read_link(&path).await {
            Ok(target) => Ok(FromOsString::from_os_str(target.as_os_str())),
            Err(e) => {
                tracing::warn!(id = id.as_u64(), path = %path.display(), error = %e, "failed to read symlink target");
                if e.kind() == std::io::ErrorKind::NotFound {
                    Err(nfsstat3::NFS3ERR_NOENT)
                } else {
                    Err(nfsstat3::NFS3ERR_BADTYPE)
                }
            }
        }
    }
}

#[expect(clippy::needless_pass_by_value)]
fn map_io_error(err: std::io::Error) -> nfsstat3 {
    use std::io::ErrorKind;
    match err.kind() {
        ErrorKind::NotFound => nfsstat3::NFS3ERR_NOENT,
        ErrorKind::PermissionDenied => nfsstat3::NFS3ERR_ACCES,
        ErrorKind::AlreadyExists => nfsstat3::NFS3ERR_EXIST,
        ErrorKind::IsADirectory => nfsstat3::NFS3ERR_ISDIR,
        ErrorKind::NotADirectory => nfsstat3::NFS3ERR_NOTDIR,
        ErrorKind::ReadOnlyFilesystem => nfsstat3::NFS3ERR_ROFS,
        ErrorKind::Unsupported => nfsstat3::NFS3ERR_NOTSUPP,
        _ => nfsstat3::NFS3ERR_IO,
    }
}

#[cfg(test)]
mod tests {
    use nfs3_server::vfs::{NextResult, ReadDirIterator};
    use tempfile::tempdir;
    use tokio::fs;

    use super::*;

    #[tokio::test]
    async fn test_cookie_validation_in_readdir() {
        let temp_dir = tempdir().unwrap();
        let root_path = temp_dir.path().to_path_buf();

        // Create some test files
        fs::write(root_path.join("file1.txt"), "content1")
            .await
            .unwrap();
        fs::write(root_path.join("file2.txt"), "content2")
            .await
            .unwrap();
        fs::write(root_path.join("file3.txt"), "content3")
            .await
            .unwrap();

        let fs = Fs::new(&root_path);
        let root_handle = fs.root_dir();

        // First readdir call with cookie 0 (start)
        let mut iter1 = fs.readdir(&root_handle, 0).await.unwrap();

        // Collect all entries
        let mut entries = Vec::new();
        loop {
            match iter1.next().await {
                NextResult::Ok(entry) => {
                    entries.push(entry);
                }
                NextResult::Eof => break,
                NextResult::Err(e) => panic!("Unexpected error: {e:?}"),
            }
        }

        assert!(!entries.is_empty(), "Should have at least some entries");

        // Test that cookies from a fully consumed iterator are no longer valid
        // since the iterator was dropped and no cached state remains
        if entries.len() > 1 {
            let cookie_from_consumed_iter = entries[0].cookie;
            match fs.readdir(&root_handle, cookie_from_consumed_iter).await {
                Err(nfsstat3::NFS3ERR_BAD_COOKIE) => {
                    // This is expected - the iterator was fully consumed and dropped
                }
                Ok(_) => panic!("Should fail with BAD_COOKIE for consumed iterator cookie"),
                Err(other) => panic!("Unexpected error for consumed iterator cookie: {other:?}"),
            }
        }

        // Test cookie reuse with partial iteration (iterator dropped before completion)
        let mut iter_partial = fs.readdir(&root_handle, 0).await.unwrap();
        let first_entry = match iter_partial.next().await {
            NextResult::Ok(entry) => entry,
            NextResult::Eof => panic!("Expected at least one entry"),
            NextResult::Err(e) => panic!("Unexpected error: {e:?}"),
        };

        // Use the cookie from the first entry for resumption
        let resume_cookie = first_entry.cookie;

        // Drop the iterator while it still has a cached ReadDir state
        drop(iter_partial);

        // This cookie should be valid because the iterator was dropped with cached state
        match fs.readdir(&root_handle, resume_cookie).await {
            Ok(mut iter2) => {
                // Should be able to continue iteration
                match iter2.next().await {
                    NextResult::Ok(_) | NextResult::Eof => {
                        // Either result is acceptable - we successfully resumed
                    }
                    NextResult::Err(e) => panic!("Should not fail with cached cookie: {e:?}"),
                }
            }
            Err(e) => panic!("Should succeed with cached cookie, got: {e:?}"),
        }

        // Test that invalid cookie is rejected
        let invalid_cookie = 999_999;
        match fs.readdir(&root_handle, invalid_cookie).await {
            Err(nfsstat3::NFS3ERR_BAD_COOKIE) => {
                // This is expected for invalid cookie
            }
            Ok(_) => panic!("Should fail with BAD_COOKIE for invalid cookie"),
            Err(other) => panic!("Unexpected error for invalid cookie: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_streaming_iteration() {
        let temp_dir = tempdir().unwrap();
        let root_path = temp_dir.path().to_path_buf();

        // Create test files
        fs::write(root_path.join("stream_file1.txt"), "content")
            .await
            .unwrap();
        fs::write(root_path.join("stream_file2.txt"), "content")
            .await
            .unwrap();
        fs::write(root_path.join("stream_file3.txt"), "content")
            .await
            .unwrap();

        let fs = Fs::new(&root_path);
        let root_handle = fs.root_dir();

        // Test that we can iterate through all entries with streaming
        let mut iter = fs.readdir(&root_handle, 0).await.unwrap();
        let mut entries = Vec::new();

        loop {
            match iter.next().await {
                NextResult::Ok(entry) => entries.push(entry.name.clone()),
                NextResult::Eof => break,
                NextResult::Err(e) => panic!("Unexpected error during streaming: {e:?}"),
            }
        }

        // Should have all our test files
        assert!(!entries.is_empty(), "Should have streamed some entries");

        // Verify consistency across multiple iterator creations
        let mut iter2 = fs.readdir(&root_handle, 0).await.unwrap();
        let mut entries2 = Vec::new();

        loop {
            match iter2.next().await {
                NextResult::Ok(entry) => entries2.push(entry.name.clone()),
                NextResult::Eof => break,
                NextResult::Err(e) => panic!("Unexpected error during streaming: {e:?}"),
            }
        }

        // Results should be consistent between different iterator instances
        assert_eq!(entries, entries2, "Results should be consistent");
    }

    #[tokio::test]
    async fn test_cookie_uniqueness() {
        let temp_dir = tempdir().unwrap();
        let root_path = temp_dir.path().to_path_buf();

        // Create test files
        fs::write(root_path.join("unique_file1.txt"), "content")
            .await
            .unwrap();
        fs::write(root_path.join("unique_file2.txt"), "content")
            .await
            .unwrap();

        let fs = Fs::new(&root_path);
        let root_handle = fs.root_dir();

        // Create multiple iterators and collect their cookies
        let mut all_cookies = std::collections::HashSet::new();

        for i in 0..3 {
            println!("Testing iterator {i}");
            let mut iter = fs.readdir(&root_handle, 0).await.unwrap();
            
            loop {
                match iter.next().await {
                    NextResult::Ok(entry) => {
                        println!("  Entry: {:?} with cookie: {:#018x}", entry.name, entry.cookie);
                        // Verify that this cookie is unique across all iterators
                        assert!(
                            all_cookies.insert(entry.cookie),
                            "Cookie {:#018x} is not unique! Already seen in previous iterator.",
                            entry.cookie
                        );
                        
                        // Verify cookie structure: upper 32 bits are counter, lower 32 bits are position
                        let counter = (entry.cookie >> 32) as u32;
                        let position = (entry.cookie & 0xFFFF_FFFF) as u32;
                        
                        println!("    Counter: {}, Position: {}", counter, position);
                        
                        // Position should be > 0 since we increment before creating the cookie
                        assert!(position > 0, "Position should be > 0, got {}", position);
                    }
                    NextResult::Eof => break,
                    NextResult::Err(e) => panic!("Unexpected error: {e:?}"),
                }
            }
        }
        
        println!("Total unique cookies generated: {}", all_cookies.len());
        assert!(all_cookies.len() >= 3, "Should have generated unique cookies across multiple iterations");
    }
}
