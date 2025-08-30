#![allow(clippy::unnecessary_wraps)]

use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use nfs3_server::fs_util::metadata_to_fattr3;
use nfs3_server::nfs3_types::nfs3::{fileid3, filename3, nfsstat3};
use nfs3_server::vfs::{
    DirEntry, DirEntryPlus, FileHandleU64, NextResult, ReadDirIterator, ReadDirPlusIterator,
};
use tokio::fs::ReadDir;
use tracing::{debug, warn};

use super::{FsInner, SymbolsPath};
use crate::mirror::string_ext::FromOsString;

#[derive(Debug)]
pub struct Mirror3DirIterator {
    root_path: PathBuf,
    inner: Arc<RwLock<FsInner>>,
    dirid: FileHandleU64,
    read_dir: Option<ReadDir>,
    cookie: u64,
    /// Direct reference to the iterator cache for Drop implementation
    iterator_cache: Arc<super::simple_iterator_cache::IteratorCache>,
}

impl Mirror3DirIterator {
    pub async fn new(
        root_path: PathBuf,
        inner: Arc<RwLock<FsInner>>,
        dirid: FileHandleU64,
        cookie: u64,
    ) -> Result<Self, nfsstat3> {
        let (dir_path, iterator_cache) = {
            let lock = inner.read().unwrap();
            let relative_path = lock.cache.handle_to_path(dirid)?;
            let iterator_cache = Arc::clone(&lock.iterator_cache);
            (root_path.join(&relative_path), iterator_cache)
        };

        // Check if it's a directory
        let metadata = tokio::fs::symlink_metadata(&dir_path)
            .await
            .map_err(|_| nfsstat3::NFS3ERR_NOENT)?;
        if !metadata.is_dir() {
            return Err(nfsstat3::NFS3ERR_NOTDIR);
        }

        // Open directory for reading
        let read_dir = tokio::fs::read_dir(&dir_path)
            .await
            .map_err(|_| nfsstat3::NFS3ERR_IO)?;

        debug!(
            "Created directory iterator for: {:?} with cookie: {}",
            dir_path, cookie
        );

        Ok(Self {
            root_path,
            inner,
            dirid,
            read_dir: Some(read_dir),
            cookie,
            iterator_cache,
        })
    }

    #[allow(clippy::missing_const_for_fn)]
    pub fn new_without_verification(
        root_path: PathBuf,
        inner: Arc<RwLock<FsInner>>,
        dirid: FileHandleU64,
        read_dir: ReadDir,
        cookie: u64,
    ) -> Self {
        let iterator_cache = {
            let lock = inner.read().unwrap();
            Arc::clone(&lock.iterator_cache)
        };
        
        Self {
            root_path,
            inner,
            dirid,
            read_dir: Some(read_dir),
            cookie,
            iterator_cache,
        }
    }
}

impl Drop for Mirror3DirIterator {
    fn drop(&mut self) {
        // Only cache if we're not exhausted (read_dir is Some)
        if self.read_dir.is_some() {
            // Cache the current position for potential future use
            self.iterator_cache.cache_state(
                self.dirid,
                self.cookie,
                Instant::now(),
            );
        }
    }
}

impl ReadDirIterator for Mirror3DirIterator {
    async fn next(&mut self) -> NextResult<DirEntry> {
        let Some(read_dir) = self.read_dir.as_mut() else { return NextResult::Eof };

        // Get next entry from tokio ReadDir
        match read_dir.next_entry().await {
            Ok(Some(entry)) => {
                let name = entry.file_name();

                // Create handle for this entry
                let handle = {
                    let mut lock = self.inner.write().unwrap();
                    match lock.cache.symbols_path(self.dirid) {
                        Ok(parent_symbols) => {
                            let parent_symbols = parent_symbols.clone();
                            match lock.cache.lookup(&parent_symbols, &name, false) {
                                Ok(handle) => handle,
                                Err(e) => return NextResult::Err(e),
                            }
                        }
                        Err(e) => return NextResult::Err(e),
                    }
                };

                self.cookie += 1;
                let dir_entry = DirEntry {
                    fileid: handle.as_u64(),
                    name: filename3::from_os_string(name),
                    cookie: self.cookie,
                };

                NextResult::Ok(dir_entry)
            }
            Ok(None) => {
                self.read_dir = None;
                NextResult::Eof
            }
            Err(_) => NextResult::Err(nfsstat3::NFS3ERR_IO),
        }
    }
}

impl ReadDirPlusIterator<FileHandleU64> for Mirror3DirIterator {
    async fn next(&mut self) -> NextResult<DirEntryPlus<FileHandleU64>> {
        let Some(read_dir) = self.read_dir.as_mut() else { return NextResult::Eof };

        // Get next entry from tokio ReadDir
        loop {
            match read_dir.next_entry().await {
                Ok(Some(entry)) => {
                    let name = entry.file_name();

                    // Create handle for this entry
                    let handle = {
                        let mut lock = self.inner.write().unwrap();
                        match lock.cache.symbols_path(self.dirid) {
                            Ok(parent_symbols) => {
                                let parent_symbols = parent_symbols.clone();
                                match lock.cache.lookup(&parent_symbols, &name, false) {
                                    Ok(handle) => handle,
                                    Err(e) => return NextResult::Err(e),
                                }
                            }
                            Err(e) => return NextResult::Err(e),
                        }
                    };

                    // Get file attributes
                    let path = {
                        let lock = self.inner.read().unwrap();
                        if let Ok(relative_path) = lock.cache.handle_to_path(handle) {
                            self.root_path.join(&relative_path)
                        } else {
                            // Skip if handle is invalid, continue to next entry
                            debug!("Invalid handle for entry: {:?}", handle);
                            continue;
                        }
                    };

                    let fattr = tokio::fs::symlink_metadata(&path)
                        .await
                        .map_or_else(
                            |_| {
                                debug!("Failed to get metadata for: {:?}", path);
                                None
                            },
                            |metadata| Some(metadata_to_fattr3(handle.as_u64(), &metadata)),
                        );

                    self.cookie += 1;
                    let dir_entry_plus = DirEntryPlus {
                        fileid: handle.as_u64(),
                        name: filename3::from_os_string(name),
                        cookie: self.cookie,
                        name_attributes: fattr,
                        name_handle: Some(handle),
                    };

                    return NextResult::Ok(dir_entry_plus);
                }
                Ok(None) => {
                    self.read_dir = None;
                    return NextResult::Eof;
                }
                Err(_) => return NextResult::Err(nfsstat3::NFS3ERR_IO),
            }
        }
    }
}
