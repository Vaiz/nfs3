use nfs3_server::vfs::{FileHandleU64, NfsFileSystem, NfsReadFileSystem, ReadDirPlusIterator};

use crate::perf::nfs3_types::nfs3::{
    createverf3, fattr3, filename3, nfspath3, nfsstat3, sattr3, stable_how,
};
use crate::perf::{EmptyDirIter, FILE_HANDLE, ROOT_HANDLE, dir_attr, file_attr};

/// Write-sink VFS that drops all writes and always reports success.
///
/// Every `write` call returns immediately with `FILE_SYNC` stability.
/// Useful for benchmarking the write path without any storage overhead.
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
