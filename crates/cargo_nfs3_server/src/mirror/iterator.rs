#![allow(clippy::unnecessary_wraps)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use nfs3_server::fs_util::metadata_to_fattr3;
use nfs3_server::nfs3_types::nfs3::nfsstat3;
use nfs3_server::vfs::{
    DirEntry, DirEntryPlus, FileHandleU64, NextResult, ReadDirIterator, ReadDirPlusIterator,
};
use tokio::fs::ReadDir;
use tracing::debug;

use super::SymbolsCache;
use crate::mirror::iterator_cache::IteratorCache;
use crate::string_ext::FromOsString;

#[derive(Debug)]
pub struct Mirror3DirIterator {
    root_path: PathBuf,
    cache: Arc<SymbolsCache>,
    dirid: FileHandleU64,
    read_dir: Option<ReadDir>,
    cookie: u64,
    /// Direct reference to the iterator cache for Drop implementation
    iterator_cache: Arc<IteratorCache>,
}

impl Mirror3DirIterator {
    pub async fn new(
        root_path: PathBuf,
        cache: Arc<SymbolsCache>,
        iterator_cache: Arc<IteratorCache>,
        dirid: FileHandleU64,
        cookie: u64,
    ) -> Result<Self, nfsstat3> {
        let dir_path = {
            let relative_path = cache.handle_to_path(dirid)?;
            root_path.join(&relative_path)
        };

        // Check if it's a directory
        let metadata = tokio::fs::symlink_metadata(&dir_path)
            .await
            .map_err(|_| nfsstat3::NFS3ERR_NOENT)?;
        if !metadata.is_dir() {
            return Err(nfsstat3::NFS3ERR_NOTDIR);
        }

        // Follow the three simple rules for iterator caching:
        let (read_dir, current_cookie) = if cookie == 0 {
            // Rule 1: Cookie is 0 - create a new iterator
            debug!(
                "Creating new ReadDir for directory: {:?} (cookie = 0)",
                dir_path
            );
            let read_dir = tokio::fs::read_dir(&dir_path)
                .await
                .map_err(|_| nfsstat3::NFS3ERR_IO)?;
            (Some(read_dir), 0)
        } else {
            // Cookie is not zero - check cache
            if let Some(cached_info) = iterator_cache.pop_state(dirid, cookie) {
                // Rule 2: Cookie is not zero and iterator with same cookie exists in cache - continue to iterate
                debug!(
                    "Reusing cached ReadDir for directory: {:?} at cookie: {} (cached position: {})",
                    dir_path, cookie, cached_info.current_position
                );
                (cached_info.read_dir, cached_info.current_position)
            } else {
                // Rule 3: Cookie is not zero and iterator with same cookie doesn't exist in cache - return BAD_COOKIE
                debug!(
                    "No cached ReadDir found for cookie {}, returning BAD_COOKIE error",
                    cookie
                );
                return Err(nfsstat3::NFS3ERR_BAD_COOKIE);
            }
        };

        Ok(Self {
            root_path,
            cache,
            dirid,
            read_dir,
            cookie: current_cookie,
            iterator_cache,
        })
    }
}

impl Drop for Mirror3DirIterator {
    fn drop(&mut self) {
        // Cache the ReadDir object if we're not exhausted and have one
        if let Some(read_dir) = self.read_dir.take() {
            // Cache the current ReadDir object with the current cookie position
            // The cookie represents the position where the next read would start from
            self.iterator_cache.cache_state(
                self.dirid,
                self.cookie,
                Some(read_dir),
                self.cookie,
                Instant::now(),
            );
            debug!(
                "Cached iterator state for dir_id: {} at cookie: {}",
                self.dirid.as_u64(),
                self.cookie
            );
        }
    }
}

impl ReadDirIterator for Mirror3DirIterator {
    async fn next(&mut self) -> NextResult<DirEntry> {
        let Some(read_dir) = self.read_dir.as_mut() else {
            return NextResult::Eof;
        };

        // Get next entry from tokio ReadDir
        match read_dir.next_entry().await {
            Ok(Some(entry)) => {
                let name = entry.file_name();

                // Create handle for this entry
                let handle = {
                    match self.cache.symbols_path(self.dirid) {
                        Ok(parent_symbols) => {
                            match self.cache.lookup(&parent_symbols, &name, false) {
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
                    name: FromOsString::from_os_string(name),
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
        let Some(read_dir) = self.read_dir.as_mut() else {
            return NextResult::Eof;
        };

        // Get next entry from tokio ReadDir
        loop {
            match read_dir.next_entry().await {
                Ok(Some(entry)) => {
                    let name = entry.file_name();

                    // Create handle for this entry
                    let handle = {
                        match self.cache.symbols_path(self.dirid) {
                            Ok(parent_symbols) => {
                                match self.cache.lookup(&parent_symbols, &name, false) {
                                    Ok(handle) => handle,
                                    Err(e) => return NextResult::Err(e),
                                }
                            }
                            Err(e) => return NextResult::Err(e),
                        }
                    };

                    // Get file attributes
                    let path = {
                        if let Ok(relative_path) = self.cache.handle_to_path(handle) {
                            self.root_path.join(&relative_path)
                        } else {
                            // Skip if handle is invalid, continue to next entry
                            debug!("Invalid handle for entry: {:?}", handle);
                            continue;
                        }
                    };

                    let fattr = tokio::fs::symlink_metadata(&path).await.map_or_else(
                        |_| {
                            debug!("Failed to get metadata for: {:?}", path);
                            None
                        },
                        |metadata| Some(metadata_to_fattr3(handle.as_u64(), &metadata)),
                    );

                    self.cookie += 1;
                    let dir_entry_plus = DirEntryPlus {
                        fileid: handle.as_u64(),
                        name: FromOsString::from_os_string(name),
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
