use nfs3_server::nfs3_types;
use nfs3_server::vfs::{
    DirEntryPlus, FileHandleU64, NextResult, NfsFileSystem, NfsReadFileSystem, ReadDirPlusIterator,
    VFSCapabilities,
};
use nfs3_types::nfs3::{
    createverf3, fattr3, filename3, ftype3, nfspath3, nfsstat3, nfstime3, sattr3, specdata3,
    stable_how,
};

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

const ROOT_HANDLE: FileHandleU64 = FileHandleU64::new(1);
const FILE_HANDLE: FileHandleU64 = FileHandleU64::new(2);

fn dir_attr(fileid: u64) -> fattr3 {
    fattr3 {
        type_: ftype3::NF3DIR,
        mode: 0o755,
        nlink: 2,
        uid: 0,
        gid: 0,
        size: 4096,
        used: 4096,
        rdev: specdata3::default(),
        fsid: 1,
        fileid,
        atime: nfstime3::default(),
        mtime: nfstime3::default(),
        ctime: nfstime3::default(),
    }
}

fn file_attr(fileid: u64, size: u64) -> fattr3 {
    fattr3 {
        type_: ftype3::NF3REG,
        mode: 0o644,
        nlink: 1,
        uid: 0,
        gid: 0,
        size,
        used: size,
        rdev: specdata3::default(),
        fsid: 1,
        fileid,
        atime: nfstime3::default(),
        mtime: nfstime3::default(),
        ctime: nfstime3::default(),
    }
}

// ---------------------------------------------------------------------------
// ReadFs — always returns a prefilled buffer
// ---------------------------------------------------------------------------

pub struct ReadFs {
    data: Vec<u8>,
}

impl ReadFs {
    pub fn new(size: usize) -> Self {
        Self {
            data: vec![0xAA; size],
        }
    }
}

impl NfsReadFileSystem for ReadFs {
    type Handle = FileHandleU64;

    fn root_dir(&self) -> FileHandleU64 {
        ROOT_HANDLE
    }

    async fn lookup(
        &self,
        _dirid: &FileHandleU64,
        _filename: &filename3<'_>,
    ) -> Result<FileHandleU64, nfsstat3> {
        Ok(FILE_HANDLE)
    }

    async fn getattr(&self, id: &FileHandleU64) -> Result<fattr3, nfsstat3> {
        if *id == ROOT_HANDLE {
            Ok(dir_attr(ROOT_HANDLE.as_u64()))
        } else {
            Ok(file_attr(FILE_HANDLE.as_u64(), self.data.len() as u64))
        }
    }

    async fn read(
        &self,
        _id: &FileHandleU64,
        offset: u64,
        count: u32,
    ) -> Result<(Vec<u8>, bool), nfsstat3> {
        let start = (offset as usize).min(self.data.len());
        let end = (start + count as usize).min(self.data.len());
        let eof = end >= self.data.len();
        Ok((self.data[start..end].to_vec(), eof))
    }

    async fn readdirplus(
        &self,
        _dirid: &FileHandleU64,
        _cookie: u64,
    ) -> Result<impl ReadDirPlusIterator<FileHandleU64>, nfsstat3> {
        Ok(EmptyDirIter)
    }

    async fn readlink(&self, _id: &FileHandleU64) -> Result<nfspath3<'_>, nfsstat3> {
        Err(nfsstat3::NFS3ERR_INVAL)
    }
}

impl NfsFileSystem for ReadFs {
    fn capabilities(&self) -> VFSCapabilities {
        VFSCapabilities::ReadOnly
    }

    async fn setattr(&self, _id: &FileHandleU64, _setattr: sattr3) -> Result<fattr3, nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn write(
        &self,
        _id: &FileHandleU64,
        _offset: u64,
        _data: &[u8],
        _stable: stable_how,
    ) -> Result<(fattr3, stable_how), nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn create(
        &self,
        _dirid: &FileHandleU64,
        _filename: &filename3<'_>,
        _attr: sattr3,
    ) -> Result<(FileHandleU64, fattr3), nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn create_exclusive(
        &self,
        _dirid: &FileHandleU64,
        _filename: &filename3<'_>,
        _createverf: createverf3,
    ) -> Result<FileHandleU64, nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn mkdir(
        &self,
        _dirid: &FileHandleU64,
        _dirname: &filename3<'_>,
    ) -> Result<(FileHandleU64, fattr3), nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn remove(
        &self,
        _dirid: &FileHandleU64,
        _filename: &filename3<'_>,
    ) -> Result<(), nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn rename<'a>(
        &self,
        _from_dirid: &FileHandleU64,
        _from_filename: &filename3<'a>,
        _to_dirid: &FileHandleU64,
        _to_filename: &filename3<'a>,
    ) -> Result<(), nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn symlink<'a>(
        &self,
        _dirid: &FileHandleU64,
        _linkname: &filename3<'a>,
        _symlink: &nfspath3<'a>,
        _attr: &sattr3,
    ) -> Result<(FileHandleU64, fattr3), nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn commit(&self, _id: &FileHandleU64, _offset: u64, _count: u32) -> Result<(), nfsstat3> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// WriteFs — drops all writes, always reports success
// ---------------------------------------------------------------------------

pub struct WriteFs {
    file_size: u64,
}

impl WriteFs {
    pub fn new(file_size: u64) -> Self {
        Self { file_size }
    }
}

impl NfsReadFileSystem for WriteFs {
    type Handle = FileHandleU64;

    fn root_dir(&self) -> FileHandleU64 {
        ROOT_HANDLE
    }

    async fn lookup(
        &self,
        _dirid: &FileHandleU64,
        _filename: &filename3<'_>,
    ) -> Result<FileHandleU64, nfsstat3> {
        Ok(FILE_HANDLE)
    }

    async fn getattr(&self, id: &FileHandleU64) -> Result<fattr3, nfsstat3> {
        if *id == ROOT_HANDLE {
            Ok(dir_attr(ROOT_HANDLE.as_u64()))
        } else {
            Ok(file_attr(FILE_HANDLE.as_u64(), self.file_size))
        }
    }

    async fn read(
        &self,
        _id: &FileHandleU64,
        _offset: u64,
        _count: u32,
    ) -> Result<(Vec<u8>, bool), nfsstat3> {
        Err(nfsstat3::NFS3ERR_INVAL)
    }

    async fn readdirplus(
        &self,
        _dirid: &FileHandleU64,
        _cookie: u64,
    ) -> Result<impl ReadDirPlusIterator<FileHandleU64>, nfsstat3> {
        Ok(EmptyDirIter)
    }

    async fn readlink(&self, _id: &FileHandleU64) -> Result<nfspath3<'_>, nfsstat3> {
        Err(nfsstat3::NFS3ERR_INVAL)
    }
}

impl NfsFileSystem for WriteFs {
    async fn setattr(&self, _id: &FileHandleU64, _setattr: sattr3) -> Result<fattr3, nfsstat3> {
        Ok(file_attr(FILE_HANDLE.as_u64(), self.file_size))
    }

    async fn write(
        &self,
        _id: &FileHandleU64,
        _offset: u64,
        _data: &[u8],
        _stable: stable_how,
    ) -> Result<(fattr3, stable_how), nfsstat3> {
        Ok((
            file_attr(FILE_HANDLE.as_u64(), self.file_size),
            stable_how::FILE_SYNC,
        ))
    }

    async fn create(
        &self,
        _dirid: &FileHandleU64,
        _filename: &filename3<'_>,
        _attr: sattr3,
    ) -> Result<(FileHandleU64, fattr3), nfsstat3> {
        Ok((FILE_HANDLE, file_attr(FILE_HANDLE.as_u64(), 0)))
    }

    async fn create_exclusive(
        &self,
        _dirid: &FileHandleU64,
        _filename: &filename3<'_>,
        _createverf: createverf3,
    ) -> Result<FileHandleU64, nfsstat3> {
        Ok(FILE_HANDLE)
    }

    async fn mkdir(
        &self,
        _dirid: &FileHandleU64,
        _dirname: &filename3<'_>,
    ) -> Result<(FileHandleU64, fattr3), nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn remove(
        &self,
        _dirid: &FileHandleU64,
        _filename: &filename3<'_>,
    ) -> Result<(), nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn rename<'a>(
        &self,
        _from_dirid: &FileHandleU64,
        _from_filename: &filename3<'a>,
        _to_dirid: &FileHandleU64,
        _to_filename: &filename3<'a>,
    ) -> Result<(), nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn symlink<'a>(
        &self,
        _dirid: &FileHandleU64,
        _linkname: &filename3<'a>,
        _symlink: &nfspath3<'a>,
        _attr: &sattr3,
    ) -> Result<(FileHandleU64, fattr3), nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn commit(&self, _id: &FileHandleU64, _offset: u64, _count: u32) -> Result<(), nfsstat3> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ListFs — returns predefined directory entries with known chunk sizes
// ---------------------------------------------------------------------------

pub struct ListFs {
    /// Prebuilt directory entries. The server knows sizes upfront.
    entries: Vec<DirEntryPlus<FileHandleU64>>,
}

impl ListFs {
    /// Create a ListFs with `count` pre-generated directory entries.
    pub fn new(count: usize) -> Self {
        let entries: Vec<_> = (0..count)
            .map(|i| {
                let fileid = (i + 10) as u64;
                let name = format!("entry_{i:05}");
                DirEntryPlus {
                    fileid,
                    name: filename3::from(name.into_bytes()),
                    cookie: (i + 1) as u64,
                    name_attributes: Some(file_attr(fileid, 1024)),
                    name_handle: Some(FileHandleU64::new(fileid)),
                }
            })
            .collect();
        Self { entries }
    }
}

impl NfsReadFileSystem for ListFs {
    type Handle = FileHandleU64;

    fn root_dir(&self) -> FileHandleU64 {
        ROOT_HANDLE
    }

    async fn lookup(
        &self,
        _dirid: &FileHandleU64,
        _filename: &filename3<'_>,
    ) -> Result<FileHandleU64, nfsstat3> {
        Ok(FILE_HANDLE)
    }

    async fn getattr(&self, id: &FileHandleU64) -> Result<fattr3, nfsstat3> {
        if *id == ROOT_HANDLE {
            Ok(dir_attr(ROOT_HANDLE.as_u64()))
        } else {
            Ok(file_attr(id.as_u64(), 1024))
        }
    }

    async fn read(
        &self,
        _id: &FileHandleU64,
        _offset: u64,
        _count: u32,
    ) -> Result<(Vec<u8>, bool), nfsstat3> {
        Err(nfsstat3::NFS3ERR_ISDIR)
    }

    async fn readdirplus(
        &self,
        _dirid: &FileHandleU64,
        cookie: u64,
    ) -> Result<impl ReadDirPlusIterator<FileHandleU64>, nfsstat3> {
        let start = if cookie == 0 {
            0
        } else {
            cookie as usize // cookie = 1-based index
        };
        Ok(PrebuiltDirIter {
            entries: &self.entries,
            pos: start,
        })
    }

    async fn readlink(&self, _id: &FileHandleU64) -> Result<nfspath3<'_>, nfsstat3> {
        Err(nfsstat3::NFS3ERR_INVAL)
    }
}

impl NfsFileSystem for ListFs {
    fn capabilities(&self) -> VFSCapabilities {
        VFSCapabilities::ReadOnly
    }

    async fn setattr(&self, _id: &FileHandleU64, _setattr: sattr3) -> Result<fattr3, nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn write(
        &self,
        _id: &FileHandleU64,
        _offset: u64,
        _data: &[u8],
        _stable: stable_how,
    ) -> Result<(fattr3, stable_how), nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn create(
        &self,
        _dirid: &FileHandleU64,
        _filename: &filename3<'_>,
        _attr: sattr3,
    ) -> Result<(FileHandleU64, fattr3), nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn create_exclusive(
        &self,
        _dirid: &FileHandleU64,
        _filename: &filename3<'_>,
        _createverf: createverf3,
    ) -> Result<FileHandleU64, nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn mkdir(
        &self,
        _dirid: &FileHandleU64,
        _dirname: &filename3<'_>,
    ) -> Result<(FileHandleU64, fattr3), nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn remove(
        &self,
        _dirid: &FileHandleU64,
        _filename: &filename3<'_>,
    ) -> Result<(), nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn rename<'a>(
        &self,
        _from_dirid: &FileHandleU64,
        _from_filename: &filename3<'a>,
        _to_dirid: &FileHandleU64,
        _to_filename: &filename3<'a>,
    ) -> Result<(), nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn symlink<'a>(
        &self,
        _dirid: &FileHandleU64,
        _linkname: &filename3<'a>,
        _symlink: &nfspath3<'a>,
        _attr: &sattr3,
    ) -> Result<(FileHandleU64, fattr3), nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn commit(&self, _id: &FileHandleU64, _offset: u64, _count: u32) -> Result<(), nfsstat3> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Iterator implementations
// ---------------------------------------------------------------------------

struct EmptyDirIter;

impl ReadDirPlusIterator<FileHandleU64> for EmptyDirIter {
    async fn next(&mut self) -> NextResult<DirEntryPlus<FileHandleU64>> {
        NextResult::Eof
    }
}

struct PrebuiltDirIter<'a> {
    entries: &'a [DirEntryPlus<FileHandleU64>],
    pos: usize,
}

impl ReadDirPlusIterator<FileHandleU64> for PrebuiltDirIter<'_> {
    async fn next(&mut self) -> NextResult<DirEntryPlus<FileHandleU64>> {
        if self.pos >= self.entries.len() {
            return NextResult::Eof;
        }
        let entry = self.entries[self.pos].clone();
        self.pos += 1;
        NextResult::Ok(entry)
    }
}
