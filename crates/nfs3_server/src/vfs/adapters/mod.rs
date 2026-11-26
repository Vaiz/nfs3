//! Adapters VFS types

use std::future::ready;

mod iterator;

use bytes::Bytes;
pub use iterator::ReadDirPlusToReadDir;
use nfs3_types::nfs3::{
    FSINFO3resok, cookieverf3, fattr3, filename3, nfsstat3, post_op_attr, sattr3,
};

use super::{
    DirEntryPlus, NextResult, NfsFileSystem, NfsReadFileSystem, ReadDirIterator,
    ReadDirPlusIterator, VFSCapabilities,
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

    async fn access(
        &self,
        id: &Self::Handle,
        auth: Option<&nfs3_types::rpc::auth_unix>,
        requested: u32,
        _capabilities: VFSCapabilities,
    ) -> Result<(fattr3, u32), nfsstat3> {
        let (mut attr, granted) = self
            .0
            .access(id, auth, requested, VFSCapabilities::ReadOnly)
            .await?;
        remove_write_permissions(&mut attr);
        Ok((attr, granted))
    }

    async fn lookup_with_attr(
        &self,
        dirid: &Self::Handle,
        filename: &filename3<'_>,
        auth: Option<&nfs3_types::rpc::auth_unix>,
    ) -> Result<(Self::Handle, post_op_attr, post_op_attr), (nfsstat3, post_op_attr)> {
        match self.0.lookup_with_attr(dirid, filename, auth).await {
            Ok((fid, mut obj_attr, mut dir_attr)) => {
                remove_write_permissions_post_op(&mut obj_attr);
                remove_write_permissions_post_op(&mut dir_attr);
                Ok((fid, obj_attr, dir_attr))
            }
            Err((stat, mut dir_attr)) => {
                remove_write_permissions_post_op(&mut dir_attr);
                Err((stat, dir_attr))
            }
        }
    }

    async fn read_with_attr(
        &self,
        id: &Self::Handle,
        offset: u64,
        count: u32,
        auth: Option<&nfs3_types::rpc::auth_unix>,
    ) -> Result<(post_op_attr, Bytes, bool), (nfsstat3, post_op_attr)> {
        match self.0.read_with_attr(id, offset, count, auth).await {
            Ok((mut attr, bytes, eof)) => {
                remove_write_permissions_post_op(&mut attr);
                Ok((attr, bytes, eof))
            }
            Err((stat, mut attr)) => {
                remove_write_permissions_post_op(&mut attr);
                Err((stat, attr))
            }
        }
    }

    async fn readlink_with_attr(
        &self,
        id: &Self::Handle,
        auth: Option<&nfs3_types::rpc::auth_unix>,
    ) -> Result<(post_op_attr, nfs3_types::nfs3::nfspath3<'_>), (nfsstat3, post_op_attr)> {
        match self.0.readlink_with_attr(id, auth).await {
            Ok((mut attr, target)) => {
                remove_write_permissions_post_op(&mut attr);
                Ok((attr, target))
            }
            Err((stat, mut attr)) => {
                remove_write_permissions_post_op(&mut attr);
                Err((stat, attr))
            }
        }
    }

    async fn readdirplus_verf(
        &self,
        dirid: &Self::Handle,
        cookie: u64,
        cookieverf: cookieverf3,
        auth: Option<&nfs3_types::rpc::auth_unix>,
    ) -> Result<
        (
            post_op_attr,
            cookieverf3,
            impl ReadDirPlusIterator<Self::Handle>,
        ),
        (nfsstat3, post_op_attr),
    > {
        match self
            .0
            .readdirplus_verf(dirid, cookie, cookieverf, auth)
            .await
        {
            Ok((mut attr, verf, iter)) => {
                remove_write_permissions_post_op(&mut attr);
                Ok((attr, verf, ReadOnlyIterator(iter)))
            }
            Err((stat, mut attr)) => {
                remove_write_permissions_post_op(&mut attr);
                Err((stat, attr))
            }
        }
    }

    async fn readdir_verf(
        &self,
        dirid: &Self::Handle,
        cookie: u64,
        cookieverf: cookieverf3,
        auth: Option<&nfs3_types::rpc::auth_unix>,
    ) -> Result<(post_op_attr, cookieverf3, impl ReadDirIterator), (nfsstat3, post_op_attr)> {
        match self.0.readdir_verf(dirid, cookie, cookieverf, auth).await {
            Ok((mut attr, verf, iter)) => {
                remove_write_permissions_post_op(&mut attr);
                Ok((attr, verf, iter))
            }
            Err((stat, mut attr)) => {
                remove_write_permissions_post_op(&mut attr);
                Err((stat, attr))
            }
        }
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

const fn remove_write_permissions_post_op(attr: &mut post_op_attr) {
    if let post_op_attr::Some(attr) = attr {
        remove_write_permissions(attr);
    }
}
