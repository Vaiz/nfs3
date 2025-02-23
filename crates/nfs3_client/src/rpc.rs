//! RPC client implementation

use std::fmt::Debug;
use std::io::Cursor;

use nfs3_types::rpc::{
    RPC_VERSION_2, accept_stat_data, call_body, msg_body, opaque_auth, reply_body, rpc_msg,
};
use nfs3_types::xdr_codec::{Pack, PackedSize, Unpack};

use crate::error::{Error, RpcError};
use crate::io::{AsyncRead, AsyncWrite};

const EOF_FLAG: u32 = 0x8000_0000;

/// RPC client
pub struct RpcClient<IO> {
    io: IO,
    xid: u32,
}

impl<IO> Debug for RpcClient<IO> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        f.debug_struct("RpcClient").finish()
    }
}

impl<IO> RpcClient<IO>
where
    IO: AsyncRead + AsyncWrite,
{
    /// Create a new RPC client. XID is initialized to a random value.
    pub fn new(io: IO) -> Self {
        Self {
            io,
            xid: rand::random(),
        }
    }

    /// Call an RPC procedure
    ///
    /// This method uses `Pack` trait to serialize the arguments and `Unpack` trait to deserialize
    /// the reply.
    pub async fn call<C, R>(&mut self, prog: u32, vers: u32, proc: u32, args: C) -> Result<R, Error>
    where
        R: Unpack<Cursor<Vec<u8>>>,
        C: Pack<Vec<u8>> + PackedSize,
    {
        let call = call_body {
            rpcvers: RPC_VERSION_2,
            prog,
            vers,
            proc,
            cred: opaque_auth::default(),
            verf: opaque_auth::default(),
        };
        let msg = rpc_msg {
            xid: self.xid,
            body: msg_body::CALL(call),
        };
        self.xid = self.xid.wrapping_add(1);

        self.send_call(&msg, args).await?;
        self.recv_reply::<R>(msg.xid).await
    }

    async fn send_call<T>(&mut self, msg: &rpc_msg<'_, '_>, args: T) -> Result<(), Error>
    where
        T: Pack<Vec<u8>> + PackedSize,
    {
        let total_len = msg.packed_size() + args.packed_size();
        if total_len % 4 != 0 {
            return Err(RpcError::WrongLength.into());
        }

        let mut buf = Vec::with_capacity(total_len + 4);
        let fragment_header = total_len as u32 | EOF_FLAG;
        fragment_header.pack(&mut buf)?;
        msg.pack(&mut buf)?;
        args.pack(&mut buf)?;
        if buf.len() - 4 != total_len {
            return Err(RpcError::WrongLength.into());
        }
        self.io.async_write_all(&buf).await?;
        Ok(())
    }

    async fn recv_reply<T>(&mut self, xid: u32) -> Result<T, Error>
    where
        T: Unpack<Cursor<Vec<u8>>>,
    {
        let mut buf = [0u8; 4];
        self.io.async_read_exact(&mut buf).await?;
        let fragment_header = u32::from_be_bytes(buf);
        if fragment_header & EOF_FLAG == 0 {
            panic!("Fragment header does not have EOF flag");
        }

        let total_len = fragment_header & !EOF_FLAG;
        let mut buf = vec![0u8; total_len as usize];
        self.io.async_read_exact(&mut buf).await?;

        let mut cursor = std::io::Cursor::new(buf);
        let (resp_msg, _) = rpc_msg::unpack(&mut cursor)?;

        if resp_msg.xid != xid {
            return Err(RpcError::UnexpectedXid.into());
        }

        let reply = match resp_msg.body {
            msg_body::REPLY(reply_body::MSG_ACCEPTED(reply)) => reply,
            msg_body::REPLY(reply_body::MSG_DENIED(r)) => return Err(r.into()),
            msg_body::CALL(_) => return Err(RpcError::UnexpectedCall.into()),
        };

        if let accept_stat_data::SUCCESS = reply.reply_data {
        } else {
            return Err(crate::error::RpcError::try_from(reply.reply_data)
                .unwrap()
                .into());
        }

        let (final_value, _) = T::unpack(&mut cursor)?;
        if cursor.position() as usize != total_len as usize {
            let pos = cursor.position() as usize;
            return Err(RpcError::NotFullyParsed {
                buf: cursor.into_inner(),
                pos,
            }
            .into());
        }
        Ok(final_value)
    }
}
