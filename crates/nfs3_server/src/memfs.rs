//! In-memory file system for `NFSv3`.
//!
//! It is a simple implementation of a file system that stores files and directories in memory.
//! This file system is used for testing purposes and is not intended for production use.
//!
//! # Limitations
//!
//! - It's a very naive implementation and does not guarantee the best performance.
//! - Methods `create_exclusive`, `rename`, `symlink`, and `readlink` are not implemented and return
//!   `NFS3ERR_NOTSUPP`.
//!
//! # Examples
//!
//! ```no_run
//! use nfs3_server::memfs::{MemFs, MemFsConfig};
//! use nfs3_server::tcp::NFSTcpListener;
//!
//! async fn run() -> anyhow::Result<()> {
//!     let mut config = MemFsConfig::default();
//!     config.add_file("/a.txt", "hello world\n".as_bytes());
//!     config.add_file("/b.txt", "Greetings\n".as_bytes());
//!     config.add_dir("/a directory");
//!
//!     let memfs = MemFs::new(config).unwrap();
//!     let listener = NFSTcpListener::bind("0.0.0.0:11111", memfs).await?;
//!     listener.handle_forever().await?;
//!     Ok(())
//! }
//! ```

use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

use nfs3_types::nfs3::{
    self as nfs, cookie3, entryplus3, fattr3, fileid3, filename3, ftype3, nfs_fh3, nfspath3,
    nfsstat3, nfstime3, sattr3, specdata3,
};
use nfs3_types::xdr_codec::Opaque;

use crate::vfs::{
    DEFAULT_FH_CONVERTER, NextResult, NfsFileSystem, NfsReadFileSystem, ReadDirIterator,
    ReadDirPlusIterator,
};

const DELIMITER: char = '/';

#[derive(Debug)]
struct Dir {
    name: filename3<'static>,
    parent: fileid3,
    attr: fattr3,
    content: HashSet<fileid3>,
}

impl Dir {
    fn new(name: filename3<'static>, id: fileid3, parent: fileid3) -> Self {
        let current_time = current_time();
        let attr = fattr3 {
            type_: ftype3::NF3DIR,
            mode: 0o777,
            nlink: 1,
            uid: 507,
            gid: 507,
            size: 0,
            used: 0,
            rdev: specdata3::default(),
            fsid: 0,
            fileid: id,
            atime: current_time.clone(),
            mtime: current_time.clone(),
            ctime: current_time,
        };
        Self {
            name,
            parent,
            attr,
            content: HashSet::new(),
        }
    }

    fn root_dir() -> Self {
        let name = filename3(Opaque::borrowed(b"/"));
        let id = 1;
        Self::new(name, id, 0)
    }

    fn add_entry(&mut self, entry: fileid3) -> bool {
        self.content.insert(entry)
    }
}

#[derive(Debug)]
struct File {
    name: filename3<'static>,
    _parent: fileid3,
    attr: fattr3,
    content: Vec<u8>,
}

impl File {
    fn new(name: filename3<'static>, id: fileid3, parent: fileid3, content: Vec<u8>) -> Self {
        let current_time = current_time();
        let attr = fattr3 {
            type_: ftype3::NF3REG,
            mode: 0o755,
            nlink: 1,
            uid: 507,
            gid: 507,
            size: content.len() as u64,
            used: content.len() as u64,
            rdev: specdata3::default(),
            fsid: 0,
            fileid: id,
            atime: current_time.clone(),
            mtime: current_time.clone(),
            ctime: current_time,
        };
        Self {
            name,
            _parent: parent,
            attr,
            content,
        }
    }

    fn resize(&mut self, size: u64) {
        self.content
            .resize(usize::try_from(size).expect("size is too large"), 0);
        self.attr.size = size;
        self.attr.used = size;
    }

    fn read(&self, offset: u64, count: u32) -> (Vec<u8>, bool) {
        let mut start = usize::try_from(offset).unwrap_or(usize::MAX);
        let mut end = start + count as usize;
        let bytes = &self.content;
        let eof = end >= bytes.len();
        if start >= bytes.len() {
            start = bytes.len();
        }
        if end > bytes.len() {
            end = bytes.len();
        }
        (bytes[start..end].to_vec(), eof)
    }

    #[allow(clippy::cast_possible_truncation)]
    fn write(&mut self, offset: u64, data: &[u8]) -> Result<fattr3, nfsstat3> {
        if offset > self.content.len() as u64 {
            return Err(nfsstat3::NFS3ERR_INVAL);
        }

        let offset = offset as usize;
        let end_offset = offset + data.len();
        if end_offset > self.content.len() {
            self.resize(end_offset as u64);
        }
        self.content[offset..end_offset].copy_from_slice(data);
        Ok(self.attr.clone())
    }
}

fn current_time() -> nfstime3 {
    let d = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("failed to get current time");
    nfstime3 {
        seconds: u32::try_from(d.as_secs()).unwrap_or(u32::MAX),
        nseconds: d.subsec_nanos(),
    }
}

#[derive(Debug)]
enum Entry {
    File(File),
    Dir(Dir),
}

impl Entry {
    fn new_file(name: filename3<'static>, id: fileid3, parent: fileid3, content: Vec<u8>) -> Self {
        Self::File(File::new(name, id, parent, content))
    }

    fn new_dir(name: filename3<'static>, id: fileid3, parent: fileid3) -> Self {
        Self::Dir(Dir::new(name, id, parent))
    }

    const fn as_dir(&self) -> Result<&Dir, nfsstat3> {
        match self {
            Self::Dir(dir) => Ok(dir),
            Self::File(_) => Err(nfsstat3::NFS3ERR_NOTDIR),
        }
    }

    const fn as_dir_mut(&mut self) -> Result<&mut Dir, nfsstat3> {
        match self {
            Self::Dir(dir) => Ok(dir),
            Self::File(_) => Err(nfsstat3::NFS3ERR_NOTDIR),
        }
    }

    const fn as_file(&self) -> Result<&File, nfsstat3> {
        match self {
            Self::File(file) => Ok(file),
            Self::Dir(_) => Err(nfsstat3::NFS3ERR_ISDIR),
        }
    }

    const fn as_file_mut(&mut self) -> Result<&mut File, nfsstat3> {
        match self {
            Self::File(file) => Ok(file),
            Self::Dir(_) => Err(nfsstat3::NFS3ERR_ISDIR),
        }
    }

    const fn fileid(&self) -> fileid3 {
        match self {
            Self::File(file) => file.attr.fileid,
            Self::Dir(dir) => dir.attr.fileid,
        }
    }

    const fn name(&self) -> &filename3<'static> {
        match self {
            Self::File(file) => &file.name,
            Self::Dir(dir) => &dir.name,
        }
    }

    const fn attr(&self) -> &fattr3 {
        match self {
            Self::File(file) => &file.attr,
            Self::Dir(dir) => &dir.attr,
        }
    }

    const fn attr_mut(&mut self) -> &mut fattr3 {
        match self {
            Self::File(file) => &mut file.attr,
            Self::Dir(dir) => &mut dir.attr,
        }
    }

    fn set_attr(&mut self, setattr: sattr3) {
        {
            let attr = self.attr_mut();
            match setattr.atime {
                nfs::set_atime::DONT_CHANGE => {}
                nfs::set_atime::SET_TO_CLIENT_TIME(c) => {
                    attr.atime = c;
                }
                nfs::set_atime::SET_TO_SERVER_TIME => {
                    attr.atime = current_time();
                }
            }
            match setattr.mtime {
                nfs::set_mtime::DONT_CHANGE => {}
                nfs::set_mtime::SET_TO_CLIENT_TIME(c) => {
                    attr.mtime = c;
                }
                nfs::set_mtime::SET_TO_SERVER_TIME => {
                    attr.mtime = current_time();
                }
            }
            if let nfs::set_uid3::Some(u) = setattr.uid {
                attr.uid = u;
            }
            if let nfs::set_gid3::Some(u) = setattr.gid {
                attr.gid = u;
            }
        }
        if let nfs::set_size3::Some(s) = setattr.size {
            if let Self::File(file) = self {
                file.resize(s);
            }
        }
    }
}

#[derive(Debug)]
struct Fs {
    entries: HashMap<fileid3, Entry>,
    root: fileid3,
}

impl Fs {
    fn new() -> Self {
        let root = Entry::Dir(Dir::root_dir());
        let fileid = root.fileid();
        let mut flat_list = HashMap::new();
        flat_list.insert(fileid, root);
        Self {
            entries: flat_list,
            root: fileid,
        }
    }

    fn push(&mut self, parent: fileid3, entry: Entry) -> Result<(), nfsstat3> {
        use std::collections::hash_map::Entry as MapEntry;

        let id = entry.fileid();

        let map_entry = self.entries.entry(id);
        match map_entry {
            MapEntry::Occupied(_) => {
                tracing::warn!("object with same id already exists: {id}");
                return Err(nfsstat3::NFS3ERR_EXIST);
            }
            MapEntry::Vacant(v) => {
                v.insert(entry);
            }
        }

        let parent_entry = self.entries.get_mut(&parent);
        match parent_entry {
            None => {
                tracing::warn!("parent not found: {parent}");
                self.entries.remove(&id); // remove the entry we just added
                Err(nfsstat3::NFS3ERR_NOENT)
            }
            Some(Entry::File(_)) => {
                tracing::warn!("parent is not a directory: {parent}");
                self.entries.remove(&id); // remove the entry we just added
                Err(nfsstat3::NFS3ERR_NOTDIR)
            }
            Some(Entry::Dir(dir)) => {
                let added = dir.add_entry(id);
                assert!(added, "failed to add a new entry to directory");
                Ok(())
            }
        }
    }

    fn remove(&mut self, dirid: fileid3, filename: &filename3) -> Result<(), nfsstat3> {
        if filename.as_ref() == b"." || filename.as_ref() == b".." {
            return Err(nfsstat3::NFS3ERR_INVAL);
        }

        let object_id = {
            let entry = self.entries.get(&dirid).ok_or(nfsstat3::NFS3ERR_NOENT)?;
            let dir = entry.as_dir()?;
            let id = dir
                .content
                .iter()
                .find(|i| self.entries.get(i).is_some_and(|f| f.name() == filename));
            id.copied().ok_or(nfsstat3::NFS3ERR_NOENT)?
        };

        let entry = self
            .entries
            .get(&object_id)
            .ok_or(nfsstat3::NFS3ERR_NOENT)?;
        if let Entry::Dir(dir) = entry {
            if !dir.content.is_empty() {
                return Err(nfsstat3::NFS3ERR_NOTEMPTY);
            }
        }

        self.entries.remove(&object_id);
        self.entries
            .get_mut(&dirid)
            .expect("entry not found")
            .as_dir_mut()?
            .content
            .remove(&object_id);
        Ok(())
    }

    fn get(&self, id: fileid3) -> Option<&Entry> {
        self.entries.get(&id)
    }

    fn get_mut(&mut self, id: fileid3) -> Option<&mut Entry> {
        self.entries.get_mut(&id)
    }
}

/// In-memory file system for `NFSv3`.
///
/// `MemFs` implements the [`NfsFileSystem`] trait and provides a simple in-memory file system
#[derive(Debug)]
pub struct MemFs {
    fs: Arc<RwLock<Fs>>,
    rootdir: fileid3,
    nextid: AtomicU64,
}

impl Default for MemFs {
    fn default() -> Self {
        let root = Fs::new();
        let rootdir = root.root;
        let nextid = AtomicU64::new(rootdir + 1);
        Self {
            fs: Arc::new(RwLock::new(root)),
            rootdir,
            nextid,
        }
    }
}

impl MemFs {
    /// Creates a new in-memory file system with the given configuration.
    pub fn new(config: MemFsConfig) -> Result<Self, nfsstat3> {
        tracing::info!("creating memfs. Entries count: {}", config.entries.len());
        let fs = Self::default();

        for entry in config.entries {
            let id = fs.path_to_id_impl(&entry.parent)?;
            let name = filename3(Opaque::owned(entry.name.into_bytes()));
            if entry.is_dir {
                fs.add_dir(id, name)?;
            } else {
                fs.add_file(id, name, sattr3::default(), entry.content)?;
            }
        }

        Ok(fs)
    }

    fn add_dir(
        &self,
        dirid: fileid3,
        dirname: filename3<'static>,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        let newid = self
            .nextid
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let dir = Entry::new_dir(dirname, newid, dirid);
        let attr = dir.attr().clone();

        self.fs
            .write()
            .expect("lock is poisoned")
            .push(dirid, dir)?;

        Ok((newid, attr))
    }

    fn add_file(
        &self,
        dirid: fileid3,
        filename: filename3<'static>,
        attr: sattr3,
        content: Vec<u8>,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        let newid = self
            .nextid
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let mut file = Entry::new_file(filename, newid, dirid, content);
        file.set_attr(attr);
        let attr = file.attr().clone();

        self.fs
            .write()
            .expect("lock is poisoned")
            .push(dirid, file)?;

        Ok((newid, attr))
    }

    fn lookup_impl(&self, dirid: fileid3, filename: &filename3) -> Result<fileid3, nfsstat3> {
        let fs = self.fs.read().expect("lock is poisoned");
        let entry = fs.get(dirid).ok_or(nfsstat3::NFS3ERR_NOENT)?;

        if let Entry::File(_) = entry {
            return Err(nfsstat3::NFS3ERR_NOTDIR);
        } else if let Entry::Dir(dir) = &entry {
            // if looking for dir/. its the current directory
            if filename.as_ref() == b"." {
                return Ok(dirid);
            }
            // if looking for dir/.. its the parent directory
            if filename.as_ref() == b".." {
                return Ok(dir.parent);
            }
            for i in &dir.content {
                match fs.get(*i) {
                    None => {
                        tracing::error!("invalid entry: {i}");
                        return Err(nfsstat3::NFS3ERR_SERVERFAULT);
                    }
                    Some(f) => {
                        if f.name() == filename {
                            return Ok(f.fileid());
                        }
                    }
                }
            }
        }
        Err(nfsstat3::NFS3ERR_NOENT)
    }

    fn path_to_id_impl(&self, path: &str) -> Result<fileid3, nfsstat3> {
        let splits = path.split(DELIMITER);
        let mut fid = self.root_dir();
        for component in splits {
            if component.is_empty() {
                continue;
            }
            fid = self.lookup_impl(fid, &component.as_bytes().into())?;
        }
        Ok(fid)
    }

    fn make_iter(&self, dirid: fileid3, start_after: cookie3) -> Result<MemFsIterator, nfsstat3> {
        let fs = self.fs.read().expect("lock is poisoned");
        let entry = fs.get(dirid).ok_or(nfsstat3::NFS3ERR_NOENT)?;
        let dir = entry.as_dir()?;

        let mut iter = dir.content.iter();
        if start_after != 0 {
            // skip to the start_after entry
            let find_result = iter.find(|i| **i == start_after);
            if find_result.is_none() {
                return Err(nfsstat3::NFS3ERR_BAD_COOKIE);
            }
        }
        let content: Vec<_> = iter.copied().collect();
        Ok(MemFsIterator::new(self.fs.clone(), content))
    }
}

impl NfsReadFileSystem for MemFs {
    fn root_dir(&self) -> fileid3 {
        self.rootdir
    }

    async fn lookup(&self, dirid: fileid3, filename: &filename3<'_>) -> Result<fileid3, nfsstat3> {
        self.lookup_impl(dirid, filename)
    }

    async fn getattr(&self, id: fileid3) -> Result<fattr3, nfsstat3> {
        let fs = self.fs.read().expect("lock is poisoned");
        let entry = fs.get(id).ok_or(nfsstat3::NFS3ERR_NOENT)?;
        Ok(entry.attr().clone())
    }
    async fn read(
        &self,
        id: fileid3,
        offset: u64,
        count: u32,
    ) -> Result<(Vec<u8>, bool), nfsstat3> {
        let fs = self.fs.read().expect("lock is poisoned");
        let entry = fs.get(id).ok_or(nfsstat3::NFS3ERR_NOENT)?;
        let file = entry.as_file()?;
        Ok(file.read(offset, count))
    }

    async fn readdir(
        &self,
        dirid: fileid3,
        start_after: fileid3,
    ) -> Result<impl ReadDirIterator, nfsstat3> {
        let iter = Self::make_iter(self, dirid, start_after)?;
        Ok(iter)
    }

    async fn readdirplus(
        &self,
        dirid: fileid3,
        start_after: fileid3,
    ) -> Result<impl ReadDirPlusIterator, nfsstat3> {
        let iter = Self::make_iter(self, dirid, start_after)?;
        Ok(iter)
    }

    /// Converts the fileid to an opaque NFS file handle.
    fn id_to_fh(&self, id: fileid3) -> nfs_fh3 {
        DEFAULT_FH_CONVERTER.id_to_fh(id)
    }
    /// Converts an opaque NFS file handle to a fileid.
    fn fh_to_id(&self, id: &nfs_fh3) -> Result<fileid3, nfsstat3> {
        DEFAULT_FH_CONVERTER.fh_to_id(id)
    }

    async fn readlink(&self, _id: fileid3) -> Result<nfspath3, nfsstat3> {
        tracing::warn!("readlink not implemented");
        Err(nfsstat3::NFS3ERR_NOTSUPP)
    }

    async fn path_to_id(&self, path: &str) -> Result<fileid3, nfsstat3> {
        self.path_to_id_impl(path)
    }
}

impl NfsFileSystem for MemFs {
    async fn setattr(&self, id: fileid3, setattr: sattr3) -> Result<fattr3, nfsstat3> {
        let mut fs = self.fs.write().expect("lock is poisoned");
        let entry = fs.get_mut(id).ok_or(nfsstat3::NFS3ERR_NOENT)?;
        entry.set_attr(setattr);
        Ok(entry.attr().clone())
    }

    async fn write(&self, id: fileid3, offset: u64, data: &[u8]) -> Result<fattr3, nfsstat3> {
        let mut fs = self.fs.write().expect("lock is poisoned");

        let entry = fs.get_mut(id).ok_or(nfsstat3::NFS3ERR_NOENT)?;
        let file = entry.as_file_mut().map_err(|_| nfsstat3::NFS3ERR_INVAL)?;
        file.write(offset, data)
    }

    async fn create(
        &self,
        dirid: fileid3,
        filename: &filename3<'_>,
        attr: sattr3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        self.add_file(dirid, filename.clone_to_owned(), attr, Vec::new())
    }

    async fn create_exclusive(
        &self,
        _dirid: fileid3,
        _filename: &filename3<'_>,
    ) -> Result<fileid3, nfsstat3> {
        tracing::warn!("create_exclusive not implemented");
        Err(nfsstat3::NFS3ERR_NOTSUPP)
    }

    async fn mkdir(
        &self,
        dirid: fileid3,
        dirname: &filename3<'_>,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        self.add_dir(dirid, dirname.clone_to_owned())
    }

    async fn remove(&self, dirid: fileid3, filename: &filename3<'_>) -> Result<(), nfsstat3> {
        self.fs
            .write()
            .expect("lock is poisoned")
            .remove(dirid, filename)
    }

    async fn rename<'a>(
        &self,
        _from_dirid: fileid3,
        _from_filename: &filename3<'a>,
        _to_dirid: fileid3,
        _to_filename: &filename3<'a>,
    ) -> Result<(), nfsstat3> {
        tracing::warn!("rename not implemented");
        Err(nfsstat3::NFS3ERR_NOTSUPP)
    }

    async fn symlink<'a>(
        &self,
        _dirid: fileid3,
        _linkname: &filename3<'a>,
        _symlink: &nfspath3<'a>,
        _attr: &sattr3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        tracing::warn!("symlink not implemented");
        Err(nfsstat3::NFS3ERR_NOTSUPP)
    }
}

struct MemFsIterator {
    fs: Arc<RwLock<Fs>>,
    entries: Vec<fileid3>,
    index: usize,
}

impl MemFsIterator {
    const fn new(fs: Arc<RwLock<Fs>>, entries: Vec<fileid3>) -> Self {
        Self {
            fs,
            entries,
            index: 0,
        }
    }
}

impl ReadDirPlusIterator for MemFsIterator {
    async fn next(&mut self) -> NextResult<entryplus3<'static>> {
        loop {
            if self.index >= self.entries.len() {
                return NextResult::Eof;
            }
            let id = self.entries[self.index];
            self.index += 1;

            let fs = self.fs.read().expect("lock is poisoned");
            let entry = fs.get(id);
            let Some(entry) = entry else {
                // skip missing entries
                tracing::warn!("entry not found: {id}");
                continue;
            };
            let attr = entry.attr().clone();
            let fh = DEFAULT_FH_CONVERTER.id_to_fh(id);
            return NextResult::Ok(entryplus3 {
                fileid: id,
                name: entry.name().clone_to_owned(),
                cookie: id,
                name_attributes: nfs::post_op_attr::Some(attr),
                name_handle: nfs::post_op_fh3::Some(fh),
            });
        }
    }
}

#[derive(Debug, Clone)]
struct MemFsConfigEntry {
    parent: String,
    name: String,
    is_dir: bool,
    content: Vec<u8>,
}

/// Initial configuration for the in-memory file system.
///
/// It allows to specify the initial files and directories in the file system.
#[derive(Default, Debug, Clone)]
pub struct MemFsConfig {
    entries: Vec<MemFsConfigEntry>,
}

impl MemFsConfig {
    /// Adds a directory to the file system configuration.
    ///
    /// # Panics
    ///
    /// Panics if the path is empty.
    pub fn add_dir(&mut self, path: &str) {
        let name = path
            .split(DELIMITER)
            .next_back()
            .expect("dir path cannot be empty")
            .to_string();
        let path = path.trim_end_matches(&name);
        self.entries.push(MemFsConfigEntry {
            parent: path.to_string(),
            name,
            is_dir: true,
            content: Vec::new(),
        });
    }

    /// Adds a file to the file system configuration.
    ///
    /// # Panics
    ///
    /// Panics if the path is empty.
    pub fn add_file(&mut self, path: &str, content: impl Into<Vec<u8>>) {
        let name = path
            .split(DELIMITER)
            .next_back()
            .expect("file path cannot be empty")
            .to_string();
        let path = path.trim_end_matches(&name);

        self.entries.push(MemFsConfigEntry {
            parent: path.to_string(),
            name,
            is_dir: false,
            content: content.into(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fs_config() {
        let mut config = MemFsConfig::default();
        config.add_file("/a.txt", b"hello world\n");
        config.add_file("/b.txt", b"Greetings to xet data\n");
        config.add_dir("/another_dir");
        config.add_file("/another_dir/thisworks.txt", b"i hope\n");

        assert_eq!(config.entries.len(), 4);
        assert_eq!(config.entries[0].parent, "/");
        assert_eq!(config.entries[1].parent, "/");
        assert_eq!(config.entries[2].parent, "/");
        assert_eq!(config.entries[3].parent, "/another_dir/");
        assert_eq!(config.entries[0].name, "a.txt");
        assert_eq!(config.entries[1].name, "b.txt");
        assert_eq!(config.entries[2].name, "another_dir");
        assert_eq!(config.entries[3].name, "thisworks.txt");
    }
}
