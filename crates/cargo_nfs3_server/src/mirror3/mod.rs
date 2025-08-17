#![allow(unused)] // FIXME
#![allow(clippy::unwrap_used)] // For experimental code

use std::borrow::Cow;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use intaglio::{Symbol, path};
use nfs3_server::fs_util::metadata_to_fattr3;
use nfs3_server::nfs3_types::nfs3::{fattr3, fileid3, filename3, ftype3, nfspath3, nfsstat3};
use nfs3_server::vfs::{
    DirEntry, DirEntryPlus, FileHandleU64, NextResult, NfsReadFileSystem, ReadDirIterator,
    ReadDirPlusIterator,
};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};

use crate::mirror::string_ext::{FromOsString, IntoOsString};

pub struct OsStrRef<'a>(Cow<'a, OsStr>);

impl Deref for OsStrRef<'_> {
    type Target = OsStr;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

impl From<OsStrRef<'_>> for Cow<'static, OsStr> {
    fn from(val: OsStrRef<'_>) -> Self {
        Cow::Owned(val.0.to_os_string())
    }
}

// TODO: use faster cache
// TODO: compress vec of symbols
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct SymbolsPath(Vec<Symbol>);

impl SymbolsPath {
    pub const fn new() -> Self {
        Self(Vec::new())
    }

    pub fn push(&mut self, symbol: Symbol) {
        self.0.push(symbol);
    }

    pub fn join(&self, symbol: Symbol) -> Self {
        let mut new_path = self.clone();
        new_path.0.push(symbol);
        new_path
    }
}

pub struct Entry {
    // path: Path
    // name: Symbol
    // type_: ftype3,
}

#[derive(Debug, Default)]
pub struct SymbolsTable {
    table: intaglio::osstr::SymbolTable,
}

impl SymbolsTable {
    fn new() -> Self {
        Self {
            table: intaglio::osstr::SymbolTable::new(),
        }
    }

    fn insert_or_resolve(&mut self, name: Cow<OsStr>) -> Symbol {
        let name = OsStrRef(name);
        self.table.intern(name).expect("symbols table full")
    }

    fn get(&self, symbol: Symbol) -> Option<&OsStr> {
        self.table.get(symbol)
    }

    // SymbolsPath expected to be always valid
    fn resolve_path(&self, path: &SymbolsPath) -> PathBuf {
        let mut path_buf = PathBuf::new();
        for symbol in &path.0 {
            let node = self.table.get(*symbol).expect("symbol not found");
            path_buf.push(node);
        }
        path_buf
    }
}

pub struct Cache {
    root: PathBuf,
    symbols: SymbolsTable,
    path_to_id: HashMap<SymbolsPath, FileHandleU64>,
    id_to_path: HashMap<FileHandleU64, SymbolsPath>,
    next_id: AtomicU64,
}

impl Cache {
    const ROOT_ID: FileHandleU64 = FileHandleU64::new(1);

    fn new(root: PathBuf) -> Self {
        let mut cache = Self {
            root,
            symbols: SymbolsTable::new(),
            path_to_id: HashMap::new(),
            id_to_path: HashMap::new(),
            next_id: AtomicU64::new(2), // Start from 2 since 1 is root
        };

        // Insert root entry
        let root_path = SymbolsPath::new();
        cache.path_to_id.insert(root_path.clone(), Self::ROOT_ID);
        cache.id_to_path.insert(Self::ROOT_ID, root_path);

        cache
    }

    #[allow(clippy::unused_self)]
    const fn root(&self) -> FileHandleU64 {
        Self::ROOT_ID
    }

    fn symbols_path(&self, id: FileHandleU64) -> Result<&SymbolsPath, nfsstat3> {
        self.id_to_path.get(&id).ok_or(nfsstat3::NFS3ERR_BADHANDLE)
    }

    // returns relative path
    fn handle_to_path(&self, id: FileHandleU64) -> Result<PathBuf, nfsstat3> {
        let path = self.symbols_path(id)?;
        Ok(self.symbols.resolve_path(path))
    }

    fn lookup_by_id(
        &mut self,
        parent_id: FileHandleU64,
        name: &OsStr,
        check_path: bool,
    ) -> Result<FileHandleU64, nfsstat3> {
        let parent = self.symbols_path(parent_id)?.clone();
        self.lookup(&parent, name, check_path)
    }

    /// To avoid cache thrashing, we only insert the new entry if object exists
    fn lookup(
        &mut self,
        parent: &SymbolsPath,
        name: &OsStr,
        check_path: bool,
    ) -> Result<FileHandleU64, nfsstat3> {
        use std::collections::hash_map::Entry;
        use std::sync::atomic::Ordering;

        let symbol = self.symbols.insert_or_resolve(name.into());
        let mut entry = self.path_to_id.entry(parent.join(symbol));
        match entry {
            Entry::Occupied(occupied_entry) => Ok(*occupied_entry.get()),
            Entry::Vacant(vacant_entry) => {
                if check_path {
                    let mut test_path = self.root.join(self.symbols.resolve_path(parent));
                    if !test_path.exists() {
                        return Err(nfsstat3::NFS3ERR_BADHANDLE);
                    }
                    test_path.push(name);
                    if !test_path.exists() {
                        return Err(nfsstat3::NFS3ERR_NOENT);
                    }
                }
                let id = FileHandleU64::new(self.next_id.fetch_add(1, Ordering::Relaxed));
                vacant_entry.insert(id);
                self.id_to_path.insert(id, parent.join(symbol));
                Ok(id)
            }
        }
    }
}

pub struct FsInner {
    root: PathBuf,
    cache: Cache,
}

pub struct Fs {
    inner: std::sync::Arc<std::sync::RwLock<FsInner>>,
}

impl Fs {
    pub fn new(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref().to_path_buf();
        let cache = Cache::new(root.clone());
        let fs_inner = FsInner { root, cache };
        Self {
            inner: std::sync::Arc::new(std::sync::RwLock::new(fs_inner)),
        }
    }
}

impl NfsReadFileSystem for Fs {
    type Handle = FileHandleU64;

    fn root_dir(&self) -> Self::Handle {
        self.inner.read().unwrap().cache.root()
    }

    async fn lookup(
        &self,
        dirid: &Self::Handle,
        filename: &filename3<'_>,
    ) -> Result<Self::Handle, nfsstat3> {
        let mut lock = self.inner.write().unwrap();
        lock.cache.lookup_by_id(*dirid, filename.as_os_str(), true)
    }

    async fn getattr(&self, id: &Self::Handle) -> Result<fattr3, nfsstat3> {
        let path = {
            let mut lock = self.inner.write().unwrap();
            let path = lock.cache.handle_to_path(id.as_u64().into())?;
            lock.root.join(&path)
        };

        let metadata = tokio::fs::symlink_metadata(&path)
            .await
            .map_err(|_| nfsstat3::NFS3ERR_NOENT)?;

        Ok(metadata_to_fattr3(id.as_u64(), &metadata))
    }

    #[allow(clippy::cast_possible_truncation)]
    async fn read(
        &self,
        id: &Self::Handle,
        offset: u64,
        count: u32,
    ) -> Result<(Vec<u8>, bool), nfsstat3> {
        let path = {
            let mut lock = self.inner.write().unwrap();
            let path = lock.cache.handle_to_path(id.as_u64().into())?;
            lock.root.join(&path)
        };

        let mut f = File::open(&path).await.or(Err(nfsstat3::NFS3ERR_NOENT))?;
        let len = f.metadata().await.or(Err(nfsstat3::NFS3ERR_NOENT))?.len();
        let mut start = offset;
        let mut end = offset + u64::from(count);
        let eof = end >= len;
        if start >= len {
            start = len;
        }
        if end > len {
            end = len;
        }
        f.seek(SeekFrom::Start(start))
            .await
            .or(Err(nfsstat3::NFS3ERR_IO))?;
        let mut buf = vec![0; (end - start) as usize];
        f.read_exact(&mut buf).await.or(Err(nfsstat3::NFS3ERR_IO))?;
        Ok((buf, eof))
    }

    async fn readdirplus(
        &self,
        dirid: &Self::Handle,
        cookie: u64,
    ) -> Result<impl ReadDirPlusIterator<Self::Handle>, nfsstat3> {
        Mirror3Iterator::new(Arc::clone(&self.inner), dirid, cookie).await
    }

    async fn readlink(&self, id: &Self::Handle) -> Result<nfspath3<'_>, nfsstat3> {
        let path = {
            let mut lock = self.inner.write().unwrap();
            let path = lock.cache.handle_to_path(id.as_u64().into())?;
            lock.root.join(&path)
        };

        if path.is_symlink() {
            path.read_link()
                .map_or(Err(nfsstat3::NFS3ERR_IO), |target| {
                    Ok(nfspath3::from_os_str(target.as_os_str()))
                })
        } else {
            Err(nfsstat3::NFS3ERR_BADTYPE)
        }
    }
}

pub struct Mirror3Iterator {
    inner: std::sync::Arc<std::sync::RwLock<FsInner>>,
    entries: Vec<(FileHandleU64, std::ffi::OsString)>,
    index: usize,
}

impl Mirror3Iterator {
    async fn new(
        inner: std::sync::Arc<std::sync::RwLock<FsInner>>,
        dirid: &FileHandleU64,
        cookie: u64,
    ) -> Result<Self, nfsstat3> {
        let dir_path = {
            let mut lock = inner.write().unwrap();
            let path = lock.cache.handle_to_path(*dirid)?;
            lock.root.join(&path)
        };

        // Check if it's a directory
        let metadata = tokio::fs::symlink_metadata(&dir_path)
            .await
            .map_err(|_| nfsstat3::NFS3ERR_NOENT)?;
        if !metadata.is_dir() {
            return Err(nfsstat3::NFS3ERR_NOTDIR);
        }

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
                let parent_symbols = lock.cache.symbols_path(*dirid)?.clone();
                lock.cache.lookup(&parent_symbols, &name, false)?
            };
            entries.push((handle, name));
        }

        // Sort by file ID for consistent ordering
        entries.sort_by_key(|(handle, _)| handle.as_u64());

        // Skip entries based on cookie
        let start_index = if cookie == 0 {
            0
        } else {
            entries
                .iter()
                .position(|(handle, _)| handle.as_u64() > cookie)
                .unwrap_or(entries.len())
        };

        Ok(Self {
            inner,
            entries: entries.into_iter().skip(start_index).collect(),
            index: 0,
        })
    }
}

impl ReadDirIterator for Mirror3Iterator {
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

impl ReadDirPlusIterator<FileHandleU64> for Mirror3Iterator {
    async fn next(&mut self) -> NextResult<DirEntryPlus<FileHandleU64>> {
        loop {
            if self.index >= self.entries.len() {
                return NextResult::Eof;
            }

            let (handle, name) = &self.entries[self.index];
            self.index += 1;

            // Get file attributes
            let path = {
                let mut lock = self.inner.write().unwrap();
                match lock.cache.handle_to_path(*handle) {
                    Ok(p) => lock.root.join(&p),
                    Err(_) => {
                        // Skip if handle is invalid, continue to next entry
                        continue;
                    }
                }
            };

            let fattr = (tokio::fs::symlink_metadata(&path).await).map_or(None, |metadata| {
                Some(metadata_to_fattr3(handle.as_u64(), &metadata))
            });

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
