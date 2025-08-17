use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use intaglio::Symbol;
use intaglio::osstr::SymbolTable;
use nfs3_server::fs_util::exists_no_traverse;
use nfs3_server::nfs3_types::nfs3::{fileid3, nfsstat3};
use tracing::debug;

use super::string_ext::IntoOsString;

#[derive(Debug, Clone)]
pub(super) struct FSEntry {
    pub(super) path: Vec<Symbol>,
}

impl FSEntry {
    const fn new(path: Vec<Symbol>) -> Self {
        Self { path }
    }
}

#[derive(Debug)]
pub(super) struct FSMap {
    pub(super) root: PathBuf,
    pub(super) next_fileid: AtomicU64,
    pub(super) intern: SymbolTable,
    pub(super) id_to_path: HashMap<fileid3, FSEntry>,
    pub(super) path_to_id: HashMap<Vec<Symbol>, fileid3>,
}

pub(super) enum RefreshResult {
    /// The fileid was deleted
    Delete,
    /// Nothing has changed
    Noop,
}

impl FSMap {
    pub(super) fn new(root: PathBuf) -> Self {
        // create root entry
        let root_entry = FSEntry::new(Vec::new());
        Self {
            root,
            next_fileid: AtomicU64::new(1),
            intern: SymbolTable::new(),
            id_to_path: HashMap::from([(0, root_entry)]),
            path_to_id: HashMap::from([(Vec::new(), 0)]),
        }
    }

    pub(super) fn sym_to_path(&self, symlist: &[Symbol]) -> PathBuf {
        let mut ret = self.root.clone();
        for i in symlist {
            ret.push(self.intern.get(*i).unwrap());
        }
        ret
    }

    pub(super) fn sym_to_fname(&self, symlist: &[Symbol]) -> OsString {
        symlist
            .last()
            .map(|x| self.intern.get(*x).unwrap())
            .unwrap_or_default()
            .into()
    }

    fn collect_all_children(id: fileid3, ret: &mut Vec<fileid3>) {
        ret.push(id);
        // No longer traverse cached children since we don't cache them
    }

    pub(super) fn delete_entry(&mut self, id: fileid3) {
        let mut children = Vec::new();
        Self::collect_all_children(id, &mut children);
        for i in &children {
            if let Some(ent) = self.id_to_path.remove(i) {
                self.path_to_id.remove(&ent.path);
            }
        }
    }

    pub(super) fn find_entry(&self, id: fileid3) -> Result<&FSEntry, nfsstat3> {
        self.id_to_path.get(&id).ok_or(nfsstat3::NFS3ERR_NOENT)
    }

    pub(super) fn find_child(&self, id: fileid3, filename: &[u8]) -> Result<fileid3, nfsstat3> {
        let mut name = self
            .id_to_path
            .get(&id)
            .ok_or(nfsstat3::NFS3ERR_NOENT)?
            .path
            .clone();
        name.push(
            self.intern
                .check_interned(filename.as_os_str())
                .ok_or(nfsstat3::NFS3ERR_NOENT)?,
        );
        Ok(*self.path_to_id.get(&name).ok_or(nfsstat3::NFS3ERR_NOENT)?)
    }

    pub(super) fn refresh_entry(&mut self, id: fileid3) -> Result<RefreshResult, nfsstat3> {
        let entry = self
            .id_to_path
            .get(&id)
            .ok_or(nfsstat3::NFS3ERR_NOENT)?
            .clone();
        let path = self.sym_to_path(&entry.path);

        if !exists_no_traverse(&path) {
            self.delete_entry(id);
            debug!("Deleting entry {:?}: {:?}. Ent: {:?}", id, path, entry);
            return Ok(RefreshResult::Delete);
        }

        Ok(RefreshResult::Noop)
    }

    pub(super) async fn read_dir_entries(&mut self, id: fileid3) -> Result<Vec<fileid3>, nfsstat3> {
        let entry = self
            .id_to_path
            .get(&id)
            .ok_or(nfsstat3::NFS3ERR_NOENT)?
            .clone();

        // Check if this is actually a directory by loading metadata from filesystem
        let path = self.sym_to_path(&entry.path);
        let metadata = tokio::fs::symlink_metadata(&path)
            .await
            .map_err(|_| nfsstat3::NFS3ERR_IO)?;

        if !metadata.is_dir() {
            return Ok(Vec::new());
        }

        let mut cur_path = entry.path.clone();
        let mut children: Vec<u64> = Vec::new();
        debug!("Reading directory entries for {:?}: {:?}", id, path);

        if let Ok(mut listing) = tokio::fs::read_dir(&path).await {
            while let Some(entry) = listing
                .next_entry()
                .await
                .map_err(|_| nfsstat3::NFS3ERR_IO)?
            {
                let sym = self.intern.intern(entry.file_name()).unwrap();
                cur_path.push(sym);
                let next_id = self.create_entry(&cur_path);
                children.push(next_id);
                cur_path.pop();
            }
            children.sort_unstable();
        }

        Ok(children)
    }

    pub(super) fn create_entry(&mut self, fullpath: &Vec<Symbol>) -> fileid3 {
        if let Some(chid) = self.path_to_id.get(fullpath) {
            *chid
        } else {
            // path does not exist
            let next_id = self.next_fileid.fetch_add(1, Ordering::Relaxed);
            debug!("creating new entry {:?}", next_id);
            let new_entry = FSEntry::new(fullpath.clone());
            self.id_to_path.insert(next_id, new_entry);
            self.path_to_id.insert(fullpath.clone(), next_id);
            next_id
        }
    }
}
