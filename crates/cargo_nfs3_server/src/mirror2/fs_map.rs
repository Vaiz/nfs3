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
    /// Last modification time for directory content tracking
    pub(super) last_dir_mtime: Option<std::time::SystemTime>,
    pub(super) children: Option<Vec<fileid3>>,
}

impl FSEntry {
    fn new(path: Vec<Symbol>) -> Self {
        Self {
            path,
            last_dir_mtime: None,
            children: None,
        }
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
    /// The fileid needs to be reloaded. mtime has been updated, caches
    /// need to be evicted.
    Reload,
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

    fn collect_all_children(&self, id: fileid3, ret: &mut Vec<fileid3>) {
        ret.push(id);
        if let Some(entry) = self.id_to_path.get(&id) {
            if let Some(ref ch) = entry.children {
                for i in ch {
                    self.collect_all_children(*i, ret);
                }
            }
        }
    }

    pub(super) fn delete_entry(&mut self, id: fileid3) {
        let mut children = Vec::new();
        self.collect_all_children(id, &mut children);
        for i in &children {
            if let Some(ent) = self.id_to_path.remove(i) {
                self.path_to_id.remove(&ent.path);
            }
        }
    }

    pub(super) fn find_entry(&self, id: fileid3) -> Result<&FSEntry, nfsstat3> {
        self.id_to_path.get(&id).ok_or(nfsstat3::NFS3ERR_NOENT)
    }

    pub(super) fn find_entry_mut(&mut self, id: fileid3) -> Result<&mut FSEntry, nfsstat3> {
        self.id_to_path.get_mut(&id).ok_or(nfsstat3::NFS3ERR_NOENT)
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

    pub(super) async fn refresh_entry(&mut self, id: fileid3) -> Result<RefreshResult, nfsstat3> {
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

        // For directories, check if the modification time has changed
        if let Ok(metadata) = tokio::fs::symlink_metadata(&path).await {
            if metadata.is_dir() {
                if let Ok(modified) = metadata.modified() {
                    let entry_mut = self.id_to_path.get_mut(&id).unwrap();
                    if let Some(last_mtime) = entry_mut.last_dir_mtime {
                        if modified != last_mtime {
                            entry_mut.last_dir_mtime = Some(modified);
                            entry_mut.children = None; // Invalidate children cache
                            debug!(
                                "Directory modified, invalidating children cache: {:?}",
                                path
                            );
                            return Ok(RefreshResult::Reload);
                        }
                    } else {
                        entry_mut.last_dir_mtime = Some(modified);
                    }
                }
            }
        } else {
            return Err(nfsstat3::NFS3ERR_IO);
        }

        Ok(RefreshResult::Noop)
    }

    pub(super) async fn refresh_dir_list(&mut self, id: fileid3) -> Result<(), nfsstat3> {
        let entry = self
            .id_to_path
            .get(&id)
            .ok_or(nfsstat3::NFS3ERR_NOENT)?
            .clone();

        // if there are children and the directory hasn't been modified, no need to refresh
        if entry.children.is_some() {
            let path = self.sym_to_path(&entry.path);
            if let Ok(metadata) = tokio::fs::symlink_metadata(&path).await {
                if metadata.is_dir() {
                    if let Ok(modified) = metadata.modified() {
                        if let Some(last_mtime) = entry.last_dir_mtime {
                            if modified == last_mtime {
                                return Ok(());
                            }
                        }
                    }
                }
            }
        }

        // Check if this is actually a directory by loading metadata from filesystem
        let path = self.sym_to_path(&entry.path);
        let metadata = tokio::fs::symlink_metadata(&path)
            .await
            .map_err(|_| nfsstat3::NFS3ERR_IO)?;

        if !metadata.is_dir() {
            return Ok(());
        }

        let mut cur_path = entry.path.clone();
        let mut new_children: Vec<u64> = Vec::new();
        debug!("Relisting entry {:?}: {:?}. Ent: {:?}", id, path, entry);

        if let Ok(mut listing) = tokio::fs::read_dir(&path).await {
            while let Some(entry) = listing
                .next_entry()
                .await
                .map_err(|_| nfsstat3::NFS3ERR_IO)?
            {
                let sym = self.intern.intern(entry.file_name()).unwrap();
                cur_path.push(sym);
                let next_id = self.create_entry(&cur_path);
                new_children.push(next_id);
                cur_path.pop();
            }
            new_children.sort_unstable();

            // Update the directory modification time and children
            let entry_mut = self
                .id_to_path
                .get_mut(&id)
                .ok_or(nfsstat3::NFS3ERR_NOENT)?;
            entry_mut.children = Some(new_children);
            if let Ok(modified) = metadata.modified() {
                entry_mut.last_dir_mtime = Some(modified);
            }
        }

        Ok(())
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
