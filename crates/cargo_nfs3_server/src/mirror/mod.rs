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
        let iterator_cache = Arc::new(IteratorCache::new(Duration::from_secs(60), 20));
        let cleaner =
            IteratorCacheCleaner::new(Arc::clone(&iterator_cache), Duration::from_secs(30));
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

    async fn create_test_fs_with_files(files: &[&str]) -> (tempfile::TempDir, Fs, FileHandleU64) {
        let temp_dir = tempdir().expect("failed to create temp directory");
        let root_path = temp_dir.path().to_path_buf();

        for file in files {
            fs::write(root_path.join(file), "content")
                .await
                .expect("failed to write test file");
        }

        let fs = Fs::new(&root_path);
        let root_handle = fs.root_dir();
        (temp_dir, fs, root_handle)
    }

    #[tokio::test]
    async fn test_cookie_validation_in_readdir() {
        let (_temp_dir, fs, root_handle) =
            create_test_fs_with_files(&["file1.txt", "file2.txt", "file3.txt"]).await;

        let mut iter1 = fs
            .readdir(&root_handle, 0)
            .await
            .expect("failed to create iterator");

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

        if entries.len() > 1 {
            let cookie_from_consumed_iter = entries[0].cookie;
            assert!(
                fs.readdir(&root_handle, cookie_from_consumed_iter)
                    .await
                    .is_err(),
                "Should fail with BAD_COOKIE for consumed iterator cookie"
            );
        }

        let mut iter_partial = fs
            .readdir(&root_handle, 0)
            .await
            .expect("failed to create iterator");
        let first_entry = match iter_partial.next().await {
            NextResult::Ok(entry) => entry,
            NextResult::Eof => panic!("Expected at least one entry"),
            NextResult::Err(e) => panic!("Unexpected error: {e:?}"),
        };

        let resume_cookie = first_entry.cookie;
        drop(iter_partial);

        let mut iter2 = fs
            .readdir(&root_handle, resume_cookie)
            .await
            .expect("Should succeed with cached cookie");
        match iter2.next().await {
            NextResult::Ok(_) | NextResult::Eof => {}
            NextResult::Err(e) => panic!("Should not fail with cached cookie: {e:?}"),
        }

        let invalid_cookie = 999_999;
        assert!(
            fs.readdir(&root_handle, invalid_cookie).await.is_err(),
            "Should fail with BAD_COOKIE for invalid cookie"
        );
    }

    #[tokio::test]
    async fn test_streaming_iteration() {
        let (_temp_dir, fs, root_handle) = create_test_fs_with_files(&[
            "stream_file1.txt",
            "stream_file2.txt",
            "stream_file3.txt",
        ])
        .await;

        let mut iter = fs
            .readdir(&root_handle, 0)
            .await
            .expect("failed to create iterator");
        let mut entries = Vec::new();

        loop {
            match iter.next().await {
                NextResult::Ok(entry) => entries.push(entry.name.clone()),
                NextResult::Eof => break,
                NextResult::Err(e) => panic!("Unexpected error during streaming: {e:?}"),
            }
        }

        assert!(!entries.is_empty(), "Should have streamed some entries");

        let mut iter2 = fs
            .readdir(&root_handle, 0)
            .await
            .expect("failed to create iterator");
        let mut entries2 = Vec::new();

        loop {
            match iter2.next().await {
                NextResult::Ok(entry) => entries2.push(entry.name.clone()),
                NextResult::Eof => break,
                NextResult::Err(e) => panic!("Unexpected error during streaming: {e:?}"),
            }
        }

        assert_eq!(entries, entries2, "Results should be consistent");
    }

    #[tokio::test]
    async fn test_cookie_uniqueness() {
        let (_temp_dir, fs, root_handle) =
            create_test_fs_with_files(&["unique_file1.txt", "unique_file2.txt"]).await;

        let mut all_cookies = std::collections::HashSet::new();

        for i in 0..3 {
            println!("Testing iterator {i}");
            let mut iter = fs
                .readdir(&root_handle, 0)
                .await
                .expect("failed to create iterator");

            loop {
                match iter.next().await {
                    NextResult::Ok(entry) => {
                        println!(
                            "  Entry: {:?} with cookie: {:#018x}",
                            entry.name, entry.cookie
                        );
                        assert!(
                            all_cookies.insert(entry.cookie),
                            "Cookie {:#018x} is not unique! Already seen in previous iterator.",
                            entry.cookie
                        );

                        let counter = (entry.cookie >> 32) as u32;
                        let position = (entry.cookie & 0xFFFF_FFFF) as u32;

                        println!("    Counter: {counter}, Position: {position}");

                        assert!(position > 0, "Position should be > 0, got {position}");
                    }
                    NextResult::Eof => break,
                    NextResult::Err(e) => panic!("Unexpected error: {e:?}"),
                }
            }
        }

        println!("Total unique cookies generated: {}", all_cookies.len());
        assert!(
            all_cookies.len() >= 3,
            "Should have generated unique cookies across multiple iterations"
        );
    }
}
