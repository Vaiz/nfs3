use nfs3_types::portmap::{PMAP_PROG, PROGRAM, VERSION};
use nfs3_types::rpc::accepted_reply;
use nfs3_types::xdr_codec::{Pack, PackedSize, Unpack};

use crate::io::{AsyncRead, AsyncWrite};
use crate::rpc::RpcClient;

struct Null;

impl<Out: std::io::Write> Pack<Out> for Null {
    fn pack(&self, _buf: &mut Out) -> nfs3_types::xdr_codec::Result<usize> {
        Ok(0)
    }
}

impl PackedSize for Null {
    const PACKED_SIZE: Option<usize> = Some(0);

    fn count_packed_size(&self) -> usize {
        0
    }
}

impl<In: std::io::Read> Unpack<In> for Null {
    fn unpack(_buf: &mut In) -> nfs3_types::xdr_codec::Result<(Null, usize)> {
        Ok((Null, 0))
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
        let _reply = self.call(PMAP_PROG::PMAPPROC_NULL, Null).await?;

        Ok(())
    }

    async fn call<T: Pack<Vec<u8>> + PackedSize>(
        &mut self,
        proc: PMAP_PROG,
        args: T,
    ) -> Result<accepted_reply<'static>, crate::error::Error> {
        self.rpc.call(PROGRAM, VERSION, proc as u32, args).await
    }
}
