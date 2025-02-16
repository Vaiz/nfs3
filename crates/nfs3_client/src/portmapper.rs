use std::io::Cursor;

use nfs3_types::portmap::{mapping, pmaplist, PMAP_PROG, PROGRAM, VERSION};
use nfs3_types::xdr_codec::{Pack, PackedSize, Unpack, Void};

use crate::io::{AsyncRead, AsyncWrite};
use crate::rpc::RpcClient;

/// Client for the portmapper service
pub struct PortmapperClient<IO> {
    rpc: RpcClient<IO>,
}

impl<IO> PortmapperClient<IO>
where
    IO: AsyncRead + AsyncWrite,
{
    pub fn new(rpc: RpcClient<IO>) -> Self {
        Self { rpc }
    }

    pub async fn null(&mut self) -> Result<(), crate::error::Error> {
        let _ = self
            .call::<Void, Void>(PMAP_PROG::PMAPPROC_NULL, Void)
            .await?;
        Ok(())
    }

    pub async fn getport(&mut self, prog: u32, vers: u32) -> Result<u32, crate::error::Error> {
        let args = mapping {
            prog,
            vers,
            prot: nfs3_types::portmap::IPPROTO_TCP,
            port: 0,
        };

        let port = self
            .call::<mapping, u32>(PMAP_PROG::PMAPPROC_GETPORT, args)
            .await?;

        if port == 0 {
            Err(crate::error::PortmapError::ProgramUnavailable.into())
        } else {
            Ok(port)
        }
    }

    pub async fn dump(&mut self) -> Result<Vec<mapping>, crate::error::Error> {
        let mappings = self
            .call::<Void, pmaplist>(PMAP_PROG::PMAPPROC_DUMP, Void)
            .await?;
        Ok(mappings.into_inner())
    }

    async fn call<C, R>(&mut self, proc: PMAP_PROG, args: C) -> Result<R, crate::error::Error>
    where
        R: Unpack<Cursor<Vec<u8>>>,
        C: Pack<Vec<u8>> + PackedSize,
    {
        self.rpc
            .call::<C, R>(PROGRAM, VERSION, proc as u32, args)
            .await
    }
}
