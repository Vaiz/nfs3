use std::borrow::Cow;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use intaglio::Symbol;
use nfs3_server::nfs3_types::nfs3::nfsstat3;
use nfs3_server::vfs::FileHandleU64;

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

    pub fn join(&self, symbol: Symbol) -> Self {
        let mut new_path = self.clone();
        new_path.0.push(symbol);
        new_path
    }
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

    pub fn insert_or_resolve(&mut self, name: Cow<OsStr>) -> Symbol {
        let name = OsStrRef(name);
        self.table.intern(name).expect("symbols table full")
    }

    // SymbolsPath expected to be always valid
    pub fn resolve_path(&self, path: &SymbolsPath) -> PathBuf {
        let mut path_buf = PathBuf::new();
        for symbol in &path.0 {
            let node = self.table.get(*symbol).expect("symbol not found");
            path_buf.push(node);
        }
        path_buf
    }
}

#[derive(Debug)]
pub struct SymbolsCache {
    root: PathBuf,
    symbols: SymbolsTable,
    path_to_id: HashMap<SymbolsPath, FileHandleU64>,
    id_to_path: HashMap<FileHandleU64, SymbolsPath>,
    next_id: AtomicU64,
}

impl SymbolsCache {
    pub const ROOT_ID: FileHandleU64 = FileHandleU64::new(1);

    pub fn new(root: PathBuf) -> Self {
        let mut cache = Self {
            root,
            symbols: SymbolsTable::new(),
            path_to_id: HashMap::new(),
            id_to_path: HashMap::new(),
            next_id: AtomicU64::new(Self::ROOT_ID.as_u64() + 1),
        };

        // Insert root entry
        let root_path = SymbolsPath::new();
        cache.path_to_id.insert(root_path.clone(), Self::ROOT_ID);
        cache.id_to_path.insert(Self::ROOT_ID, root_path);

        cache
    }

    pub fn symbols_path(&self, id: FileHandleU64) -> Result<&SymbolsPath, nfsstat3> {
        self.id_to_path.get(&id).ok_or(nfsstat3::NFS3ERR_BADHANDLE)
    }

    // returns relative path
    pub fn handle_to_path(&self, id: FileHandleU64) -> Result<PathBuf, nfsstat3> {
        let path = self.symbols_path(id)?;
        Ok(self.symbols.resolve_path(path))
    }

    pub fn lookup_by_id(
        &mut self,
        parent_id: FileHandleU64,
        name: &OsStr,
        check_path: bool,
    ) -> Result<FileHandleU64, nfsstat3> {
        let parent = self.symbols_path(parent_id)?.clone();
        self.lookup(&parent, name, check_path)
    }

    /// To avoid cache thrashing, we only insert the new entry if object exists
    pub fn lookup(
        &mut self,
        parent: &SymbolsPath,
        name: &OsStr,
        check_path: bool,
    ) -> Result<FileHandleU64, nfsstat3> {
        use std::collections::hash_map::Entry;

        let symbol = self.symbols.insert_or_resolve(name.into());
        let entry = self.path_to_id.entry(parent.join(symbol));
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
