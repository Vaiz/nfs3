use std::io::Cursor;

use nfs3_types::mount::*;
use nfs3_types::xdr_codec::{Pack, PackedSize, Unpack, Void};

use crate::error::Error;
use crate::io::{AsyncRead, AsyncWrite};
use crate::rpc::RpcClient;

pub struct MountClient<IO> {
    rpc: RpcClient<IO>,
}

impl<IO> MountClient<IO>
where
    IO: AsyncRead + AsyncWrite,
{
    pub fn new(rpc: RpcClient<IO>) -> Self {
        Self { rpc }
    }

    pub async fn null(&mut self) -> Result<(), Error> {
        let _ = self
            .call::<Void, Void>(MOUNT_PROGRAM::MOUNTPROC3_NULL, Void)
            .await?;
        Ok(())
    }

    pub async fn mnt<'a>(&mut self, dirpath_: dirpath<'a>) -> Result<mountres3<'a>, Error> {
        self.call::<dirpath, mountres3>(MOUNT_PROGRAM::MOUNTPROC3_MNT, dirpath_)
            .await
    }

    pub async fn dump(&mut self) -> Result<mountlist<'_, '_>, Error> {
        self.call::<Void, mountlist>(MOUNT_PROGRAM::MOUNTPROC3_DUMP, Void)
            .await
    }

    pub async fn umnt<'a>(&mut self, dirpath_: dirpath<'a>) -> Result<(), Error> {
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

    pub async fn export(&mut self) -> Result<exports<'_, '_>, Error> {
        self.call::<Void, exports>(MOUNT_PROGRAM::MOUNTPROC3_EXPORT, Void)
            .await
    }

    async fn call<C, R>(&mut self, proc: MOUNT_PROGRAM, args: C) -> Result<R, crate::error::Error>
    where
        R: Unpack<Cursor<Vec<u8>>>,
        C: Pack<Vec<u8>> + PackedSize,
    {
        self.rpc
            .call::<C, R>(PROGRAM, VERSION, proc as u32, args)
            .await
    }
}
