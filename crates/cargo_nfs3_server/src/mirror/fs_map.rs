use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::Metadata;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use intaglio::Symbol;
use intaglio::osstr::SymbolTable;
use nfs3_server::fs_util::{exists_no_traverse, fattr3_differ, metadata_to_fattr3};
use nfs3_server::nfs3_types::nfs3::{fattr3, fileid3, ftype3, nfsstat3};
use tracing::debug;

use super::string_ext::IntoOsString;

#[derive(Debug, Clone)]
pub(super) struct FSEntry {
    pub(super) path: Vec<Symbol>,
    pub(super) fsmeta: fattr3,
    /// metadata when building the children list
    pub(super) children_meta: fattr3,
    pub(super) children: Option<Vec<fileid3>>,
}

impl FSEntry {
    fn new(path: Vec<Symbol>, id: fileid3, fsmeta: &std::fs::Metadata) -> Self {
        let meta = metadata_to_fattr3(id, fsmeta);
        Self {
            path,
            fsmeta: meta.clone(),
            children_meta: meta,
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
        let root_meta = root.metadata().expect("Failed to get root metadata");
        let root_entry = FSEntry::new(Vec::new(), 1, &root_meta);
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
        //
        if !exists_no_traverse(&path) {
            self.delete_entry(id);
            debug!("Deleting entry A {:?}: {:?}. Ent: {:?}", id, path, entry);
            return Ok(RefreshResult::Delete);
        }

        let meta = tokio::fs::symlink_metadata(&path)
            .await
            .map_err(|_| nfsstat3::NFS3ERR_IO)?;
        let meta = metadata_to_fattr3(id, &meta);
        if !fattr3_differ(&meta, &entry.fsmeta) {
            return Ok(RefreshResult::Noop);
        }
        // If we get here we have modifications
        if entry.fsmeta.type_ as u32 != meta.type_ as u32 {
            // if the file type changed ex: file->dir or dir->file
            // really the entire file has been replaced.
            // we expire the entire id
            debug!(
                "File Type Mismatch FT {:?} : {:?} vs {:?}",
                id, entry.fsmeta.type_, meta.type_
            );
            debug!(
                "File Type Mismatch META {:?} : {:?} vs {:?}",
                id, entry.fsmeta, meta
            );
            self.delete_entry(id);
            debug!("Deleting entry B {:?}: {:?}. Ent: {:?}", id, path, entry);
            return Ok(RefreshResult::Delete);
        }
        // inplace modification.
        // update metadata
        self.id_to_path.get_mut(&id).unwrap().fsmeta = meta;
        debug!("Reloading entry {:?}: {:?}. Ent: {:?}", id, path, entry);
        Ok(RefreshResult::Reload)
    }

    pub(super) async fn refresh_dir_list(&mut self, id: fileid3) -> Result<(), nfsstat3> {
        let entry = self
            .id_to_path
            .get(&id)
            .ok_or(nfsstat3::NFS3ERR_NOENT)?
            .clone();
        // if there are children and the metadata did not change
        if entry.children.is_some() && !fattr3_differ(&entry.children_meta, &entry.fsmeta) {
            return Ok(());
        }
        if !matches!(entry.fsmeta.type_, ftype3::NF3DIR) {
            return Ok(());
        }
        let mut cur_path = entry.path.clone();
        let path = self.sym_to_path(&entry.path);
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
                let meta = entry.metadata().await.unwrap();
                let next_id = self.create_entry(&cur_path, &meta);
                new_children.push(next_id);
                cur_path.pop();
            }
            new_children.sort_unstable();
            self.id_to_path
                .get_mut(&id)
                .ok_or(nfsstat3::NFS3ERR_NOENT)?
                .children = Some(new_children);
        }

        Ok(())
    }

    pub(super) fn create_entry(&mut self, fullpath: &Vec<Symbol>, meta: &Metadata) -> fileid3 {
        if let Some(chid) = self.path_to_id.get(fullpath) {
            if let Some(chent) = self.id_to_path.get_mut(chid) {
                chent.fsmeta = metadata_to_fattr3(*chid, meta);
            }
            *chid
        } else {
            // path does not exist
            let next_id = self.next_fileid.fetch_add(1, Ordering::Relaxed);
            debug!("creating new entry {:?}: {:?}", next_id, meta);
            let new_entry = FSEntry::new(fullpath.clone(), next_id, meta);
            self.id_to_path.insert(next_id, new_entry);
            self.path_to_id.insert(fullpath.clone(), next_id);
            next_id
        }
    }
}
