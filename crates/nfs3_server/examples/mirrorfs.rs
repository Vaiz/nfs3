#![allow(clippy::unwrap_used, clippy::significant_drop_tightening)] // for the sake of the example

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::Metadata;
use std::io::SeekFrom;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use intaglio::Symbol;
use intaglio::osstr::SymbolTable;
use nfs3_server::fs_util::{
    exists_no_traverse, fattr3_differ, file_setattr, metadata_to_fattr3, path_setattr,
};
use nfs3_server::tcp::{NFSTcp, NFSTcpListener};
use nfs3_server::vfs::{
    NFSFileSystem, NextResult, ReadDirIterator, ReadDirPlusIterator, VFSCapabilities,
};
use nfs3_types::nfs3::{
    entryplus3, fattr3, fileid3, filename3, ftype3, nfspath3, nfsstat3, post_op_attr, post_op_fh3,
    sattr3,
};
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tracing::debug;

use crate::string_ext::{FromOsString, IntoOsString};

const HOSTPORT: u16 = 11111;

// This example implements a simple NFS server that mirrors a directory from the host filesystem.
// Usage:
// cargo run --example mirrorfs -- <directory> [bind_ip] [bind_port]
// <directory> is the directory to mirror
// [bind_ip] is the IP address to bind to (default: 0.0.0.0)
// [bind_port] is the port to bind to (default: 11111)
//
// To mount the NFS server on Linux, use the following command:
// mount -t nfs -o nolock,vers=3,tcp,port=11111,mountport=11111,soft 127.0.0.1:/ mnt/

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(std::io::stderr)
        .init();

    let args = std::env::args().collect::<Vec<_>>();
    let path = args.get(1).expect("must supply directory to mirror");
    let bind_ip = args.get(2).map_or("0.0.0.0", std::string::String::as_str);
    let bind_port = args
        .get(3)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(HOSTPORT);

    let path = PathBuf::from(path);

    let fs = MirrorFS::new(path);
    let listener = NFSTcpListener::bind(&format!("{bind_ip}:{bind_port}"), fs)
        .await
        .unwrap();
    listener.handle_forever().await.unwrap();
}

#[derive(Debug, Clone)]
struct FSEntry {
    name: Vec<Symbol>,
    fsmeta: fattr3,
    /// metadata when building the children list
    children_meta: fattr3,
    children: Option<Vec<fileid3>>,
}

#[derive(Debug)]
struct FSMap {
    root: PathBuf,
    next_fileid: AtomicU64,
    intern: SymbolTable,
    id_to_path: HashMap<fileid3, FSEntry>,
    path_to_id: HashMap<Vec<Symbol>, fileid3>,
}

enum RefreshResult {
    /// The fileid was deleted
    Delete,
    /// The fileid needs to be reloaded. mtime has been updated, caches
    /// need to be evicted.
    Reload,
    /// Nothing has changed
    Noop,
}

impl FSMap {
    fn new(root: PathBuf) -> Self {
        // create root entry
        let root_entry = FSEntry {
            name: Vec::new(),
            fsmeta: metadata_to_fattr3(1, &root.metadata().unwrap()),
            children_meta: metadata_to_fattr3(1, &root.metadata().unwrap()),
            children: None,
        };
        Self {
            root,
            next_fileid: AtomicU64::new(1),
            intern: SymbolTable::new(),
            id_to_path: HashMap::from([(0, root_entry)]),
            path_to_id: HashMap::from([(Vec::new(), 0)]),
        }
    }
    fn sym_to_path(&self, symlist: &[Symbol]) -> PathBuf {
        let mut ret = self.root.clone();
        for i in symlist {
            ret.push(self.intern.get(*i).unwrap());
        }
        ret
    }

    fn sym_to_fname(&self, symlist: &[Symbol]) -> OsString {
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

    fn delete_entry(&mut self, id: fileid3) {
        let mut children = Vec::new();
        self.collect_all_children(id, &mut children);
        for i in &children {
            if let Some(ent) = self.id_to_path.remove(i) {
                self.path_to_id.remove(&ent.name);
            }
        }
    }

    fn find_entry(&self, id: fileid3) -> Result<&FSEntry, nfsstat3> {
        self.id_to_path.get(&id).ok_or(nfsstat3::NFS3ERR_NOENT)
    }
    fn find_entry_mut(&mut self, id: fileid3) -> Result<&mut FSEntry, nfsstat3> {
        self.id_to_path.get_mut(&id).ok_or(nfsstat3::NFS3ERR_NOENT)
    }
    fn find_child(&self, id: fileid3, filename: &[u8]) -> Result<fileid3, nfsstat3> {
        let mut name = self
            .id_to_path
            .get(&id)
            .ok_or(nfsstat3::NFS3ERR_NOENT)?
            .name
            .clone();
        name.push(
            self.intern
                .check_interned(filename.as_os_str())
                .ok_or(nfsstat3::NFS3ERR_NOENT)?,
        );
        Ok(*self.path_to_id.get(&name).ok_or(nfsstat3::NFS3ERR_NOENT)?)
    }
    async fn refresh_entry(&mut self, id: fileid3) -> Result<RefreshResult, nfsstat3> {
        let entry = self
            .id_to_path
            .get(&id)
            .ok_or(nfsstat3::NFS3ERR_NOENT)?
            .clone();
        let path = self.sym_to_path(&entry.name);
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
    async fn refresh_dir_list(&mut self, id: fileid3) -> Result<(), nfsstat3> {
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
        let mut cur_path = entry.name.clone();
        let path = self.sym_to_path(&entry.name);
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

    fn create_entry(&mut self, fullpath: &Vec<Symbol>, meta: &Metadata) -> fileid3 {
        if let Some(chid) = self.path_to_id.get(fullpath) {
            if let Some(chent) = self.id_to_path.get_mut(chid) {
                chent.fsmeta = metadata_to_fattr3(*chid, meta);
            }
            *chid
        } else {
            // path does not exist
            let next_id = self.next_fileid.fetch_add(1, Ordering::Relaxed);
            let metafattr = metadata_to_fattr3(next_id, meta);
            let new_entry = FSEntry {
                name: fullpath.clone(),
                fsmeta: metafattr.clone(),
                children_meta: metafattr,
                children: None,
            };
            debug!("creating new entry {:?}: {:?}", next_id, meta);
            self.id_to_path.insert(next_id, new_entry);
            self.path_to_id.insert(fullpath.clone(), next_id);
            next_id
        }
    }
}
#[derive(Debug)]
pub struct MirrorFS {
    fsmap: Arc<tokio::sync::RwLock<FSMap>>,
}

/// Enumeration for the `create_fs_object` method
enum CreateFSObject<'a> {
    /// Creates a directory
    Directory,
    /// Creates a file with a set of attributes
    File(sattr3),
    /// Creates an exclusive file with a set of attributes
    Exclusive,
    /// Creates a symlink with a set of attributes to a target location
    Symlink((sattr3, nfspath3<'a>)),
}
impl MirrorFS {
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self {
            fsmap: Arc::new(tokio::sync::RwLock::new(FSMap::new(root))),
        }
    }

    /// creates a FS object in a given directory and of a given type
    /// Updates as much metadata as we can in-place
    async fn create_fs_object(
        &self,
        dirid: fileid3,
        objectname: &filename3<'_>,
        object: &CreateFSObject<'_>,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        let mut fsmap = self.fsmap.write().await;
        let ent = fsmap.find_entry(dirid)?;
        let mut path = fsmap.sym_to_path(&ent.name);
        let objectname_osstr = objectname.as_os_str().to_os_string();
        path.push(&objectname_osstr);

        match object {
            CreateFSObject::Directory => {
                debug!("mkdir {:?}", path);
                if exists_no_traverse(&path) {
                    return Err(nfsstat3::NFS3ERR_EXIST);
                }
                tokio::fs::create_dir(&path)
                    .await
                    .map_err(|_| nfsstat3::NFS3ERR_IO)?;
            }
            CreateFSObject::File(setattr) => {
                debug!("create {:?}", path);
                let file = std::fs::File::create(&path).map_err(|_| nfsstat3::NFS3ERR_IO)?;
                let _ = file_setattr(&file, setattr).await;
            }
            CreateFSObject::Exclusive => {
                debug!("create exclusive {:?}", path);
                let _ = std::fs::File::options()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .map_err(|_| nfsstat3::NFS3ERR_EXIST)?;
            }
            CreateFSObject::Symlink((_, target)) => {
                debug!("symlink {:?} {:?}", path, target);
                if exists_no_traverse(&path) {
                    return Err(nfsstat3::NFS3ERR_EXIST);
                }

                #[cfg(unix)]
                tokio::fs::symlink(target.as_os_str(), &path)
                    .await
                    .map_err(|_| nfsstat3::NFS3ERR_IO)?;

                #[cfg(not(unix))]
                return Err(nfsstat3::NFS3ERR_IO);
                // we do not set attributes on symlinks
            }
        }

        let mut name = ent.name.clone();
        let _ = fsmap.refresh_entry(dirid).await;
        let sym = fsmap.intern.intern(objectname_osstr).unwrap();
        name.push(sym);
        let meta = path.symlink_metadata().map_err(|_| nfsstat3::NFS3ERR_IO)?;
        let fileid = fsmap.create_entry(&name, &meta);

        // update the children list
        if let Some(ref mut children) = fsmap
            .id_to_path
            .get_mut(&dirid)
            .ok_or(nfsstat3::NFS3ERR_NOENT)?
            .children
        {
            match children.binary_search(&fileid) {
                Ok(_) => {
                    return Err(nfsstat3::NFS3ERR_EXIST);
                }
                Err(pos) => {
                    children.insert(pos, fileid);
                }
            }
        }
        Ok((fileid, metadata_to_fattr3(fileid, &meta)))
    }
}

impl NFSFileSystem for MirrorFS {
    fn root_dir(&self) -> fileid3 {
        0
    }
    fn capabilities(&self) -> VFSCapabilities {
        VFSCapabilities::ReadWrite
    }

    async fn lookup(&self, dirid: fileid3, filename: &filename3<'_>) -> Result<fileid3, nfsstat3> {
        let mut fsmap = self.fsmap.write().await;
        if let Ok(id) = fsmap.find_child(dirid, filename.as_ref()) {
            if fsmap.id_to_path.contains_key(&id) {
                return Ok(id);
            }
        }
        // Optimize for negative lookups.
        // See if the file actually exists on the filesystem
        let dirent = fsmap.find_entry(dirid)?;
        let mut path = fsmap.sym_to_path(&dirent.name);
        let objectname_osstr = filename.to_os_string();
        path.push(&objectname_osstr);
        if !exists_no_traverse(&path) {
            return Err(nfsstat3::NFS3ERR_NOENT);
        }
        // ok the file actually exists.
        // that means something changed under me probably.
        // refresh.

        if matches!(fsmap.refresh_entry(dirid).await?, RefreshResult::Delete) {
            return Err(nfsstat3::NFS3ERR_NOENT);
        }
        let _ = fsmap.refresh_dir_list(dirid).await;

        fsmap.find_child(dirid, filename.as_ref())
        // debug!("lookup({:?}, {:?})", dirid, filename);

        // debug!(" -- lookup result {:?}", res);
    }

    async fn getattr(&self, id: fileid3) -> Result<fattr3, nfsstat3> {
        // debug!("Stat query {:?}", id);
        let mut fsmap = self.fsmap.write().await;
        if matches!(fsmap.refresh_entry(id).await?, RefreshResult::Delete) {
            return Err(nfsstat3::NFS3ERR_NOENT);
        }
        let ent = fsmap.find_entry(id)?;
        let path = fsmap.sym_to_path(&ent.name);
        debug!("Stat {:?}: {:?}", path, ent);
        Ok(ent.fsmeta.clone())
    }

    #[allow(clippy::cast_possible_truncation)]
    async fn read(
        &self,
        id: fileid3,
        offset: u64,
        count: u32,
    ) -> Result<(Vec<u8>, bool), nfsstat3> {
        let fsmap = self.fsmap.read().await;
        let entry = fsmap.find_entry(id)?;
        let path = fsmap.sym_to_path(&entry.name);
        drop(fsmap);
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

    async fn readdir(
        &self,
        dirid: fileid3,
        start_after: fileid3,
    ) -> Result<impl ReadDirIterator, nfsstat3> {
        let fsmap = Arc::clone(&self.fsmap);
        let iter = MirrorFsIterator::new(fsmap, dirid, start_after).await?;
        Ok(iter)
    }

    async fn readdirplus(
        &self,
        dirid: fileid3,
        start_after: fileid3,
    ) -> Result<impl ReadDirPlusIterator, nfsstat3> {
        let fsmap = Arc::clone(&self.fsmap);
        let iter = MirrorFsIterator::new(fsmap, dirid, start_after).await?;
        Ok(iter)
    }

    async fn setattr(&self, id: fileid3, setattr: sattr3) -> Result<fattr3, nfsstat3> {
        let mut fsmap = self.fsmap.write().await;
        let entry = fsmap.find_entry(id)?;
        let path = fsmap.sym_to_path(&entry.name);
        path_setattr(&path, &setattr).await?;

        // I have to lookup a second time to update
        let metadata = path.symlink_metadata().or(Err(nfsstat3::NFS3ERR_IO))?;
        if let Ok(entry) = fsmap.find_entry_mut(id) {
            entry.fsmeta = metadata_to_fattr3(id, &metadata);
        }
        Ok(metadata_to_fattr3(id, &metadata))
    }
    async fn write(&self, id: fileid3, offset: u64, data: &[u8]) -> Result<fattr3, nfsstat3> {
        let fsmap = self.fsmap.read().await;
        let ent = fsmap.find_entry(id)?;
        let path = fsmap.sym_to_path(&ent.name);
        drop(fsmap);
        debug!("write to init {:?}", path);
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .await
            .map_err(|e| {
                debug!("Unable to open {:?}", e);
                nfsstat3::NFS3ERR_IO
            })?;
        f.seek(SeekFrom::Start(offset)).await.map_err(|e| {
            debug!("Unable to seek {:?}", e);
            nfsstat3::NFS3ERR_IO
        })?;
        f.write_all(data).await.map_err(|e| {
            debug!("Unable to write {:?}", e);
            nfsstat3::NFS3ERR_IO
        })?;
        debug!("write to {:?} {:?} {:?}", path, offset, data.len());
        let _ = f.flush().await;
        let _ = f.sync_all().await;
        let meta = f.metadata().await.or(Err(nfsstat3::NFS3ERR_IO))?;
        Ok(metadata_to_fattr3(id, &meta))
    }

    async fn create(
        &self,
        dirid: fileid3,
        filename: &filename3<'_>,
        setattr: sattr3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        self.create_fs_object(dirid, filename, &CreateFSObject::File(setattr))
            .await
    }

    async fn create_exclusive(
        &self,
        dirid: fileid3,
        filename: &filename3<'_>,
    ) -> Result<fileid3, nfsstat3> {
        Ok(self
            .create_fs_object(dirid, filename, &CreateFSObject::Exclusive)
            .await?
            .0)
    }

    async fn remove(&self, dirid: fileid3, filename: &filename3<'_>) -> Result<(), nfsstat3> {
        let mut fsmap = self.fsmap.write().await;
        let ent = fsmap.find_entry(dirid)?;
        let mut path = fsmap.sym_to_path(&ent.name);
        path.push(filename.as_os_str());
        if let Ok(meta) = path.symlink_metadata() {
            if meta.is_dir() {
                tokio::fs::remove_dir(&path)
                    .await
                    .map_err(|_| nfsstat3::NFS3ERR_IO)?;
            } else {
                tokio::fs::remove_file(&path)
                    .await
                    .map_err(|_| nfsstat3::NFS3ERR_IO)?;
            }

            let mut sympath = ent.name.clone();
            let filesym = fsmap.intern.intern(filename.to_os_string()).unwrap();
            sympath.push(filesym);
            if let Some(fileid) = fsmap.path_to_id.get(&sympath).copied() {
                // update the fileid -> path
                // and the path -> fileid mappings for the deleted file
                fsmap.id_to_path.remove(&fileid);
                fsmap.path_to_id.remove(&sympath);
                // we need to update the children listing for the directories
                if let Ok(dirent_mut) = fsmap.find_entry_mut(dirid) {
                    if let Some(ref mut fromch) = dirent_mut.children {
                        if let Ok(pos) = fromch.binary_search(&fileid) {
                            fromch.remove(pos);
                        } else {
                            // already removed
                        }
                    }
                }
            }

            let _ = fsmap.refresh_entry(dirid).await;
        } else {
            return Err(nfsstat3::NFS3ERR_NOENT);
        }

        Ok(())
    }

    async fn rename(
        &self,
        from_dirid: fileid3,
        from_filename: &filename3<'_>,
        to_dirid: fileid3,
        to_filename: &filename3<'_>,
    ) -> Result<(), nfsstat3> {
        let mut fsmap = self.fsmap.write().await;

        let from_dirent = fsmap.find_entry(from_dirid)?;
        let mut from_path = fsmap.sym_to_path(&from_dirent.name);
        from_path.push(from_filename.as_os_str());

        let to_dirent = fsmap.find_entry(to_dirid)?;
        let mut to_path = fsmap.sym_to_path(&to_dirent.name);
        // to folder must exist
        if !exists_no_traverse(&to_path) {
            return Err(nfsstat3::NFS3ERR_NOENT);
        }
        to_path.push(to_filename.as_os_str());

        // src path must exist
        if !exists_no_traverse(&from_path) {
            return Err(nfsstat3::NFS3ERR_NOENT);
        }
        debug!("Rename {:?} to {:?}", from_path, to_path);
        tokio::fs::rename(&from_path, &to_path)
            .await
            .map_err(|_| nfsstat3::NFS3ERR_IO)?;

        let mut from_sympath = from_dirent.name.clone();
        let mut to_sympath = to_dirent.name.clone();
        let oldsym = fsmap.intern.intern(from_filename.to_os_string()).unwrap();
        let newsym = fsmap.intern.intern(to_filename.to_os_string()).unwrap();
        from_sympath.push(oldsym);
        to_sympath.push(newsym);
        if let Some(fileid) = fsmap.path_to_id.get(&from_sympath).copied() {
            // update the fileid -> path
            // and the path -> fileid mappings for the new file
            fsmap
                .id_to_path
                .get_mut(&fileid)
                .unwrap()
                .name
                .clone_from(&to_sympath);
            fsmap.path_to_id.remove(&from_sympath);
            fsmap.path_to_id.insert(to_sympath, fileid);
            if to_dirid != from_dirid {
                // moving across directories.
                // we need to update the children listing for the directories
                if let Ok(from_dirent_mut) = fsmap.find_entry_mut(from_dirid) {
                    if let Some(ref mut fromch) = from_dirent_mut.children {
                        if let Ok(pos) = fromch.binary_search(&fileid) {
                            fromch.remove(pos);
                        } else {
                            // already removed
                        }
                    }
                }
                if let Ok(to_dirent_mut) = fsmap.find_entry_mut(to_dirid) {
                    if let Some(ref mut toch) = to_dirent_mut.children {
                        match toch.binary_search(&fileid) {
                            Ok(_) => {
                                return Err(nfsstat3::NFS3ERR_EXIST);
                            }
                            Err(pos) => {
                                // insert the fileid in the new directory
                                toch.insert(pos, fileid);
                            }
                        }
                    }
                }
            }
        }
        let _ = fsmap.refresh_entry(from_dirid).await;
        if to_dirid != from_dirid {
            let _ = fsmap.refresh_entry(to_dirid).await;
        }

        Ok(())
    }
    async fn mkdir(
        &self,
        dirid: fileid3,
        dirname: &filename3<'_>,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        self.create_fs_object(dirid, dirname, &CreateFSObject::Directory)
            .await
    }

    async fn symlink<'a>(
        &self,
        dirid: fileid3,
        linkname: &filename3<'a>,
        symlink: &nfspath3<'a>,
        attr: &sattr3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        self.create_fs_object(
            dirid,
            linkname,
            &CreateFSObject::Symlink((attr.clone(), symlink.clone())),
        )
        .await
    }
    async fn readlink(&self, id: fileid3) -> Result<nfspath3, nfsstat3> {
        let fsmap = self.fsmap.read().await;
        let ent = fsmap.find_entry(id)?;
        let path = fsmap.sym_to_path(&ent.name);
        drop(fsmap);
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

struct MirrorFsIterator {
    fsmap: Arc<tokio::sync::RwLock<FSMap>>,
    entries: Vec<fileid3>,
    index: usize,
}

impl MirrorFsIterator {
    #[allow(clippy::significant_drop_tightening)] // doesn't really matter in this case
    async fn new(
        fsmap: Arc<tokio::sync::RwLock<FSMap>>,
        dirid: fileid3,
        start_after: fileid3,
    ) -> Result<Self, nfsstat3> {
        let fsmap_clone = Arc::clone(&fsmap);
        let mut fsmap = fsmap.write().await;
        fsmap.refresh_entry(dirid).await?;
        fsmap.refresh_dir_list(dirid).await?;

        let entry = fsmap.find_entry(dirid)?;
        if !matches!(entry.fsmeta.type_, ftype3::NF3DIR) {
            return Err(nfsstat3::NFS3ERR_NOTDIR);
        }
        debug!("readdir({:?}, {:?})", entry, start_after);
        // we must have children here
        let children = entry.children.as_ref().ok_or(nfsstat3::NFS3ERR_IO)?;

        let pos = match children.binary_search(&start_after) {
            Ok(pos) => pos + 1,
            Err(pos) => {
                // just ignore missing entry
                pos
            }
        };

        let remain_children = children.iter().skip(pos).copied().collect::<Vec<_>>();
        debug!("children len: {:?}", children.len());
        debug!("remaining_len : {:?}", remain_children.len());

        Ok(Self {
            fsmap: fsmap_clone,
            entries: remain_children,
            index: 0,
        })
    }
}

impl ReadDirPlusIterator for MirrorFsIterator {
    async fn next(&mut self) -> NextResult<entryplus3<'static>> {
        loop {
            if self.index >= self.entries.len() {
                return NextResult::Eof;
            }

            let fileid = self.entries[self.index];
            self.index += 1;

            let fsmap = self.fsmap.read().await;
            let fs_entry = match fsmap.find_entry(fileid) {
                Ok(entry) => entry,
                Err(nfsstat3::NFS3ERR_NOENT) => {
                    // skip missing entries
                    debug!("missing entry {fileid}");
                    continue;
                }
                Err(e) => {
                    return NextResult::Err(e);
                }
            };

            let name = fsmap.sym_to_fname(&fs_entry.name);
            debug!("\t --- {fileid} {name:?}");
            let attr = fs_entry.fsmeta.clone();

            let entryplus = entryplus3 {
                fileid,
                name: filename3::from_os_string(name),
                cookie: fileid,
                name_attributes: post_op_attr::Some(attr),
                name_handle: post_op_fh3::None,
            };

            return NextResult::Ok(entryplus);
        }
    }
}

/// Extension methods for converting between `OsString` and `nfs3_types`.
///
/// NOTE: This is something that works without any guarantees of correctly
/// handling OS encoding. It should be used for testing purposes only.
pub mod string_ext {
    use std::ffi::{OsStr, OsString};
    #[cfg(unix)]
    use std::os::unix::ffi::OsStrExt;

    use nfs3_types::nfs3::{filename3, nfspath3};
    #[cfg(not(unix))]
    use nfs3_types::xdr_codec::Opaque;

    pub trait IntoOsString {
        fn as_os_str(&self) -> &OsStr;
        fn to_os_string(&self) -> OsString {
            self.as_os_str().to_os_string()
        }
    }

    pub trait FromOsString: Sized {
        fn from_os_str(osstr: &OsStr) -> Self;
        #[must_use]
        fn from_os_string(osstr: OsString) -> Self {
            Self::from_os_str(osstr.as_os_str())
        }
    }

    #[cfg(unix)]
    impl IntoOsString for [u8] {
        fn as_os_str(&self) -> &OsStr {
            OsStr::from_bytes(self)
        }
    }
    #[cfg(unix)]
    impl IntoOsString for filename3<'_> {
        fn as_os_str(&self) -> &OsStr {
            OsStr::from_bytes(self.as_ref())
        }
    }

    #[cfg(unix)]
    impl FromOsString for filename3<'static> {
        fn from_os_str(osstr: &OsStr) -> Self {
            Self::from(osstr.as_bytes().to_vec())
        }
    }

    #[cfg(unix)]
    impl IntoOsString for nfspath3<'_> {
        fn as_os_str(&self) -> &OsStr {
            OsStr::from_bytes(self.as_ref())
        }
    }
    #[cfg(unix)]
    impl FromOsString for nfspath3<'static> {
        fn from_os_str(osstr: &OsStr) -> Self {
            Self::from(osstr.as_bytes().to_vec())
        }
    }

    #[cfg(not(unix))]
    impl IntoOsString for [u8] {
        fn as_os_str(&self) -> &OsStr {
            std::str::from_utf8(self)
                .expect("cannot convert bytes to utf8 string")
                .as_ref()
        }
    }
    #[cfg(not(unix))]
    impl IntoOsString for filename3<'_> {
        fn as_os_str(&self) -> &OsStr {
            self.as_ref().as_os_str()
        }
    }

    #[cfg(not(unix))]
    impl FromOsString for filename3<'_> {
        fn from_os_str(osstr: &OsStr) -> Self {
            Self(Opaque::owned(
                osstr
                    .to_str()
                    .expect("cannot convert OsStr to utf8 string")
                    .into(),
            ))
        }
    }

    #[cfg(not(unix))]
    impl FromOsString for nfspath3<'_> {
        fn from_os_str(osstr: &OsStr) -> Self {
            Self(Opaque::owned(
                osstr
                    .to_str()
                    .expect("cannot convert OsStr to utf8 string")
                    .into(),
            ))
        }
    }
}
