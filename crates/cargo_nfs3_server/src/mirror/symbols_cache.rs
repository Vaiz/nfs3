use std::borrow::Cow;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::RwLock;
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

// TODO: use faster hash
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
struct SymbolsCacheInner {
    symbols: SymbolsTable,
    path_to_id: HashMap<SymbolsPath, FileHandleU64>,
    id_to_path: HashMap<FileHandleU64, SymbolsPath>,
    next_id: AtomicU64,
}

impl SymbolsCacheInner {
    fn lookup(
        &mut self,
        root: &std::path::Path,
        parent: &SymbolsPath,
        name: &OsStr,
        check_path: bool,
    ) -> Result<FileHandleU64, nfsstat3> {
        use std::collections::hash_map::Entry;

        let inner = self;
        let symbol = inner.symbols.insert_or_resolve(name.into());
        let item = parent.join(symbol);

        let entry = inner.path_to_id.entry(item);
        let vacant_entry = match entry {
            Entry::Occupied(entry) => return Ok(*entry.get()),
            Entry::Vacant(entry) => entry,
        };

        // Entry doesn't exist, need to create it
        if check_path {
            let test_path_parent = inner.symbols.resolve_path(parent);
            let mut test_path = root.join(test_path_parent);
            if !test_path.exists() {
                return Err(nfsstat3::NFS3ERR_BADHANDLE);
            }
            test_path.push(name);
            if !test_path.exists() {
                return Err(nfsstat3::NFS3ERR_NOENT);
            }
        }

        // Create new entry
        let id = FileHandleU64::new(inner.next_id.fetch_add(1, Ordering::Relaxed));
        vacant_entry.insert(id);
        inner.id_to_path.insert(id, parent.join(symbol));
        Ok(id)
    }
}

#[derive(Debug)]
pub struct SymbolsCache {
    root: PathBuf,
    inner: RwLock<SymbolsCacheInner>,
}

impl SymbolsCache {
    pub const ROOT_ID: FileHandleU64 = FileHandleU64::new(1);

    pub fn new(root: PathBuf) -> Self {
        let root_path = SymbolsPath::new();
        let mut path_to_id = HashMap::new();
        let mut id_to_path = HashMap::new();

        // Insert root entry
        path_to_id.insert(root_path.clone(), Self::ROOT_ID);
        id_to_path.insert(Self::ROOT_ID, root_path);

        let inner = SymbolsCacheInner {
            symbols: SymbolsTable::new(),
            path_to_id,
            id_to_path,
            next_id: AtomicU64::new(Self::ROOT_ID.as_u64() + 1),
        };

        Self {
            root,
            inner: RwLock::new(inner),
        }
    }

    pub fn symbols_path(&self, id: FileHandleU64) -> Result<SymbolsPath, nfsstat3> {
        let inner = self.inner.read().expect("lock is poisoned");
        inner
            .id_to_path
            .get(&id)
            .cloned()
            .ok_or(nfsstat3::NFS3ERR_BADHANDLE)
    }

    pub fn handle_to_path(&self, id: FileHandleU64) -> Result<PathBuf, nfsstat3> {
        let inner = self.inner.read().expect("lock is poisoned");
        let path = inner
            .id_to_path
            .get(&id)
            .ok_or(nfsstat3::NFS3ERR_BADHANDLE)?;
        Ok(inner.symbols.resolve_path(path))
    }

    pub fn lookup_by_id(
        &self,
        parent_id: FileHandleU64,
        name: &OsStr,
        check_path: bool,
    ) -> Result<FileHandleU64, nfsstat3> {
        let parent = self.symbols_path(parent_id)?;
        self.lookup(&parent, name, check_path)
    }

    /// To avoid cache thrashing, we only insert the new entry if object exists
    pub fn lookup(
        &self,
        parent: &SymbolsPath,
        name: &OsStr,
        check_path: bool,
    ) -> Result<FileHandleU64, nfsstat3> {
        self.inner
            .write()
            .expect("lock is poisoned")
            .lookup(&self.root, parent, name, check_path)
    }
}
