//! RPC client implementation

use std::fmt::Debug;
use std::io;

use nfs3_types::rpc::{
    RPC_VERSION_2, RecordBuf, accept_stat_data, call_body, msg_body, opaque_auth, reply_body,
    rpc_msg,
};
use nfs3_types::xdr_codec::{Decoder, Encoder, Pack, Unpack};

use crate::RpcError;
use crate::io::{AsyncRead, AsyncWrite};

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

        let mut out = Encoder::with_size_hint(total_len);
        msg.pack(&mut out)?;
        args.pack(&mut out)?;
        if out.len() != total_len {
            return Err(RpcError::WrongLength);
        }
        let mut call = out.finish_record()?;
        io.async_write_all_buf(&mut call).await?;
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
        let mut record = RecordBuf::new(usize::MAX);
        loop {
            let mut marker = [0u8; 4];
            io.async_read_exact(&mut marker).await?;
            record.start_fragment(marker)?;
            while record.remaining() != 0 {
                let (buf, remaining) = record.buf_mut();
                // Read only what fits in the reserved capacity, so memory keeps
                // following the received data.
                let len = remaining.min(buf.capacity() - buf.len());
                let expected = buf.len() + len;
                io.async_read_exact_buf(buf, len).await?;
                if buf.len() != expected {
                    return Err(io::Error::other(
                        "async_read_exact_buf appended the wrong number of bytes",
                    ));
                }
            }
            if let Some(record) = record.take_record() {
                return Ok(record);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::{Bytes, BytesMut};
    use nfs3_types::rpc::{
        accept_stat_data, accepted_reply, fragment_header, msg_body, opaque_auth, reply_body,
        rpc_msg,
    };
    use nfs3_types::xdr_codec::{Encoder, Opaque, Pack, Void};

    use super::RpcClient;
    use crate::RpcError;
    use crate::io::{AsyncRead, AsyncWrite};

    /// In-memory transport that transfers at most 7 bytes per read or write.
    struct Mock {
        input: Bytes,
        output: Vec<u8>,
    }

    impl AsyncRead for Mock {
        async fn async_read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = buf.len().min(self.input.len()).min(7);
            buf[..n].copy_from_slice(&self.input.split_to(n));
            Ok(n)
        }
    }

    impl AsyncWrite for Mock {
        async fn async_write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let n = buf.len().min(7);
            self.output.extend_from_slice(&buf[..n]);
            Ok(n)
        }
    }

    fn reply(xid: u32, result: &Opaque<'_>, fragments: &[usize]) -> Bytes {
        let msg = rpc_msg {
            xid,
            body: msg_body::REPLY(reply_body::MSG_ACCEPTED(accepted_reply {
                verf: opaque_auth::default(),
                reply_data: accept_stat_data::SUCCESS,
            })),
        };
        let mut out = Encoder::new();
        msg.pack(&mut out).unwrap();
        result.pack(&mut out).unwrap();
        let record = out.finish();

        let mut wire = BytesMut::new();
        let mut rest = &*record;
        for (i, &len) in fragments.iter().enumerate() {
            let last = i + 1 == fragments.len();
            let len = if last { rest.len() } else { len };
            let header = fragment_header::new(u32::try_from(len).unwrap(), last);
            wire.extend_from_slice(&header.into_xdr_buf());
            wire.extend_from_slice(&rest[..len]);
            rest = &rest[len..];
        }
        wire.freeze()
    }

    #[tokio::test]
    async fn fragmented_reply_is_reassembled() {
        let payload = Opaque::owned((0..=255u8).cycle().take(5000).collect());
        let mut client = RpcClient::new(Mock {
            input: Bytes::new(),
            output: Vec::new(),
        });
        client.io.input = reply(client.xid, &payload, &[0, 13, 2000, 0]);

        let result: Opaque<'static> = client.call(1, 1, 0, &payload).await.unwrap();
        assert_eq!(result, payload);
        assert!(client.io.input.is_empty());

        // The call, with its payload in a separate chunk, was written completely.
        let output = &client.io.output;
        let marker = u32::from_be_bytes(output[..4].try_into().unwrap());
        assert_eq!(marker as usize, 0x8000_0000 | (output.len() - 4));
        assert!(output.ends_with(&payload[payload.len() - 100..]));
    }

    #[tokio::test]
    async fn truncated_reply_is_an_io_error() {
        let mut client = RpcClient::new(Mock {
            input: Bytes::new(),
            output: Vec::new(),
        });
        let wire = reply(client.xid, &Opaque::borrowed(b"abc"), &[0]);
        client.io.input = wire.slice(..wire.len() - 1);

        let err = client
            .call::<_, Opaque<'static>>(1, 1, 0, &Void)
            .await
            .unwrap_err();
        assert!(
            matches!(err, RpcError::Io(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof)
        );
        assert!(!err.is_connection_reusable());
    }
}
