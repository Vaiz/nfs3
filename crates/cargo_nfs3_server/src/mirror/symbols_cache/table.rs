use std::borrow::Cow;
use std::ffi::OsStr;
use std::path::PathBuf;

use intaglio::Symbol;

use super::SymbolsPath;

#[derive(Debug, Default)]
pub struct SymbolsTable {
    table: intaglio::osstr::SymbolTable,
}

impl SymbolsTable {
    pub fn new() -> Self {
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
        for symbol in path.symbols() {
            let node = self.table.get(symbol).expect("symbol not found");
            path_buf.push(node);
        }
        path_buf
    }

    pub fn len(&self) -> usize {
        self.table.len()
    }
}

/// a helper for `SymbolTable` that implements `Into<Cow<'static, OsStr>>`
pub struct OsStrRef<'a>(Cow<'a, OsStr>);

impl std::ops::Deref for OsStrRef<'_> {
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
