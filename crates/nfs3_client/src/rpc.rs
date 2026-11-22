//! RPC client implementation

use std::fmt::Debug;
use std::io;

use bytes::BufMut;
use nfs3_types::rpc::{
    RPC_VERSION_2, RecordBuf, accept_stat_data, call_body, msg_body, opaque_auth, reply_body,
    rpc_msg,
};
use nfs3_types::xdr_codec::{Decoder, Encoder, Pack, Unpack};

use crate::RpcError;
use crate::io::{AsyncRead, AsyncWrite};

// Leave room for a 1 MiB READ or directory listing plus RPC and NFS headers.
const MAX_RPC_REPLY_SIZE: usize = 2 * 1024 * 1024;

/// RPC client
pub struct RpcClient<IO> {
    io: IO,
    xid: u32,
    credential: opaque_auth<'static>,
    verifier: opaque_auth<'static>,
}

impl<IO> Debug for RpcClient<IO> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        f.debug_struct("RpcClient").finish()
    }
}

impl<IO> RpcClient<IO>
where
    IO: AsyncRead + AsyncWrite + Send,
{
    /// Create a new RPC client. XID is initialized to a random value.
    pub fn new(io: IO) -> Self {
        Self::new_with_auth(io, opaque_auth::default(), opaque_auth::default())
    }

    /// Create a new RPC client with custom credential and verifier.
    pub fn new_with_auth(
        io: IO,
        credential: opaque_auth<'static>,
        verifier: opaque_auth<'static>,
    ) -> Self {
        Self {
            io,
            xid: fastrand::u32(..),
            credential,
            verifier,
        }
    }

    /// Call an RPC procedure
    ///
    /// # Errors and connection state
    ///
    /// On a successful return the connection is in a clean state and may be
    /// reused for the next call. When an error is returned, use
    /// [`RpcError::is_connection_reusable`] to decide whether the
    /// connection can be kept.
    #[expect(
        clippy::similar_names,
        reason = "prog and proc are fields of call_body"
    )]
    pub async fn call<C, R>(
        &mut self,
        prog: u32,
        vers: u32,
        proc: u32,
        args: &C,
    ) -> Result<R, RpcError>
    where
        R: Unpack,
        C: Pack + Send + Sync,
    {
        let call = call_body {
            rpcvers: RPC_VERSION_2,
            prog,
            vers,
            proc,
            cred: self.credential.borrow(),
            verf: self.verifier.borrow(),
        };
        let msg = rpc_msg {
            xid: self.xid,
            body: msg_body::CALL(call),
        };
        self.xid = self.xid.wrapping_add(1);

        Self::send_call(&mut self.io, &msg, args).await?;
        Self::recv_reply::<R>(&mut self.io, msg.xid).await
    }

    async fn send_call<T>(io: &mut IO, msg: &rpc_msg<'_, '_>, args: &T) -> Result<(), RpcError>
    where
        T: Pack + Send + Sync,
    {
        let total_len = msg.packed_size() + args.packed_size();
        if !total_len.is_multiple_of(4) {
            return Err(RpcError::WrongLength);
        }

        let mut out = Encoder::new(total_len);
        msg.pack(&mut out)?;
        args.pack(&mut out)?;
        if out.len() != total_len {
            return Err(RpcError::WrongLength);
        }
        let mut call = out.finish_record()?;
        io.async_write_all(&mut call).await?;
        Ok(())
    }

    async fn recv_reply<T>(io: &mut IO, xid: u32) -> Result<T, RpcError>
    where
        T: Unpack,
    {
        let mut input = Self::recv_record(io).await?;
        let resp_msg = rpc_msg::unpack(&mut input)?;

        if resp_msg.xid != xid {
            return Err(RpcError::UnexpectedXid);
        }

        let reply = match resp_msg.body {
            msg_body::REPLY(reply_body::MSG_ACCEPTED(reply)) => reply,
            msg_body::REPLY(reply_body::MSG_DENIED(r)) => return Err(RpcError::from(r)),
            msg_body::CALL(_) => return Err(RpcError::UnexpectedCall),
        };

        if !matches!(reply.reply_data, accept_stat_data::SUCCESS) {
            return Err(RpcError::try_from(reply.reply_data)
                .expect("accept_stat_data::SUCCESS is not a valid error"));
        }

        let final_value = T::unpack(&mut input)?;
        if !input.is_empty() {
            return Err(RpcError::NotFullyParsed {
                remaining: input.into_remaining(),
            });
        }
        Ok(final_value)
    }

    /// Reads one RPC record (all fragments) into a single buffer.
    async fn recv_record(io: &mut IO) -> io::Result<Decoder> {
        let mut record = RecordBuf::new(MAX_RPC_REPLY_SIZE);
        while !record.is_complete() {
            let mut marker = [0u8; 4];
            io.async_read_exact(&mut marker).await?;
            record.start_fragment(marker)?;
            while let Some(mut buf) = record.buf_mut() {
                // Read only what fits in the reserved capacity, so memory keeps
                // following the received data.
                let len = buf.chunk_mut().len();
                let buf = buf.get_mut();
                let expected = buf.len() + len;
                io.async_read_exact_buf(buf, len).await?;
                if buf.len() != expected {
                    return Err(io::Error::other(
                        "async_read_exact_buf appended the wrong number of bytes",
                    ));
                }
            }
        }
        Ok(record.into_decoder())
    }
}

#[cfg(test)]
#[path = "tests/rpc.rs"]
mod tests;
