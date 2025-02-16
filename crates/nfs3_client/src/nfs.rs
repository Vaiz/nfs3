use std::io::Cursor;

use nfs3_types::mount::{dirpath, mountres3_ok};
use nfs3_types::nfs3::*;
use nfs3_types::xdr_codec::{Opaque, Pack, PackedSize, Unpack, Void};

use crate::error::Error;
use crate::io::{AsyncRead, AsyncWrite};
use crate::rpc::RpcClient;
use crate::{mount, portmapper};

pub struct Nfs3Client<IO> {
    rpc: RpcClient<IO>,
}

impl<IO> Nfs3Client<IO>
where
    IO: AsyncRead + AsyncWrite,
{
    pub fn new(rpc: RpcClient<IO>) -> Self {
        Self { rpc }
    }

    pub async fn null(&mut self) -> Result<(), Error> {
        let _ = self
            .call::<Void, Void>(NFS_PROGRAM::NFSPROC3_NULL, Void)
            .await?;
        Ok(())
    }

    pub async fn getattr(&mut self, args: GETATTR3args) -> Result<GETATTR3res, Error> {
        self.call::<GETATTR3args, GETATTR3res>(NFS_PROGRAM::NFSPROC3_GETATTR, args)
            .await
    }

    pub async fn setattr(&mut self, args: SETATTR3args) -> Result<SETATTR3res, Error> {
        self.call::<SETATTR3args, SETATTR3res>(NFS_PROGRAM::NFSPROC3_SETATTR, args)
            .await
    }

    pub async fn lookup(&mut self, args: LOOKUP3args<'_>) -> Result<LOOKUP3res, Error> {
        self.call::<LOOKUP3args, LOOKUP3res>(NFS_PROGRAM::NFSPROC3_LOOKUP, args)
            .await
    }

    pub async fn access(&mut self, args: ACCESS3args) -> Result<ACCESS3res, Error> {
        self.call::<ACCESS3args, ACCESS3res>(NFS_PROGRAM::NFSPROC3_ACCESS, args)
            .await
    }

    pub async fn readlink(&mut self, args: READLINK3args) -> Result<READLINK3res<'static>, Error> {
        self.call::<READLINK3args, READLINK3res>(NFS_PROGRAM::NFSPROC3_READLINK, args)
            .await
    }

    pub async fn read(&mut self, args: READ3args) -> Result<READ3res<'static>, Error> {
        self.call::<READ3args, READ3res>(NFS_PROGRAM::NFSPROC3_READ, args)
            .await
    }

    pub async fn write(&mut self, args: WRITE3args<'_>) -> Result<WRITE3res, Error> {
        self.call::<WRITE3args, WRITE3res>(NFS_PROGRAM::NFSPROC3_WRITE, args)
            .await
    }

    pub async fn create(&mut self, args: CREATE3args<'_>) -> Result<CREATE3res, Error> {
        self.call::<CREATE3args, CREATE3res>(NFS_PROGRAM::NFSPROC3_CREATE, args)
            .await
    }

    pub async fn mkdir(&mut self, args: MKDIR3args<'_>) -> Result<MKDIR3res, Error> {
        self.call::<MKDIR3args, MKDIR3res>(NFS_PROGRAM::NFSPROC3_MKDIR, args)
            .await
    }

    pub async fn symlink(&mut self, args: SYMLINK3args<'_>) -> Result<SYMLINK3res, Error> {
        self.call::<SYMLINK3args, SYMLINK3res>(NFS_PROGRAM::NFSPROC3_SYMLINK, args)
            .await
    }

    pub async fn mknod(&mut self, args: MKNOD3args<'_>) -> Result<MKNOD3res, Error> {
        self.call::<MKNOD3args, MKNOD3res>(NFS_PROGRAM::NFSPROC3_MKNOD, args)
            .await
    }

    pub async fn remove(&mut self, args: REMOVE3args<'_>) -> Result<REMOVE3res, Error> {
        self.call::<REMOVE3args, REMOVE3res>(NFS_PROGRAM::NFSPROC3_REMOVE, args)
            .await
    }

    pub async fn rmdir(&mut self, args: RMDIR3args<'_>) -> Result<RMDIR3res, Error> {
        self.call::<RMDIR3args, RMDIR3res>(NFS_PROGRAM::NFSPROC3_RMDIR, args)
            .await
    }

    pub async fn rename(&mut self, args: RENAME3args<'_, '_>) -> Result<RENAME3res, Error> {
        self.call::<RENAME3args, RENAME3res>(NFS_PROGRAM::NFSPROC3_RENAME, args)
            .await
    }

    pub async fn link(&mut self, args: LINK3args<'_>) -> Result<LINK3res, Error> {
        self.call::<LINK3args, LINK3res>(NFS_PROGRAM::NFSPROC3_LINK, args)
            .await
    }

    pub async fn readdir(&mut self, args: READDIR3args) -> Result<READDIR3res<'static>, Error> {
        self.call::<READDIR3args, READDIR3res>(NFS_PROGRAM::NFSPROC3_READDIR, args)
            .await
    }

    pub async fn readdirplus(
        &mut self,
        args: READDIRPLUS3args,
    ) -> Result<READDIRPLUS3res<'static>, Error> {
        self.call::<READDIRPLUS3args, READDIRPLUS3res>(NFS_PROGRAM::NFSPROC3_READDIRPLUS, args)
            .await
    }

    pub async fn fsstat(&mut self, args: FSSTAT3args) -> Result<FSSTAT3res, Error> {
        self.call::<FSSTAT3args, FSSTAT3res>(NFS_PROGRAM::NFSPROC3_FSSTAT, args)
            .await
    }

    pub async fn fsinfo(&mut self, args: FSINFO3args) -> Result<FSINFO3res, Error> {
        self.call::<FSINFO3args, FSINFO3res>(NFS_PROGRAM::NFSPROC3_FSINFO, args)
            .await
    }

    pub async fn pathconf(&mut self, args: PATHCONF3args) -> Result<PATHCONF3res, Error> {
        self.call::<PATHCONF3args, PATHCONF3res>(NFS_PROGRAM::NFSPROC3_PATHCONF, args)
            .await
    }

    pub async fn commit(&mut self, args: COMMIT3args) -> Result<COMMIT3res, Error> {
        self.call::<COMMIT3args, COMMIT3res>(NFS_PROGRAM::NFSPROC3_COMMIT, args)
            .await
    }

    async fn call<C, R>(&mut self, proc: NFS_PROGRAM, args: C) -> Result<R, crate::error::Error>
    where
        R: Unpack<Cursor<Vec<u8>>>,
        C: Pack<Vec<u8>> + PackedSize,
    {
        self.rpc
            .call::<C, R>(PROGRAM, VERSION, proc as u32, args)
            .await
    }
}

pub async fn connect<C, S>(
    connector: C,
    host: &str,
    mount_path: &str,
) -> Result<(Nfs3Client<S>, mountres3_ok<'static>), Error>
where
    C: crate::net::Connector<Connection = S>,
    S: AsyncRead + AsyncWrite + 'static,
{
    let rpc = connector
        .connect(host, nfs3_types::portmap::PMAP_PORT)
        .await?;
    let rpc = RpcClient::new(rpc);
    let mut portmapper = portmapper::PortmapperClient::new(rpc);

    let mount_port = portmapper
        .getport(nfs3_types::mount::PROGRAM, nfs3_types::mount::VERSION)
        .await?;
    let nfs_port = portmapper
        .getport(nfs3_types::nfs3::PROGRAM, nfs3_types::nfs3::VERSION)
        .await?;

    let mount_rpc = connector.connect(host, mount_port as u16).await?;
    let mount_rpc = RpcClient::new(mount_rpc);
    let mut mount = mount::MountClient::new(mount_rpc);
    let mount_path = Opaque::borrowed(mount_path.as_bytes());
    let mount_res = mount.mnt(dirpath(mount_path)).await?;

    let rpc = connector.connect(host, nfs_port as u16).await?;
    let rpc = RpcClient::new(rpc);
    let nfs3_client = Nfs3Client::new(rpc);

    Ok((nfs3_client, mount_res))
}
