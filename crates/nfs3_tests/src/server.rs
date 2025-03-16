use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use nfs3_server::test_reexports::{RPCContext, TransactionTracker};
use nfs3_server::vfs::{DirEntry, NFSFileSystem, ReadDirResult, VFSCapabilities};
use nfs3_types::nfs3::{
    self as nfs, fattr3, fileid3, filename3, ftype3, nfs_fh3, nfspath3, nfsstat3, nfstime3, sattr3,
    specdata3,
};
use nfs3_types::xdr_codec::Opaque;

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
pub struct TestFs {
    fs: Arc<RwLock<Fs>>,
    rootdir: fileid3,
    nextid: AtomicU64,
}

impl Default for TestFs {
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

#[async_trait]
impl NFSFileSystem for TestFs {
    fn capabilities(&self) -> VFSCapabilities {
        VFSCapabilities::ReadWrite
    }

    fn root_dir(&self) -> fileid3 {
        self.rootdir
    }

    async fn lookup(&self, dirid: fileid3, filename: &filename3) -> Result<fileid3, nfsstat3> {
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

    async fn getattr(&self, id: fileid3) -> Result<fattr3, nfsstat3> {
        let fs = self.fs.read().unwrap();
        let entry = fs.get(id).ok_or(nfsstat3::NFS3ERR_NOENT)?;
        Ok(entry.attr().clone())
    }

    async fn setattr(&self, id: fileid3, setattr: sattr3) -> Result<fattr3, nfsstat3> {
        let mut fs = self.fs.write().unwrap();
        let mut entry = fs.get_mut(id).ok_or(nfsstat3::NFS3ERR_NOENT)?;
        {
            let attr = entry.attr_mut();
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
            if let Entry::File(file) = &mut entry {
                file.resize(s);
            }
        }
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
        _attr: sattr3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        let newid = self
            .nextid
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let file = Entry::new_file(filename.clone_to_owned(), newid, dirid, Vec::new());
        let attr = file.attr().clone();

        self.fs.write().unwrap().push(dirid, file)?;

        Ok((newid, attr))
    }

    async fn create_exclusive(
        &self,
        _dirid: fileid3,
        _filename: &filename3,
    ) -> Result<fileid3, nfsstat3> {
        Err(nfsstat3::NFS3ERR_NOTSUPP)
    }

    async fn mkdir(
        &self,
        dirid: fileid3,
        dirname: &filename3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        let newid = self
            .nextid
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let dir = Entry::new_dir(dirname.clone_to_owned(), newid, dirid);
        let attr = dir.attr().clone();

        self.fs.write().unwrap().push(dirid, dir)?;

        Ok((newid, attr))
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
        return Err(nfsstat3::NFS3ERR_NOTSUPP);
    }

    async fn readdir(
        &self,
        dirid: fileid3,
        start_after: fileid3,
        max_entries: usize,
    ) -> Result<ReadDirResult<'static>, nfsstat3> {
        let fs = self.fs.read().unwrap();
        let entry = fs.get(dirid).ok_or(nfsstat3::NFS3ERR_NOENT)?;
        let dir = entry.as_dir()?;
        let mut ret = ReadDirResult {
            entries: Vec::new(),
            end: false,
        };
        let mut iter = dir.content.iter();

        if start_after != 0 {
            loop {
                if let Some(i) = iter.next() {
                    if *i == start_after {
                        break;
                    }
                } else {
                    return Err(nfsstat3::NFS3ERR_BAD_COOKIE);
                }
            }
        }

        while ret.entries.len() < max_entries {
            let next_id = iter.next();
            if next_id.is_none() {
                break;
            }
            let i = next_id.unwrap();
            let entry = fs.get(*i).ok_or(nfsstat3::NFS3ERR_SERVERFAULT)?;

            ret.entries.push(DirEntry {
                fileid: *i,
                name: entry.name().clone_to_owned(),
                attr: entry.attr().clone(),
            });
        }
        ret.end = iter.next().is_none();
        Ok(ret)
    }

    async fn symlink(
        &self,
        _dirid: fileid3,
        _linkname: &filename3,
        _symlink: &nfspath3,
        _attr: &sattr3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        Err(nfsstat3::NFS3ERR_NOTSUPP)
    }
    async fn readlink(&self, _id: fileid3) -> Result<nfspath3, nfsstat3> {
        return Err(nfsstat3::NFS3ERR_NOTSUPP);
    }
}

pub struct Server<IO> {
    context: RPCContext,
    io: IO,
}

impl<IO> Server<IO>
where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + 'static,
{
    pub async fn new(io: IO, config: FsConfig) -> anyhow::Result<Self> {
        let test_fs = Arc::new(TestFs::default());
        // let test_fs = Arc::new(crate::wasm_fs::new_mem_fs());

        let context = RPCContext {
            local_port: 2049,
            client_addr: "localhost".to_string(),
            auth: nfs3_types::rpc::auth_unix::default(),
            vfs: test_fs,
            mount_signal: None,
            export_name: Arc::new("/mnt".to_string()),
            transaction_tracker: Arc::new(TransactionTracker::new(Duration::from_secs(60))),
        };

        let this = Self { context, io };
        this.init_fs(config).await?;
        Ok(this)
    }

    pub fn root_dir(&self) -> nfs_fh3 {
        self.context.vfs.id_to_fh(self.context.vfs.root_dir())
    }

    async fn init_fs(&self, config: FsConfig) -> anyhow::Result<()> {
        let fs = &self.context.vfs;
        for entry in config.entries {
            let id = fs.path_to_id(&entry.parent).await.map_err(|e| {
                anyhow::anyhow!(
                    "failed to resolve path ({}) to fileid. Code: {e:?}",
                    entry.parent
                )
            })?;

            let name = filename3(Opaque::owned(entry.name.into_bytes()));
            if entry.is_dir {
                fs.mkdir(id, &name).await.unwrap();
            } else {
                let (fileid, _) = fs.create(id, &name, sattr3::default()).await.unwrap();
                fs.write(fileid, 0, &entry.content).await.unwrap();
            }
        }
        Ok(())
    }

    pub async fn run(self) -> Result<(), anyhow::Error> {
        nfs3_server::test_reexports::process_socket(self.io, self.context).await
    }
}

#[derive(Default, Debug, Clone)]
pub struct FsConfig {
    entries: Vec<FsConfigEntry>,
}

#[derive(Debug, Clone)]
struct FsConfigEntry {
    parent: String,
    name: String,
    is_dir: bool,
    content: Vec<u8>,
}

impl FsConfig {
    pub fn add_dir(&mut self, path: &str) {
        let name = path.split('/').next_back().unwrap().to_string();
        let path = path.trim_end_matches(&name);
        self.entries.push(FsConfigEntry {
            parent: path.to_string(),
            name,
            is_dir: true,
            content: Vec::new(),
        });
    }

    pub fn add_file(&mut self, path: &str, content: &[u8]) {
        let name = path.split('/').next_back().unwrap().to_string();
        let path = path.trim_end_matches(&name);
        self.entries.push(FsConfigEntry {
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
        let mut config = FsConfig::default();
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
