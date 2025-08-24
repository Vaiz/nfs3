use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use nfs3_server::fs_util::metadata_to_fattr3;
use nfs3_server::nfs3_types::nfs3::{fileid3, filename3, nfsstat3};
use nfs3_server::vfs::{
    DirEntry, DirEntryPlus, FileHandleU64, NextResult, ReadDirIterator, ReadDirPlusIterator,
};
use tracing::{debug, warn};

use super::{FsInner, IteratorState, SymbolsPath};
use crate::mirror::string_ext::FromOsString;

pub struct Mirror3ReadDirIterator {
    entries: Vec<(FileHandleU64, std::ffi::OsString)>,
    index: usize,
}

pub struct Mirror3ReadDirPlusIterator {
    root_path: PathBuf,
    inner: Arc<RwLock<FsInner>>,
    entries: Vec<(FileHandleU64, std::ffi::OsString)>,
    index: usize,
}

impl Mirror3ReadDirIterator {
    pub async fn new(
        root_path: PathBuf,
        inner: Arc<RwLock<FsInner>>,
        dirid: FileHandleU64,
        cookie: u64,
    ) -> Result<Self, nfsstat3> {
        let entries = create_entries(root_path, inner, dirid, cookie).await?;
        Ok(Self { entries, index: 0 })
    }
}

impl Mirror3ReadDirPlusIterator {
    pub async fn new(
        root_path: PathBuf,
        inner: Arc<RwLock<FsInner>>,
        dirid: FileHandleU64,
        cookie: u64,
    ) -> Result<Self, nfsstat3> {
        let entries = create_entries(root_path.clone(), Arc::clone(&inner), dirid, cookie).await?;
        Ok(Self {
            root_path,
            inner,
            entries,
            index: 0,
        })
    }
}

async fn create_entries(
    root_path: PathBuf,
    inner: Arc<RwLock<FsInner>>,
    dirid: FileHandleU64,
    cookie: u64,
) -> Result<Vec<(FileHandleU64, std::ffi::OsString)>, nfsstat3> {
    let dir_path = {
        let lock = inner.read().unwrap();
        let relative_path = lock.cache.handle_to_path(dirid)?;
        root_path.join(&relative_path)
    };

    // Check if it's a directory and get its modification time
    let metadata = tokio::fs::symlink_metadata(&dir_path)
        .await
        .map_err(|_| nfsstat3::NFS3ERR_NOENT)?;
    if !metadata.is_dir() {
        return Err(nfsstat3::NFS3ERR_NOTDIR);
    }

    let dir_mtime = metadata
        .modified()
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);

    // Check if we have a cached iterator for this directory
    let iterator_cache = {
        let lock = inner.read().unwrap();
        lock.iterator_cache.clone()
    };
    let cached_state = iterator_cache.get(&dirid).await;

    let iterator_state = if let Some(state) = cached_state {
        if state.is_valid(dir_mtime) {
            // Validate the cookie against cached entries
            if !state.validate_cookie(cookie) {
                warn!(
                    "Invalid cookie {} for directory {:?}, cookie not found in cached entries",
                    cookie, dirid
                );
                return Err(nfsstat3::NFS3ERR_BAD_COOKIE);
            }
            debug!(
                "Using cached iterator state for directory {:?} with cookie {}",
                dirid, cookie
            );
            state
        } else {
            debug!(
                "Cached iterator state for directory {:?} is stale, refreshing",
                dirid
            );
            // Directory was modified, need to refresh
            let new_state = create_fresh_iterator_state(
                root_path,
                Arc::clone(&inner),
                dirid,
                dir_path,
                dir_mtime,
            )
            .await?;

            // Validate cookie against fresh entries
            if !new_state.validate_cookie(cookie) {
                warn!(
                    "Invalid cookie {} for directory {:?} after refresh",
                    cookie, dirid
                );
                return Err(nfsstat3::NFS3ERR_BAD_COOKIE);
            }

            // Cache the new state
            iterator_cache.insert(dirid, new_state.clone()).await;
            new_state
        }
    } else {
        debug!(
            "No cached iterator state for directory {:?}, creating fresh",
            dirid
        );
        // No cached state, create fresh
        let new_state =
            create_fresh_iterator_state(root_path, Arc::clone(&inner), dirid, dir_path, dir_mtime)
                .await?;

        // Validate cookie against fresh entries
        if !new_state.validate_cookie(cookie) {
            warn!(
                "Invalid cookie {} for directory {:?} in fresh iterator",
                cookie, dirid
            );
            return Err(nfsstat3::NFS3ERR_BAD_COOKIE);
        }

        // Cache the new state
        iterator_cache.insert(dirid, new_state.clone()).await;
        new_state
    };

    Ok(iterator_state.get_entries_after_cookie(cookie))
}

async fn create_fresh_iterator_state(
    root_path: PathBuf,
    inner: Arc<RwLock<FsInner>>,
    dirid: FileHandleU64,
    dir_path: PathBuf,
    dir_mtime: std::time::SystemTime,
) -> Result<IteratorState, nfsstat3> {
    debug!("Reading directory: {:?}", dir_path);

    // Read directory entries
    let mut read_dir = tokio::fs::read_dir(&dir_path)
        .await
        .map_err(|_| nfsstat3::NFS3ERR_IO)?;

    let mut entries = Vec::new();
    while let Some(entry) = read_dir
        .next_entry()
        .await
        .map_err(|_| nfsstat3::NFS3ERR_IO)?
    {
        let name = entry.file_name();
        // Create handle for this entry
        let handle = {
            let mut lock = inner.write().unwrap();
            // Use file name directly to create a new ID
            let parent_symbols = lock.cache.symbols_path(dirid)?.clone();
            lock.cache.lookup(&parent_symbols, &name, false)?
        };
        entries.push((handle, name));
    }

    // Sort by file ID for consistent ordering
    entries.sort_by_key(|(handle, _)| handle.as_u64());

    debug!("Found {} entries for directory {:?}", entries.len(), dirid);

    Ok(IteratorState::new(entries, dir_mtime))
}

impl ReadDirIterator for Mirror3ReadDirIterator {
    async fn next(&mut self) -> NextResult<DirEntry> {
        if self.index >= self.entries.len() {
            return NextResult::Eof;
        }

        let (handle, name) = &self.entries[self.index];
        self.index += 1;

        let dir_entry = DirEntry {
            fileid: handle.as_u64(),
            name: filename3::from_os_string(name.clone()),
            cookie: handle.as_u64(),
        };

        NextResult::Ok(dir_entry)
    }
}

impl ReadDirPlusIterator<FileHandleU64> for Mirror3ReadDirPlusIterator {
    async fn next(&mut self) -> NextResult<DirEntryPlus<FileHandleU64>> {
        loop {
            if self.index >= self.entries.len() {
                return NextResult::Eof;
            }

            let (handle, name) = &self.entries[self.index];
            self.index += 1;

            // Get file attributes
            let path = {
                let lock = self.inner.read().unwrap();
                if let Ok(relative_path) = lock.cache.handle_to_path(*handle) {
                    self.root_path.join(&relative_path)
                } else {
                    // Skip if handle is invalid, continue to next entry
                    debug!("Invalid handle for entry: {:?}", handle);
                    continue;
                }
            };

            let fattr = (tokio::fs::symlink_metadata(&path).await).map_or_else(
                |_| {
                    debug!("Failed to get metadata for: {:?}", path);
                    None
                },
                |metadata| Some(metadata_to_fattr3(handle.as_u64(), &metadata)),
            );

            let dir_entry_plus = DirEntryPlus {
                fileid: handle.as_u64(),
                name: filename3::from_os_string(name.clone()),
                cookie: handle.as_u64(),
                name_attributes: fattr,
                name_handle: Some(*handle),
            };

            return NextResult::Ok(dir_entry_plus);
        }
    }
}
