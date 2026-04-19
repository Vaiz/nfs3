use nfs3_server::vfs::{
    DirEntryPlus, FileHandleU64, NextResult, NfsFileSystem, NfsReadFileSystem,
    ReadDirPlusIterator, VFSCapabilities,
};

use crate::perf::nfs3_types::nfs3::{
    createverf3, fattr3, filename3, nfspath3, nfsstat3, sattr3, stable_how,
};
use crate::perf::{FILE_HANDLE, ROOT_HANDLE, dir_attr, file_attr};

/// Directory-listing VFS that returns predefined entries with known chunk sizes.
///
/// All entries are pre-built at construction time so the server knows
/// exact sizes before serialization — no per-request allocation.
pub struct ListFs {
    entries: Vec<DirEntryPlus<FileHandleU64>>,
}

impl ListFs {
    /// Create a `ListFs` with `count` pre-generated directory entries.
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
