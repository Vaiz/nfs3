use nfs3_types::portmap::{PMAP_PROG, PROGRAM, VERSION, mapping, pmaplist};
use nfs3_types::xdr_codec::{Pack, Unpack, Void};

use crate::io::{AsyncRead, AsyncWrite};
use crate::rpc::RpcClient;

/// Client for the portmapper service
#[derive(Debug)]
pub struct PortmapperClient<IO> {
    rpc: RpcClient<IO>,
}

impl<IO> PortmapperClient<IO>
where
    IO: AsyncRead + AsyncWrite + Send,
{
    pub fn new(io: IO) -> Self {
        Self {
            rpc: RpcClient::new(io),
        }
    }

    pub async fn null(&mut self) -> Result<(), crate::error::Error> {
        let _ = self
            .call::<Void, Void>(PMAP_PROG::PMAPPROC_NULL, Void)
            .await?;
        Ok(())
    }

    pub async fn getport(&mut self, prog: u32, vers: u32) -> Result<u16, crate::error::Error> {
        let args = mapping {
            prog,
            vers,
            prot: nfs3_types::portmap::IPPROTO_TCP,
            port: 0,
        };

        let port = self
            .call::<mapping, u32>(PMAP_PROG::PMAPPROC_GETPORT, args)
            .await?;

        let port_u16: Result<u16, _> = port.try_into();
        match port_u16 {
            Ok(0) => Err(crate::error::PortmapError::ProgramUnavailable.into()),
            Ok(port) => Ok(port),
            Err(_) => Err(crate::error::PortmapError::InvalidPortValue(port).into()),
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
        R: Unpack,
        C: Pack,
    {
        self.rpc
            .call::<C, R>(PROGRAM, VERSION, proc as u32, &args)
            .await
    }
}
