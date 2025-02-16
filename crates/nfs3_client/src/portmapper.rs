use std::io::Cursor;

use nfs3_types::portmap::{mapping, PMAP_PROG, PROGRAM, VERSION};
use nfs3_types::xdr_codec::{Pack, PackedSize, Unpack};

use crate::io::{AsyncRead, AsyncWrite};
use crate::rpc::RpcClient;

struct Void;

impl<Out: std::io::Write> Pack<Out> for Void {
    fn pack(&self, _buf: &mut Out) -> nfs3_types::xdr_codec::Result<usize> {
        Ok(0)
    }
}

impl PackedSize for Void {
    const PACKED_SIZE: Option<usize> = Some(0);

    fn count_packed_size(&self) -> usize {
        0
    }
}

impl<In: std::io::Read> Unpack<In> for Void {
    fn unpack(_buf: &mut In) -> nfs3_types::xdr_codec::Result<(Void, usize)> {
        Ok((Void, 0))
    }
}

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
            return Err(crate::error::PortmapError::ProgramUnavailable.into());
        } else {
            return Ok(port);
        }
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
