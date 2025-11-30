mod iterator;
mod iterator_cache;
mod symbols_cache;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use iterator::MirrorFsIterator;
use iterator_cache::{IteratorCache, IteratorCacheCleaner};
use nfs3_server::fs_util::metadata_to_fattr3;
use nfs3_server::nfs3_types::nfs3::{
    createverf3, fattr3, filename3, nfspath3, nfsstat3, sattr3, set_gid3, set_mode3, set_size3,
    set_uid3,
};
use nfs3_server::vfs::{
    FileHandleU64, NfsFileSystem, NfsReadFileSystem, ReadDirIterator, ReadDirPlusIterator,
    VFSCapabilities,
};
use symbols_cache::SymbolsCache;
use tokio::fs::{File, ReadDir};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};
use tracing::debug;

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
        let (read_dir, cookie_value) = self.initialize_read_dir(dirid, cookie).await?;

        Ok(MirrorFsIterator::new(
            self.root.clone(),
            Arc::clone(&self.cache),
            Arc::clone(&self.iterator_cache),
            dirid,
            Some(read_dir),
            cookie_value,
        ))
    }

    /// Initialize the `ReadDir` state based on the cookie value.
    /// Returns (`ReadDir`, `actual_cookie`) tuple.
    async fn initialize_read_dir(
        &self,
        dirid: FileHandleU64,
        cookie: u64,
    ) -> Result<(ReadDir, u64), nfsstat3> {
        let dir_path = {
            let relative_path = self.cache.handle_to_path(dirid)?;
            self.root.join(&relative_path)
        };

        if cookie == 0 {
            debug!(
                "Creating new ReadDir for directory: {} (cookie = 0)",
                dir_path.display()
            );
            // Check if it's a directory
            let metadata = tokio::fs::symlink_metadata(&dir_path)
                .await
                .map_err(|_| nfsstat3::NFS3ERR_NOENT)?;
            if !metadata.is_dir() {
                return Err(nfsstat3::NFS3ERR_NOTDIR);
            }
            let read_dir = tokio::fs::read_dir(&dir_path)
                .await
                .map_err(|_| nfsstat3::NFS3ERR_IO)?;
            let base = self.iterator_cache.generate_base_cookie();
            Ok((read_dir, base))
        } else if let Some(cached_info) = self.iterator_cache.pop_state(dirid, cookie) {
            debug!(
                "Reusing cached ReadDir for directory: {} at cookie: {cookie}",
                dir_path.display()
            );
            Ok((cached_info.read_dir, cached_info.cookie))
        } else {
            debug!("No cached ReadDir found for cookie {cookie}, returning BAD_COOKIE error");
            Err(nfsstat3::NFS3ERR_BAD_COOKIE)
        }
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
        ErrorKind::CrossesDevices => nfsstat3::NFS3ERR_XDEV,
        _ => nfsstat3::NFS3ERR_IO,
    }
}

impl NfsFileSystem for Fs {
    fn capabilities(&self) -> VFSCapabilities {
        VFSCapabilities::ReadWrite
    }

    async fn setattr(&self, id: &Self::Handle, setattr: sattr3) -> Result<fattr3, nfsstat3> {
        let path = self.path(*id)?;
        nfs3_server::fs_util::path_setattr(&path, &setattr).await?;
        self.getattr(id).await
    }

    async fn write(&self, id: &Self::Handle, offset: u64, data: &[u8]) -> Result<fattr3, nfsstat3> {
        let path = self.path(*id)?;

        // Check if it's a regular file
        let metadata = tokio::fs::symlink_metadata(&path)
            .await
            .map_err(map_io_error)?;

        if !metadata.is_file() {
            return Err(nfsstat3::NFS3ERR_INVAL);
        }

        (async || {
            let mut file = tokio::fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .await?;
            file.seek(SeekFrom::Start(offset)).await?;
            file.write_all(data).await?;
            file.flush().await
        })()
        .await
        .map_err(map_io_error)?;

        self.getattr(id).await
    }

    async fn create(
        &self,
        dirid: &Self::Handle,
        filename: &filename3<'_>,
        attr: sattr3,
    ) -> Result<(Self::Handle, fattr3), nfsstat3> {
        let dir_path = self.path(*dirid)?;
        let file_path = dir_path.join(filename.as_os_str());

        (async || {
            let file = tokio::fs::File::create(&file_path).await?;
            if let set_size3::Some(size) = attr.size {
                file.set_len(size).await?;
            }
            Ok(())
        })()
        .await
        .map_err(map_io_error)?;

        // Register the file in the cache
        let file_id = self
            .cache
            .lookup_by_id(*dirid, filename.as_os_str(), true)?;

        let fattr = self.setattr(&file_id, attr).await?;
        Ok((file_id, fattr))
    }

    async fn create_exclusive(
        &self,
        dirid: &Self::Handle,
        filename: &filename3<'_>,
        _createverf: createverf3,
    ) -> Result<Self::Handle, nfsstat3> {
        let dir_path = self.path(*dirid)?;
        let file_path = dir_path.join(filename.as_os_str());

        let file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file_path)
            .await
            .map_err(map_io_error)?;
        drop(file);

        // Register the file in the cache
        let file_id = self
            .cache
            .lookup_by_id(*dirid, filename.as_os_str(), true)?;

        Ok(file_id)
    }

    async fn mkdir(
        &self,
        dirid: &Self::Handle,
        dirname: &filename3<'_>,
    ) -> Result<(Self::Handle, fattr3), nfsstat3> {
        let dir_path = self.path(*dirid)?;
        let new_dir_path = dir_path.join(dirname.as_os_str());

        // Create the directory
        tokio::fs::create_dir(&new_dir_path)
            .await
            .map_err(map_io_error)?;

        // Register the directory in the cache
        let new_dir_id = self.cache.lookup_by_id(*dirid, dirname.as_os_str(), true)?;

        let fattr = self.getattr(&new_dir_id).await?;
        Ok((new_dir_id, fattr))
    }

    async fn remove(&self, dirid: &Self::Handle, filename: &filename3<'_>) -> Result<(), nfsstat3> {
        let dir_path = self.path(*dirid)?;
        let file_path = dir_path.join(filename.as_os_str());

        // Check if it's a file or directory
        (async || {
            let metadata = tokio::fs::symlink_metadata(&file_path).await?;
            if metadata.is_dir() {
                tokio::fs::remove_dir(&file_path).await
            } else {
                tokio::fs::remove_file(&file_path).await
            }
        })()
        .await
        .map_err(map_io_error)
    }

    async fn rename<'a>(
        &self,
        from_dirid: &Self::Handle,
        from_filename: &filename3<'a>,
        to_dirid: &Self::Handle,
        to_filename: &filename3<'a>,
    ) -> Result<(), nfsstat3> {
        // Validate filenames - "." and ".." are not allowed
        let from_name = from_filename.as_os_str();
        let to_name = to_filename.as_os_str();

        if from_name == "." || from_name == ".." || to_name == "." || to_name == ".." {
            return Err(nfsstat3::NFS3ERR_INVAL);
        }

        let from_dir_path = self.path(*from_dirid)?;
        let from_path = from_dir_path.join(from_name);

        let to_dir_path = self.path(*to_dirid)?;
        let to_path = to_dir_path.join(to_name);

        // Check if source and target are the same (no-op)
        if from_dirid == to_dirid && from_filename == to_filename {
            return Ok(());
        }

        // Check source exists
        let from_metadata = tokio::fs::symlink_metadata(&from_path)
            .await
            .map_err(map_io_error)?;

        // Check if target exists
        if let Ok(to_metadata) = tokio::fs::symlink_metadata(&to_path).await {
            // Both must be compatible types
            if from_metadata.is_dir() != to_metadata.is_dir() {
                return Err(nfsstat3::NFS3ERR_EXIST);
            }

            // If target is a directory, it must be empty
            if to_metadata.is_dir() {
                let mut read_dir = tokio::fs::read_dir(&to_path).await.map_err(map_io_error)?;
                if read_dir.next_entry().await.map_err(map_io_error)?.is_some() {
                    return Err(nfsstat3::NFS3ERR_NOTEMPTY);
                }
            }
        }

        // Perform the rename
        tokio::fs::rename(&from_path, &to_path)
            .await
            .map_err(map_io_error)?;

        // Invalidate cache entries
        let _ = self
            .cache
            .lookup_by_id(*from_dirid, from_filename.as_os_str(), false);
        let _ = self
            .cache
            .lookup_by_id(*to_dirid, to_filename.as_os_str(), false);

        Ok(())
    }

    async fn symlink<'a>(
        &self,
        dirid: &Self::Handle,
        linkname: &filename3<'a>,
        symlink: &nfspath3<'a>,
        attr: &sattr3,
    ) -> Result<(Self::Handle, fattr3), nfsstat3> {
        let dir_path = self.path(*dirid)?;
        let link_path = dir_path.join(linkname.as_os_str());
        let target_path = symlink.0.as_os_str();

        #[cfg(unix)]
        {
            tokio::fs::symlink(target_path, &link_path)
                .await
                .map_err(map_io_error)?;
        }

        #[cfg(windows)]
        {
            // On Windows, we need to determine if target is a file or directory
            // For simplicity, we'll try as a file first
            tokio::fs::symlink_file(target_path, &link_path)
                .await
                .map_err(map_io_error)?;
        }

        // Register the symlink in the cache
        let link_id = self
            .cache
            .lookup_by_id(*dirid, linkname.as_os_str(), true)?;

        // Apply attributes if specified (mode, uid, gid)
        if !matches!(attr.mode, set_mode3::None)
            || !matches!(attr.uid, set_uid3::None)
            || !matches!(attr.gid, set_gid3::None)
        {
            // Note: Setting attributes on symlinks is tricky, many systems don't support it
            // We'll try but ignore errors
            let _ = self.setattr(&link_id, attr.clone()).await;
        }

        let fattr = self.getattr(&link_id).await?;
        Ok((link_id, fattr))
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
                NextResult::Err(e) => panic!("Unexpected error: {e}"),
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
            NextResult::Err(e) => panic!("Unexpected error: {e}"),
        };

        let resume_cookie = first_entry.cookie;
        drop(iter_partial);

        let mut iter2 = fs
            .readdir(&root_handle, resume_cookie)
            .await
            .expect("Should succeed with cached cookie");
        match iter2.next().await {
            NextResult::Ok(_) | NextResult::Eof => {}
            NextResult::Err(e) => panic!("Should not fail with cached cookie: {e}"),
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
                NextResult::Err(e) => panic!("Unexpected error during streaming: {e}"),
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
                NextResult::Err(e) => panic!("Unexpected error during streaming: {e}"),
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
                    NextResult::Err(e) => panic!("Unexpected error: {e}"),
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
