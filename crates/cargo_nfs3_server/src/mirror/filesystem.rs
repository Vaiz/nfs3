use std::path::Path;
use std::sync::Arc;

use nfs3_server::fs_util::{exists_no_traverse, file_setattr, metadata_to_fattr3, path_setattr};
use nfs3_server::nfs3_types::nfs3::{
    createverf3, fattr3, fileid3, filename3, nfspath3, nfsstat3, sattr3,
};
use nfs3_server::vfs::{
    FileHandleU64, NfsFileSystem, NfsReadFileSystem, ReadDirIterator, ReadDirPlusIterator,
};
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};
use tracing::debug;

use super::fs_map::{FSMap, RefreshResult};
use super::iterator::MirrorFsIterator;
use super::string_ext::{FromOsString, IntoOsString};

/// Enumeration for the `create_fs_object` method
enum CreateFSObject<'a> {
    /// Creates a directory
    Directory,
    /// Creates a file with a set of attributes
    File(sattr3),
    /// Creates an exclusive file with a set of attributes
    Exclusive(createverf3),
    /// Creates a symlink with a set of attributes to a target location
    Symlink((sattr3, nfspath3<'a>)),
}

#[derive(Debug)]
pub struct MirrorFs {
    fsmap: Arc<tokio::sync::RwLock<FSMap>>,
}

impl MirrorFs {
    #[must_use]
    pub fn new(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref().to_path_buf();
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
        let mut path = fsmap.sym_to_path(&ent.path);
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
            CreateFSObject::Exclusive(_verf) => {
                // TODO: Store the createverf3 value in the file's metadata. If the file already
                // exists and the stored createverf3 matches the requested one,
                // treat this as a successful creation. Otherwise, return an error
                // as per NFSv3 exclusive create semantics.
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

        let mut name = ent.path.clone();
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

impl NfsReadFileSystem for MirrorFs {
    type Handle = FileHandleU64;

    fn root_dir(&self) -> Self::Handle {
        FileHandleU64::new(0)
    }

    async fn lookup(
        &self,
        dirid: &Self::Handle,
        filename: &filename3<'_>,
    ) -> Result<Self::Handle, nfsstat3> {
        let dirid = dirid.as_u64();
        let mut fsmap = self.fsmap.write().await;
        if let Ok(id) = fsmap.find_child(dirid, filename.as_ref()) {
            if fsmap.id_to_path.contains_key(&id) {
                return Ok(FileHandleU64::new(id));
            }
        }
        // Optimize for negative lookups.
        // See if the file actually exists on the filesystem
        let dirent = fsmap.find_entry(dirid)?;
        let mut path = fsmap.sym_to_path(&dirent.path);
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
        fsmap
            .find_child(dirid, filename.as_ref())
            .map(FileHandleU64::new)
    }

    async fn getattr(&self, id: &Self::Handle) -> Result<fattr3, nfsstat3> {
        let id = id.as_u64();
        let mut fsmap = self.fsmap.write().await;
        if matches!(fsmap.refresh_entry(id).await?, RefreshResult::Delete) {
            return Err(nfsstat3::NFS3ERR_NOENT);
        }
        let ent = fsmap.find_entry(id)?;
        let path = fsmap.sym_to_path(&ent.path);
        debug!("Stat {:?}: {:?}", path, ent);
        Ok(ent.fsmeta.clone())
    }

    #[allow(clippy::cast_possible_truncation)]
    async fn read(
        &self,
        id: &Self::Handle,
        offset: u64,
        count: u32,
    ) -> Result<(Vec<u8>, bool), nfsstat3> {
        let id = id.as_u64();
        let fsmap = self.fsmap.read().await;
        let entry = fsmap.find_entry(id)?;
        let path = fsmap.sym_to_path(&entry.path);
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
        dirid: &Self::Handle,
        start_after: nfs3_server::nfs3_types::nfs3::cookie3,
    ) -> Result<impl ReadDirIterator, nfsstat3> {
        let dirid = dirid.as_u64();
        let fsmap = Arc::clone(&self.fsmap);
        let iter = MirrorFsIterator::new(fsmap, dirid, start_after).await?;
        Ok(iter)
    }

    async fn readdirplus(
        &self,
        dirid: &Self::Handle,
        start_after: nfs3_server::nfs3_types::nfs3::cookie3,
    ) -> Result<impl ReadDirPlusIterator<Self::Handle>, nfsstat3> {
        let dirid = dirid.as_u64();
        let fsmap = Arc::clone(&self.fsmap);
        let iter = MirrorFsIterator::new(fsmap, dirid, start_after).await?;
        Ok(iter)
    }

    async fn readlink(&self, id: &Self::Handle) -> Result<nfspath3<'_>, nfsstat3> {
        let id = id.as_u64();
        let fsmap = self.fsmap.read().await;
        let ent = fsmap.find_entry(id)?;
        let path = fsmap.sym_to_path(&ent.path);
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

impl NfsFileSystem for MirrorFs {
    async fn setattr(&self, id: &Self::Handle, setattr: sattr3) -> Result<fattr3, nfsstat3> {
        let id = id.as_u64();
        let mut fsmap = self.fsmap.write().await;
        let entry = fsmap.find_entry(id)?;
        let path = fsmap.sym_to_path(&entry.path);
        path_setattr(&path, &setattr).await?;

        // I have to lookup a second time to update
        let metadata = path.symlink_metadata().or(Err(nfsstat3::NFS3ERR_IO))?;
        if let Ok(entry) = fsmap.find_entry_mut(id) {
            entry.fsmeta = metadata_to_fattr3(id, &metadata);
        }
        Ok(metadata_to_fattr3(id, &metadata))
    }

    async fn write(&self, id: &Self::Handle, offset: u64, data: &[u8]) -> Result<fattr3, nfsstat3> {
        let id = id.as_u64();
        let fsmap = self.fsmap.read().await;
        let ent = fsmap.find_entry(id)?;
        let path = fsmap.sym_to_path(&ent.path);
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
        dirid: &Self::Handle,
        filename: &filename3<'_>,
        setattr: sattr3,
    ) -> Result<(Self::Handle, fattr3), nfsstat3> {
        self.create_fs_object(dirid.as_u64(), filename, &CreateFSObject::File(setattr))
            .await
            .map(|(id, attr)| (FileHandleU64::new(id), attr))
    }

    async fn create_exclusive(
        &self,
        dirid: &Self::Handle,
        filename: &filename3<'_>,
        createverf: createverf3,
    ) -> Result<Self::Handle, nfsstat3> {
        let id = self
            .create_fs_object(
                dirid.as_u64(),
                filename,
                &CreateFSObject::Exclusive(createverf),
            )
            .await?
            .0;
        Ok(FileHandleU64::new(id))
    }

    async fn remove(&self, dirid: &Self::Handle, filename: &filename3<'_>) -> Result<(), nfsstat3> {
        let dirid = dirid.as_u64();
        let mut fsmap = self.fsmap.write().await;
        let ent = fsmap.find_entry(dirid)?;
        let mut path = fsmap.sym_to_path(&ent.path);
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

            let mut sympath = ent.path.clone();
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
        from_dirid: &Self::Handle,
        from_filename: &filename3<'_>,
        to_dirid: &Self::Handle,
        to_filename: &filename3<'_>,
    ) -> Result<(), nfsstat3> {
        let from_dirid = from_dirid.as_u64();
        let to_dirid = to_dirid.as_u64();
        let mut fsmap = self.fsmap.write().await;

        let from_dirent = fsmap.find_entry(from_dirid)?;
        let mut from_path = fsmap.sym_to_path(&from_dirent.path);
        from_path.push(from_filename.as_os_str());

        let to_dirent = fsmap.find_entry(to_dirid)?;
        let mut to_path = fsmap.sym_to_path(&to_dirent.path);
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

        let mut from_sympath = from_dirent.path.clone();
        let mut to_sympath = to_dirent.path.clone();
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
                .path
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
        dirid: &Self::Handle,
        dirname: &filename3<'_>,
    ) -> Result<(Self::Handle, fattr3), nfsstat3> {
        self.create_fs_object(dirid.as_u64(), dirname, &CreateFSObject::Directory)
            .await
            .map(|(id, attr)| (FileHandleU64::new(id), attr))
    }

    async fn symlink<'a>(
        &self,
        dirid: &Self::Handle,
        linkname: &filename3<'a>,
        symlink: &nfspath3<'a>,
        attr: &sattr3,
    ) -> Result<(Self::Handle, fattr3), nfsstat3> {
        self.create_fs_object(
            dirid.as_u64(),
            linkname,
            &CreateFSObject::Symlink((attr.clone(), symlink.clone())),
        )
        .await
        .map(|(id, attr)| (FileHandleU64::new(id), attr))
    }
}
