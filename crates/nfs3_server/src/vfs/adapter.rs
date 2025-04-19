use nfs3_types::nfs3::{fattr3, fileid3, filename3, nfsstat3, sattr3};

use super::{
    NfsFileSystem, NfsReadFileSystem, ReadDirIterator, ReadDirPlusIterator, VFSCapabilities,
};

pub(crate) struct NfsFileSystemAdapter<T>(T);

impl<T> NfsFileSystemAdapter<T>
where
    T: NfsReadFileSystem,
{
    pub fn new(inner: T) -> Self {
        NfsFileSystemAdapter(inner)
    }
}

impl<T> NfsReadFileSystem for NfsFileSystemAdapter<T>
where
    T: NfsReadFileSystem,
{
    fn root_dir(&self) -> fileid3 {
        self.0.root_dir()
    }

    async fn lookup(&self, dirid: fileid3, filename: &filename3<'_>) -> Result<fileid3, nfsstat3> {
        self.0.lookup(dirid, filename).await
    }

    async fn getattr(&self, id: fileid3) -> Result<fattr3, nfsstat3> {
        self.0.getattr(id).await
    }

    async fn read(
        &self,
        id: fileid3,
        offset: u64,
        count: u32,
    ) -> Result<(Vec<u8>, bool), nfsstat3> {
        self.0.read(id, offset, count).await
    }

    async fn readdir(
        &self,
        dirid: fileid3,
        start_after: fileid3,
    ) -> Result<impl ReadDirIterator, nfsstat3> {
        self.0.readdir(dirid, start_after).await
    }

    async fn readdirplus(
        &self,
        dirid: fileid3,
        start_after: fileid3,
    ) -> Result<impl ReadDirPlusIterator, nfsstat3> {
        self.0.readdirplus(dirid, start_after).await
    }

    async fn readlink(&self, id: fileid3) -> Result<nfs3_types::nfs3::nfspath3, nfsstat3> {
        self.0.readlink(id).await
    }
}

impl<T> NfsFileSystem for NfsFileSystemAdapter<T>
where
    T: NfsReadFileSystem,
{
    fn capabilities(&self) -> VFSCapabilities {
        VFSCapabilities::ReadOnly
    }

    async fn setattr(&self, _id: fileid3, _setattr: sattr3) -> Result<fattr3, nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn write(&self, _id: fileid3, _offset: u64, _data: &[u8]) -> Result<fattr3, nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn create(
        &self,
        _dirid: fileid3,
        _filename: &filename3<'_>,
        _attr: sattr3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn create_exclusive(
        &self,
        _dirid: fileid3,
        _filename: &filename3<'_>,
    ) -> Result<fileid3, nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn mkdir(
        &self,
        _dirid: fileid3,
        _dirname: &filename3<'_>,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn remove(&self, _dirid: fileid3, _filename: &filename3<'_>) -> Result<(), nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn rename<'a>(
        &self,
        _from_dirid: fileid3,
        _from_filename: &filename3<'a>,
        _to_dirid: fileid3,
        _to_filename: &filename3<'a>,
    ) -> Result<(), nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }

    async fn symlink<'a>(
        &self,
        _dirid: fileid3,
        _linkname: &filename3<'a>,
        _symlink: &nfs3_types::nfs3::nfspath3<'a>,
        _attr: &sattr3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }
}
