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

use super::{IteratorCache, SymbolsCache};
use crate::string_ext::FromOsString;

#[derive(Debug)]
pub struct MirrorFsIterator {
    root_path: PathBuf,
    cache: Arc<SymbolsCache>,
    dirid: FileHandleU64,
    read_dir: Option<ReadDir>,
    /// Cookie starts as base (with unique counter) and gets incremented for each entry
    cookie: u64,
    /// Direct reference to the iterator cache for Drop implementation
    iterator_cache: Arc<IteratorCache>,
}

impl MirrorFsIterator {
    pub const fn new(
        root_path: PathBuf,
        cache: Arc<SymbolsCache>,
        iterator_cache: Arc<IteratorCache>,
        dirid: FileHandleU64,
        read_dir: Option<ReadDir>,
        cookie: u64,
    ) -> Self {
        Self {
            root_path,
            cache,
            dirid,
            read_dir,
            cookie,
            iterator_cache,
        }
    }

    /// Common logic for getting the next directory entry and creating a handle
    async fn next_entry_common(&mut self) -> NextResult<(FileHandleU64, std::ffi::OsString, u64)> {
        let Some(read_dir) = self.read_dir.as_mut() else {
            return NextResult::Eof;
        };

        match read_dir.next_entry().await {
            Ok(Some(entry)) => {
                let name = entry.file_name();

                let handle = match self.cache.symbols_path(self.dirid) {
                    Ok(parent_symbols) => match self.cache.lookup(&parent_symbols, &name, false) {
                        Ok(handle) => handle,
                        Err(e) => return NextResult::Err(e),
                    },
                    Err(e) => return NextResult::Err(e),
                };

                self.cookie += 1;

                NextResult::Ok((handle, name, self.cookie))
            }
            Ok(None) => {
                self.read_dir = None;
                NextResult::Eof
            }
            Err(_) => NextResult::Err(nfsstat3::NFS3ERR_IO),
        }
    }
}

impl Drop for MirrorFsIterator {
    fn drop(&mut self) {
        if let Some(read_dir) = self.read_dir.take() {
            self.iterator_cache
                .cache_state(self.dirid, self.cookie, read_dir, Instant::now());
            debug!(
                "Cached iterator state for dir_id: {} at cookie: {:#018x}",
                self.dirid.as_u64(),
                self.cookie
            );
        }
    }
}

impl ReadDirIterator for MirrorFsIterator {
    async fn next(&mut self) -> NextResult<DirEntry> {
        match self.next_entry_common().await {
            NextResult::Ok((handle, name, cookie)) => {
                let dir_entry = DirEntry {
                    fileid: handle.as_u64(),
                    name: FromOsString::from_os_string(name),
                    cookie,
                };
                NextResult::Ok(dir_entry)
            }
            NextResult::Err(e) => NextResult::Err(e),
            NextResult::Eof => NextResult::Eof,
        }
    }
}

impl ReadDirPlusIterator<FileHandleU64> for MirrorFsIterator {
    async fn next(&mut self) -> NextResult<DirEntryPlus<FileHandleU64>> {
        loop {
            match self.next_entry_common().await {
                NextResult::Ok((handle, name, cookie)) => {
                    let path = {
                        if let Ok(relative_path) = self.cache.handle_to_path(handle) {
                            self.root_path.join(&relative_path)
                        } else {
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

                    let dir_entry_plus = DirEntryPlus {
                        fileid: handle.as_u64(),
                        name: FromOsString::from_os_string(name),
                        cookie,
                        name_attributes: fattr,
                        name_handle: Some(handle),
                    };

                    return NextResult::Ok(dir_entry_plus);
                }
                NextResult::Err(e) => return NextResult::Err(e),
                NextResult::Eof => return NextResult::Eof,
            }
        }
    }
}
