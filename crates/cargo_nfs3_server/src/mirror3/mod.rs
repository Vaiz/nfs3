#![allow(unused)] // FIXME

use std::borrow::Cow;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;

use intaglio::{Symbol, path};
use nfs3_server::nfs3_types::nfs3::{fattr3, fileid3, filename3, ftype3, nfspath3, nfsstat3};
use nfs3_server::vfs::{FileHandleU64, NfsReadFileSystem, ReadDirPlusIterator};

use crate::mirror::string_ext::{FromOsString, IntoOsString};

pub struct OsStrRef<'a>(Cow<'a, OsStr>);

impl Deref for OsStrRef<'_> {
    type Target = OsStr;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

impl<'a> Into<Cow<'static, OsStr>> for OsStrRef<'a> {
    fn into(self) -> Cow<'static, OsStr> {
        Cow::Owned(self.0.to_os_string())
    }
}

// TODO: use faster cache
// TODO: compress vec of symbols
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct SymbolsPath(Vec<Symbol>);

impl SymbolsPath {
    pub fn new() -> Self {
        SymbolsPath(Vec::new())
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
        SymbolsTable {
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

    fn root(&self) -> FileHandleU64 {
        Self::ROOT_ID
    }

    fn symbols_path(&self, id: &FileHandleU64) -> Result<&SymbolsPath, nfsstat3> {
        self.id_to_path.get(id).ok_or(nfsstat3::NFS3ERR_BADHANDLE)
    }

    // returns relative path
    fn handle_to_path(&self, id: &FileHandleU64) -> Result<PathBuf, nfsstat3> {
        let path = self.symbols_path(id)?;
        Ok(self.symbols.resolve_path(path))
    }

    fn lookup_by_id(
        &mut self,
        parent_id: &FileHandleU64,
        name: &Cow<OsStr>,
        check_path: bool,
    ) -> Result<FileHandleU64, nfsstat3> {
        let parent = self.symbols_path(parent_id)?.clone();
        self.lookup(&parent, name, check_path)
    }

    /// To avoid cache thrashing, we only insert the new entry if object exists
    fn lookup(
        &mut self,
        parent: &SymbolsPath,
        name: &Cow<OsStr>,
        check_path: bool,
    ) -> Result<FileHandleU64, nfsstat3> {
        use std::collections::hash_map::Entry;
        use std::sync::atomic::Ordering;

        let symbol = self.symbols.insert_or_resolve(name.clone());
        let mut entry = self.path_to_id.entry(parent.join(symbol));
        match entry {
            Entry::Occupied(occupied_entry) => Ok(*occupied_entry.get()),
            Entry::Vacant(vacant_entry) => {
                if check_path {
                    let mut test_path = self.root.join(self.symbols.resolve_path(&parent));
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
        lock.cache
            .lookup_by_id(dirid, &filename.as_os_str().into(), true)
    }

    async fn getattr(&self, id: &Self::Handle) -> Result<fattr3, nfsstat3> {
        todo!()
    }

    async fn read(
        &self,
        id: &Self::Handle,
        offset: u64,
        count: u32,
    ) -> Result<(Vec<u8>, bool), nfsstat3> {
        todo!()
    }

    async fn readdirplus(
        &self,
        dirid: &Self::Handle,
        cookie: u64,
    ) -> Result<impl ReadDirPlusIterator<Self::Handle>, nfsstat3> {
        todo!()
    }

    async fn readlink(&self, id: &Self::Handle) -> Result<nfspath3<'_>, nfsstat3> {
        todo!()
    }
}
