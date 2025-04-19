//! An adapter for read-only NFS filesystems.

use nfs3_types::nfs3::{Nfs3Option, fattr3, fileid3, filename3, nfsstat3, sattr3};

use super::{
    NextResult, NfsFileSystem, NfsReadFileSystem, ReadDirIterator, ReadDirPlusIterator,
    VFSCapabilities,
};

/// An internal adapter that allows to reuse the same code with `ReadOnly` filesystems.
///
/// In general, you should not use this adapter directly. Instead, use the
/// [`NFSTcpListener::bind_ro`][1] method to bind a read-only NFS server.
///
/// [1]: crate::tcp::NFSTcpListener::bind_ro
pub struct ReadOnlyAdapter<T>(T);

impl<T> ReadOnlyAdapter<T>
where
    T: NfsReadFileSystem,
{
    pub const fn new(inner: T) -> Self {
        Self(inner)
    }
}

impl<T> NfsReadFileSystem for ReadOnlyAdapter<T>
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
        let mut result = self.0.getattr(id).await;
        if let Ok(attr) = &mut result {
            remove_write_permissions(attr);
        }
        result
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
        self.0
            .readdirplus(dirid, start_after)
            .await
            .map(ReadOnlyIterator)
    }

    async fn readlink(&self, id: fileid3) -> Result<nfs3_types::nfs3::nfspath3, nfsstat3> {
        self.0.readlink(id).await
    }
}

impl<T> NfsFileSystem for ReadOnlyAdapter<T>
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

#[derive(Debug)]
struct ReadOnlyIterator<T>(T);

impl<T> ReadDirPlusIterator for ReadOnlyIterator<T>
where
    T: ReadDirPlusIterator,
{
    async fn next(&mut self) -> NextResult<nfs3_types::nfs3::entryplus3<'static>> {
        let mut result = self.0.next().await;
        if let NextResult::Ok(entry) = &mut result {
            if let Nfs3Option::Some(attr) = &mut entry.name_attributes {
                remove_write_permissions(attr);
            }
        }
        result
    }
}

const fn remove_write_permissions(attr: &mut fattr3) {
    attr.mode &= 0o555; // Read-only permissions
}
