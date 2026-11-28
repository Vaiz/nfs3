//! Adapters VFS types

use std::future::ready;

mod iterator;

use bytes::Bytes;
pub use iterator::ReadDirPlusToReadDir;
use nfs3_types::mount::mountstat3;
use nfs3_types::nfs3::{FSINFO3resok, fattr3, filename3, nfs_fh3, nfsstat3, post_op_attr, sattr3};

use super::{
    DirEntryPlus, FileHandleConverter, NextResult, NfsFileSystem, NfsReadFileSystem,
    ReadDirIterator, ReadDirPlusIterator, VFSCapabilities,
};
use crate::vfs::FileHandle;

/// An internal adapter that allows to reuse the same code with `ReadOnly` filesystems.
///
/// In general, you should not use this adapter directly. Instead, use the
/// [`Builder::read_only_filesystem`][1] method to configure a read-only NFS server.
///
/// [1]: crate::tcp::Builder::read_only_filesystem
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
    type Handle = T::Handle;

    fn authorize(&self, auth: Option<&nfs3_types::rpc::auth_unix>) -> bool {
        self.0.authorize(auth)
    }

    fn encode_handle(&self, converter: &FileHandleConverter, handle: &Self::Handle) -> nfs_fh3 {
        self.0.encode_handle(converter, handle)
    }

    fn decode_handle(
        &self,
        converter: &FileHandleConverter,
        fh: &nfs_fh3,
    ) -> Result<Self::Handle, nfsstat3> {
        self.0.decode_handle(converter, fh)
    }

    async fn mount(
        &self,
        export_name: &str,
        path: &str,
        exact_export_match: bool,
    ) -> Result<Self::Handle, mountstat3> {
        self.0.mount(export_name, path, exact_export_match).await
    }

    fn root_dir(&self) -> Self::Handle {
        self.0.root_dir()
    }

    async fn lookup(
        &self,
        dirid: &Self::Handle,
        filename: &filename3<'_>,
    ) -> Result<Self::Handle, nfsstat3> {
        self.0.lookup(dirid, filename).await
    }

    async fn getattr(&self, id: &Self::Handle) -> Result<fattr3, nfsstat3> {
        let mut result = self.0.getattr(id).await;
        if let Ok(attr) = &mut result {
            remove_write_permissions(attr);
        }
        result
    }

    async fn read(
        &self,
        id: &Self::Handle,
        offset: u64,
        count: u32,
    ) -> Result<(Bytes, bool), nfsstat3> {
        self.0.read(id, offset, count).await
    }

    async fn readdir(
        &self,
        dirid: &Self::Handle,
        cookie: u64,
    ) -> Result<impl ReadDirIterator, nfsstat3> {
        self.0.readdir(dirid, cookie).await
    }

    async fn readdirplus(
        &self,
        dirid: &Self::Handle,
        cookie: u64,
    ) -> Result<impl ReadDirPlusIterator<Self::Handle>, nfsstat3> {
        self.0
            .readdirplus(dirid, cookie)
            .await
            .map(ReadOnlyIterator)
    }

    async fn readlink(
        &self,
        id: &Self::Handle,
    ) -> Result<nfs3_types::nfs3::nfspath3<'_>, nfsstat3> {
        self.0.readlink(id).await
    }

    async fn fsinfo(&self, root_fileid: &Self::Handle) -> Result<FSINFO3resok, nfsstat3> {
        let mut fsinfo = self.0.fsinfo(root_fileid).await?;
        if let post_op_attr::Some(attr) = &mut fsinfo.obj_attributes {
            remove_write_permissions(attr);
        }
        Ok(fsinfo)
    }
}

impl<T> NfsFileSystem for ReadOnlyAdapter<T>
where
    T: NfsReadFileSystem,
{
    fn capabilities(&self) -> VFSCapabilities {
        VFSCapabilities::ReadOnly
    }

    fn setattr(
        &self,
        _id: &Self::Handle,
        _setattr: sattr3,
    ) -> impl Future<Output = Result<fattr3, nfsstat3>> + Send {
        ready(Err(nfsstat3::NFS3ERR_ROFS))
    }

    fn write(
        &self,
        _id: &Self::Handle,
        _offset: u64,
        _data: Bytes,
        _stable: nfs3_types::nfs3::stable_how,
    ) -> impl Future<Output = Result<(fattr3, nfs3_types::nfs3::stable_how), nfsstat3>> + Send {
        ready(Err(nfsstat3::NFS3ERR_ROFS))
    }

    fn create(
        &self,
        _dirid: &Self::Handle,
        _filename: &filename3<'_>,
        _attr: sattr3,
    ) -> impl Future<Output = Result<(Self::Handle, fattr3), nfsstat3>> + Send {
        ready(Err(nfsstat3::NFS3ERR_ROFS))
    }

    fn create_exclusive(
        &self,
        _dirid: &Self::Handle,
        _filename: &filename3<'_>,
        _createverf: nfs3_types::nfs3::createverf3,
    ) -> impl Future<Output = Result<Self::Handle, nfsstat3>> + Send {
        ready(Err(nfsstat3::NFS3ERR_ROFS))
    }

    fn mkdir(
        &self,
        _dirid: &Self::Handle,
        _dirname: &filename3<'_>,
    ) -> impl Future<Output = Result<(Self::Handle, fattr3), nfsstat3>> + Send {
        ready(Err(nfsstat3::NFS3ERR_ROFS))
    }

    fn remove(
        &self,
        _dirid: &Self::Handle,
        _filename: &filename3<'_>,
    ) -> impl Future<Output = Result<(), nfsstat3>> + Send {
        ready(Err(nfsstat3::NFS3ERR_ROFS))
    }

    fn rename<'a>(
        &self,
        _from_dirid: &Self::Handle,
        _from_filename: &filename3<'a>,
        _to_dirid: &Self::Handle,
        _to_filename: &filename3<'a>,
    ) -> impl Future<Output = Result<(), nfsstat3>> + Send {
        ready(Err(nfsstat3::NFS3ERR_ROFS))
    }

    fn symlink<'a>(
        &self,
        _dirid: &Self::Handle,
        _linkname: &filename3<'a>,
        _symlink: &nfs3_types::nfs3::nfspath3<'a>,
        _attr: &sattr3,
    ) -> impl Future<Output = Result<(Self::Handle, fattr3), nfsstat3>> + Send {
        ready(Err(nfsstat3::NFS3ERR_ROFS))
    }

    fn commit(
        &self,
        _id: &Self::Handle,
        _offset: u64,
        _count: u32,
    ) -> impl Future<Output = Result<(), nfsstat3>> + Send {
        ready(Err(nfsstat3::NFS3ERR_ROFS))
    }
}

#[derive(Debug)]
struct ReadOnlyIterator<T>(T);

impl<H, T> ReadDirPlusIterator<H> for ReadOnlyIterator<T>
where
    H: FileHandle,
    T: ReadDirPlusIterator<H>,
{
    async fn next(&mut self) -> NextResult<DirEntryPlus<H>> {
        let mut result = self.0.next().await;
        if let NextResult::Ok(entry) = &mut result {
            if let Some(attr) = &mut entry.name_attributes {
                remove_write_permissions(attr);
            }
        }
        result
    }
}

const fn remove_write_permissions(attr: &mut fattr3) {
    attr.mode &= 0o555; // Read-only permissions
}
