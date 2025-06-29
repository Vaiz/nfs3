use nfs3_types::mount::{
    MOUNT_PROGRAM, PROGRAM, VERSION, dirpath, exports, mountlist, mountres3, mountres3_ok,
};
use nfs3_types::rpc::opaque_auth;
use nfs3_types::xdr_codec::{Pack, Unpack, Void};

use crate::error::Error;
use crate::io::{AsyncRead, AsyncWrite};
use crate::rpc::RpcClient;

/// Client for the mount service
#[derive(Debug)]
pub struct MountClient<IO> {
    rpc: RpcClient<IO>,
}

impl<IO> MountClient<IO>
where
    IO: AsyncRead + AsyncWrite + Send,
{
    /// Create a new mount client.
    pub fn new(io: IO) -> Self {
        Self {
            rpc: RpcClient::new(io),
        }
    }

    /// Create a new mount client with custom credential and verifier.
    pub fn new_with_auth(
        io: IO,
        credential: opaque_auth<'static>,
        verifier: opaque_auth<'static>,
    ) -> Self {
        Self {
            rpc: RpcClient::new_with_auth(io, credential, verifier),
        }
    }

    pub async fn null(&mut self) -> Result<(), Error> {
        let _ = self
            .call::<Void, Void>(MOUNT_PROGRAM::MOUNTPROC3_NULL, Void)
            .await?;
        Ok(())
    }

    pub async fn mnt(&mut self, dirpath_: dirpath<'_>) -> Result<mountres3_ok<'static>, Error> {
        let result = self
            .call::<dirpath, mountres3>(MOUNT_PROGRAM::MOUNTPROC3_MNT, dirpath_)
            .await?;

        match result {
            mountres3::Ok(ok) => Ok(ok),
            mountres3::Err(err) => Err(Error::MountError(err)),
        }
    }

    pub async fn dump(&mut self) -> Result<mountlist<'static, 'static>, Error> {
        self.call::<Void, mountlist>(MOUNT_PROGRAM::MOUNTPROC3_DUMP, Void)
            .await
    }

    pub async fn umnt(&mut self, dirpath_: dirpath<'_>) -> Result<(), Error> {
        let _ = self
            .call::<dirpath, Void>(MOUNT_PROGRAM::MOUNTPROC3_UMNT, dirpath_)
            .await?;
        Ok(())
    }

    pub async fn umntall(&mut self) -> Result<(), Error> {
        let _ = self
            .call::<Void, Void>(MOUNT_PROGRAM::MOUNTPROC3_UMNTALL, Void)
            .await?;
        Ok(())
    }

    pub async fn export(&mut self) -> Result<exports<'static, 'static>, Error> {
        self.call::<Void, exports>(MOUNT_PROGRAM::MOUNTPROC3_EXPORT, Void)
            .await
    }

    async fn call<C, R>(&mut self, proc: MOUNT_PROGRAM, args: C) -> Result<R, crate::error::Error>
    where
        R: Unpack,
        C: Pack,
    {
        self.rpc
            .call::<C, R>(PROGRAM, VERSION, proc as u32, &args)
            .await
    }
}
