mod path;
mod table;

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};

use nfs3_server::nfs3_types::nfs3::nfsstat3;
use nfs3_server::vfs::FileHandleU64;
use path::SymbolsPath;
use table::SymbolsTable;

use crate::threshold_logger::ThresholdLogger;

#[derive(Debug)]
struct SymbolsCacheInner {
    symbols: SymbolsTable,
    path_to_id: HashMap<SymbolsPath, FileHandleU64>,
    id_to_path: HashMap<FileHandleU64, SymbolsPath>,
    next_id: AtomicU64,

    symbols_logger: ThresholdLogger,
    path_to_id_logger: ThresholdLogger,
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
        inner.symbols_logger.check_and_log(inner.symbols.len());

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

        let id = FileHandleU64::new(inner.next_id.fetch_add(1, Ordering::Relaxed));
        vacant_entry.insert(id);
        inner.id_to_path.insert(id, parent.join(symbol));

        inner
            .path_to_id_logger
            .check_and_log(inner.path_to_id.len());

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
            symbols_logger: ThresholdLogger::new("symbols_table"),
            path_to_id_logger: ThresholdLogger::new("path_to_id_map"),
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
