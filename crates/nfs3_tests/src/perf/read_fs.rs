use nfs3_server::vfs::{
    FileHandleU64, NfsFileSystem, NfsReadFileSystem, ReadDirPlusIterator, VFSCapabilities,
};

use crate::perf::nfs3_types::nfs3::{
    createverf3, fattr3, filename3, nfspath3, nfsstat3, sattr3, stable_how,
};
use crate::perf::{EmptyDirIter, FILE_HANDLE, ROOT_HANDLE, dir_attr, file_attr};

/// Read-only VFS that always returns a prefilled buffer.
///
/// Every `read` call returns a slice of the same pre-allocated data.
/// All lookups resolve to a single file handle — no handle acquisition needed.
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
