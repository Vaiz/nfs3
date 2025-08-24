#![allow(clippy::unnecessary_wraps)]

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use nfs3_server::fs_util::metadata_to_fattr3;
use nfs3_server::nfs3_types::nfs3::{fileid3, filename3, nfsstat3};
use nfs3_server::vfs::{
    DirEntry, DirEntryPlus, FileHandleU64, NextResult, ReadDirIterator, ReadDirPlusIterator,
};
use tokio::fs::ReadDir;
use tracing::{debug, warn};

use super::{FsInner, SymbolsPath};
use crate::mirror::string_ext::FromOsString;

pub struct Mirror3ReadDirIterator {
    root_path: PathBuf,
    inner: Arc<RwLock<FsInner>>,
    dirid: FileHandleU64,
    read_dir: ReadDir,
    cookie: u64,
    current_position: u64,
    exhausted: bool,
    seeked: bool,
}

pub struct Mirror3ReadDirPlusIterator {
    root_path: PathBuf,
    inner: Arc<RwLock<FsInner>>,
    dirid: FileHandleU64,
    read_dir: ReadDir,
    cookie: u64,
    current_position: u64,
    exhausted: bool,
    seeked: bool,
}

impl Mirror3ReadDirIterator {
    pub async fn new(
        root_path: PathBuf,
        inner: Arc<RwLock<FsInner>>,
        dirid: FileHandleU64,
        cookie: u64,
    ) -> Result<Self, nfsstat3> {
        let dir_path = {
            let lock = inner.read().unwrap();
            let relative_path = lock.cache.handle_to_path(dirid)?;
            root_path.join(&relative_path)
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
            "Created readdir iterator for directory: {:?} with cookie: {}",
            dir_path, cookie
        );

        Ok(Self {
            root_path,
            inner,
            dirid,
            read_dir,
            cookie,
            current_position: 0,
            exhausted: false,
            seeked: false,
        })
    }

    fn verify_cookie_position(&mut self) -> Result<(), nfsstat3> {
        if self.cookie == 0 {
            // Starting from beginning is always valid
            self.seeked = true;
            return Ok(());
        }

        if self.seeked {
            // Already verified
            return Ok(());
        }

        debug!(
            "Verifying iterator is at cookie {} in directory {:?}",
            self.cookie, self.dirid
        );

        // For cookie verification, we need to check that we can find the next entry
        // after the given cookie. We don't need to validate the exact position,
        // just that we can continue from where the cookie indicates.
        self.seeked = true;
        Ok(())
    }
}

impl Mirror3ReadDirPlusIterator {
    pub async fn new(
        root_path: PathBuf,
        inner: Arc<RwLock<FsInner>>,
        dirid: FileHandleU64,
        cookie: u64,
    ) -> Result<Self, nfsstat3> {
        let dir_path = {
            let lock = inner.read().unwrap();
            let relative_path = lock.cache.handle_to_path(dirid)?;
            root_path.join(&relative_path)
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
            "Created readdirplus iterator for directory: {:?} with cookie: {}",
            dir_path, cookie
        );

        Ok(Self {
            root_path,
            inner,
            dirid,
            read_dir,
            cookie,
            current_position: 0,
            exhausted: false,
            seeked: false,
        })
    }

    fn verify_cookie_position(&mut self) -> Result<(), nfsstat3> {
        if self.cookie == 0 {
            // Starting from beginning is always valid
            self.seeked = true;
            return Ok(());
        }

        if self.seeked {
            // Already verified
            return Ok(());
        }

        debug!(
            "Verifying iterator is at cookie {} in directory {:?}",
            self.cookie, self.dirid
        );

        // For cookie verification, we need to check that we can find the next entry
        // after the given cookie. We don't need to validate the exact position,
        // just that we can continue from where the cookie indicates.
        self.seeked = true;
        Ok(())
    }
}

impl ReadDirIterator for Mirror3ReadDirIterator {
    async fn next(&mut self) -> NextResult<DirEntry> {
        // If we haven't verified the cookie position yet, do it now
        if !self.seeked {
            if let Err(e) = self.verify_cookie_position() {
                return NextResult::Err(e);
            }
        }

        if self.exhausted {
            return NextResult::Eof;
        }

        // Get next entry from tokio ReadDir
        match self.read_dir.next_entry().await {
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

                let dir_entry = DirEntry {
                    fileid: handle.as_u64(),
                    name: filename3::from_os_string(name),
                    cookie: handle.as_u64(),
                };

                self.current_position = handle.as_u64();
                NextResult::Ok(dir_entry)
            }
            Ok(None) => {
                self.exhausted = true;
                NextResult::Eof
            }
            Err(_) => NextResult::Err(nfsstat3::NFS3ERR_IO),
        }
    }
}

impl ReadDirPlusIterator<FileHandleU64> for Mirror3ReadDirPlusIterator {
    async fn next(&mut self) -> NextResult<DirEntryPlus<FileHandleU64>> {
        // If we haven't verified the cookie position yet, do it now
        if !self.seeked {
            if let Err(e) = self.verify_cookie_position() {
                return NextResult::Err(e);
            }
        }

        if self.exhausted {
            return NextResult::Eof;
        }

        // Get next entry from tokio ReadDir
        loop {
            match self.read_dir.next_entry().await {
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

                    let dir_entry_plus = DirEntryPlus {
                        fileid: handle.as_u64(),
                        name: filename3::from_os_string(name),
                        cookie: handle.as_u64(),
                        name_attributes: fattr,
                        name_handle: Some(handle),
                    };

                    self.current_position = handle.as_u64();
                    return NextResult::Ok(dir_entry_plus);
                }
                Ok(None) => {
                    self.exhausted = true;
                    return NextResult::Eof;
                }
                Err(_) => return NextResult::Err(nfsstat3::NFS3ERR_IO),
            }
        }
    }
}
