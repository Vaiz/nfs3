use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

use nfs3_types::nfs3::{
    self as nfs, cookie3, fattr3, fileid3, filename3, ftype3, nfs_fh3, nfspath3, nfsstat3,
    nfstime3, sattr3, specdata3,
};
use nfs3_types::xdr_codec::Opaque;

use crate::vfs::{
    DEFAULT_FH_CONVERTER, NFSFileSystem, ReadDirIterator, ReadDirPlusIterator, VFSCapabilities,
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
        self.content.resize(size as usize, 0);
        self.attr.size = size;
        self.attr.used = size;
    }

    fn read(&self, offset: u64, count: u32) -> Result<(Vec<u8>, bool), nfsstat3> {
        let mut start = offset as usize;
        let mut end = offset as usize + count as usize;
        let bytes = &self.content;
        let eof = end >= bytes.len();
        if start >= bytes.len() {
            start = bytes.len();
        }
        if end > bytes.len() {
            end = bytes.len();
        }
        Ok((bytes[start..end].to_vec(), eof))
    }

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
        .unwrap();
    nfstime3 {
        seconds: d.as_secs() as u32,
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

    fn as_dir(&self) -> Result<&Dir, nfsstat3> {
        match self {
            Self::Dir(dir) => Ok(dir),
            Self::File(_) => Err(nfsstat3::NFS3ERR_NOTDIR),
        }
    }

    fn as_dir_mut(&mut self) -> Result<&mut Dir, nfsstat3> {
        match self {
            Self::Dir(dir) => Ok(dir),
            Self::File(_) => Err(nfsstat3::NFS3ERR_NOTDIR),
        }
    }

    fn as_file(&self) -> Result<&File, nfsstat3> {
        match self {
            Self::File(file) => Ok(file),
            Self::Dir(_) => Err(nfsstat3::NFS3ERR_ISDIR),
        }
    }

    fn as_file_mut(&mut self) -> Result<&mut File, nfsstat3> {
        match self {
            Self::File(file) => Ok(file),
            Self::Dir(_) => Err(nfsstat3::NFS3ERR_ISDIR),
        }
    }

    fn fileid(&self) -> fileid3 {
        match self {
            Self::File(file) => file.attr.fileid,
            Self::Dir(dir) => dir.attr.fileid,
        }
    }

    fn name(&self) -> &filename3<'static> {
        match self {
            Self::File(file) => &file.name,
            Self::Dir(dir) => &dir.name,
        }
    }

    fn attr(&self) -> &fattr3 {
        match self {
            Self::File(file) => &file.attr,
            Self::Dir(dir) => &dir.attr,
        }
    }

    fn attr_mut(&mut self) -> &mut fattr3 {
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
            };
            match setattr.mtime {
                nfs::set_mtime::DONT_CHANGE => {}
                nfs::set_mtime::SET_TO_CLIENT_TIME(c) => {
                    attr.mtime = c;
                }
                nfs::set_mtime::SET_TO_SERVER_TIME => {
                    attr.mtime = current_time();
                }
            };
            if let nfs::set_uid3::Some(u) = setattr.uid {
                attr.uid = u;
            }
            if let nfs::set_gid3::Some(u) = setattr.gid {
                attr.gid = u;
            }
        }
        if let nfs::set_size3::Some(s) = setattr.size {
            if let Entry::File(file) = self {
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
        if filename == ".".as_bytes() || filename == "..".as_bytes() {
            return Err(nfsstat3::NFS3ERR_INVAL);
        }

        let object_id = {
            let entry = self.entries.get(&dirid).ok_or(nfsstat3::NFS3ERR_NOENT)?;
            let dir = entry.as_dir()?;
            let id = dir.content.iter().find(|i| {
                if let Some(f) = self.entries.get(i) {
                    f.name() == filename
                } else {
                    false
                }
            });
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
            .unwrap()
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

        let dir = Entry::new_dir(dirname.clone_to_owned(), newid, dirid);
        let attr = dir.attr().clone();

        self.fs.write().unwrap().push(dirid, dir)?;

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

        self.fs.write().unwrap().push(dirid, file)?;

        Ok((newid, attr))
    }

    fn lookup_impl(&self, dirid: fileid3, filename: &filename3) -> Result<fileid3, nfsstat3> {
        let fs = self.fs.read().unwrap();
        let entry = fs.get(dirid).ok_or(nfsstat3::NFS3ERR_NOENT)?;

        if let Entry::File(_) = entry {
            return Err(nfsstat3::NFS3ERR_NOTDIR);
        } else if let Entry::Dir(dir) = &entry {
            // if looking for dir/. its the current directory
            if filename == ".".as_bytes() {
                return Ok(dirid);
            }
            // if looking for dir/.. its the parent directory
            if filename == "..".as_bytes() {
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
        let fs = self.fs.read().unwrap();
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

#[async_trait::async_trait]
impl NFSFileSystem for MemFs {
    fn capabilities(&self) -> VFSCapabilities {
        VFSCapabilities::ReadWrite
    }

    fn root_dir(&self) -> fileid3 {
        self.rootdir
    }

    async fn lookup(&self, dirid: fileid3, filename: &filename3) -> Result<fileid3, nfsstat3> {
        self.lookup_impl(dirid, filename)
    }

    async fn getattr(&self, id: fileid3) -> Result<fattr3, nfsstat3> {
        let fs = self.fs.read().unwrap();
        let entry = fs.get(id).ok_or(nfsstat3::NFS3ERR_NOENT)?;
        Ok(entry.attr().clone())
    }

    async fn setattr(&self, id: fileid3, setattr: sattr3) -> Result<fattr3, nfsstat3> {
        let mut fs = self.fs.write().unwrap();
        let entry = fs.get_mut(id).ok_or(nfsstat3::NFS3ERR_NOENT)?;
        entry.set_attr(setattr);
        Ok(entry.attr().clone())
    }

    async fn read(
        &self,
        id: fileid3,
        offset: u64,
        count: u32,
    ) -> Result<(Vec<u8>, bool), nfsstat3> {
        let fs = self.fs.read().unwrap();
        let entry = fs.get(id).ok_or(nfsstat3::NFS3ERR_NOENT)?;
        let file = entry.as_file()?;
        file.read(offset, count)
    }
    async fn write(&self, id: fileid3, offset: u64, data: &[u8]) -> Result<fattr3, nfsstat3> {
        let mut fs = self.fs.write().unwrap();

        let entry = fs.get_mut(id).ok_or(nfsstat3::NFS3ERR_NOENT)?;
        let file = entry.as_file_mut()?;
        file.write(offset, data)
    }

    async fn create(
        &self,
        dirid: fileid3,
        filename: &filename3,
        attr: sattr3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        self.add_file(dirid, filename.clone_to_owned(), attr, Vec::new())
    }

    async fn create_exclusive(
        &self,
        _dirid: fileid3,
        _filename: &filename3,
    ) -> Result<fileid3, nfsstat3> {
        tracing::warn!("create_exclusive not implemented");
        Err(nfsstat3::NFS3ERR_NOTSUPP)
    }

    async fn mkdir(
        &self,
        dirid: fileid3,
        dirname: &filename3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        self.add_dir(dirid, dirname.clone_to_owned())
    }

    async fn remove(&self, dirid: fileid3, filename: &filename3) -> Result<(), nfsstat3> {
        self.fs.write().unwrap().remove(dirid, filename)
    }

    async fn rename(
        &self,
        _from_dirid: fileid3,
        _from_filename: &filename3,
        _to_dirid: fileid3,
        _to_filename: &filename3,
    ) -> Result<(), nfsstat3> {
        tracing::warn!("rename not implemented");
        return Err(nfsstat3::NFS3ERR_NOTSUPP);
    }

    async fn readdir(
        &self,
        dirid: fileid3,
        start_after: fileid3,
    ) -> Result<Box<dyn ReadDirIterator>, nfsstat3> {
        let iter = Self::make_iter(self, dirid, start_after)?;
        Ok(Box::new(iter))
    }

    async fn readdirplus(
        &self,
        dirid: fileid3,
        start_after: fileid3,
    ) -> Result<Box<dyn ReadDirPlusIterator>, nfsstat3> {
        let iter = Self::make_iter(self, dirid, start_after)?;
        Ok(Box::new(iter))
    }

    async fn symlink(
        &self,
        _dirid: fileid3,
        _linkname: &filename3,
        _symlink: &nfspath3,
        _attr: &sattr3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        tracing::warn!("symlink not implemented");
        Err(nfsstat3::NFS3ERR_NOTSUPP)
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
        return Err(nfsstat3::NFS3ERR_NOTSUPP);
    }

    async fn path_to_id(&self, path: &str) -> Result<fileid3, nfsstat3> {
        self.path_to_id_impl(path)
    }
}

struct MemFsIterator {
    fs: Arc<RwLock<Fs>>,
    entries: Vec<fileid3>,
    index: usize,
}

impl MemFsIterator {
    fn new(fs: Arc<RwLock<Fs>>, entries: Vec<fileid3>) -> Self {
        Self {
            fs,
            entries,
            index: 0,
        }
    }
}

#[async_trait::async_trait]
impl ReadDirPlusIterator for MemFsIterator {
    async fn next(&mut self) -> Result<nfs::entryplus3<'static>, nfsstat3> {
        if self.index >= self.entries.len() {
            return Err(nfsstat3::NFS3ERR_NOENT);
        }
        let id = self.entries[self.index];
        self.index += 1;

        let fs = self.fs.read().unwrap();
        let entry = fs.get(id).ok_or(nfsstat3::NFS3ERR_NOENT)?;
        let attr = entry.attr().clone();
        let fh = DEFAULT_FH_CONVERTER.id_to_fh(id);
        Ok(nfs::entryplus3 {
            fileid: id,
            name: entry.name().clone_to_owned(),
            cookie: id,
            name_attributes: nfs::post_op_attr::Some(attr),
            name_handle: nfs::post_op_fh3::Some(fh),
        })
    }

    fn eof(&self) -> bool {
        self.index >= self.entries.len()
    }
}

#[derive(Debug, Clone)]
struct MemFsConfigEntry {
    parent: String,
    name: String,
    is_dir: bool,
    content: Vec<u8>,
}

#[derive(Default, Debug, Clone)]
pub struct MemFsConfig {
    entries: Vec<MemFsConfigEntry>,
}

impl MemFsConfig {
    pub fn add_dir(&mut self, path: &str) {
        let name = path.split(DELIMITER).next_back().unwrap().to_string();
        let path = path.trim_end_matches(&name);
        self.entries.push(MemFsConfigEntry {
            parent: path.to_string(),
            name,
            is_dir: true,
            content: Vec::new(),
        });
    }

    pub fn add_file(&mut self, path: &str, content: &[u8]) {
        let name = path.split(DELIMITER).next_back().unwrap().to_string();
        let path = path.trim_end_matches(&name);
        self.entries.push(MemFsConfigEntry {
            parent: path.to_string(),
            name,
            is_dir: false,
            content: content.to_vec(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fs_config() {
        let mut config = MemFsConfig::default();
        config.add_file("/a.txt", "hello world\n".as_bytes());
        config.add_file("/b.txt", "Greetings to xet data\n".as_bytes());
        config.add_dir("/another_dir");
        config.add_file("/another_dir/thisworks.txt", "i hope\n".as_bytes());

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
