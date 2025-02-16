use std::io::Cursor;

use nfs3_types::rpc::{
    accept_stat_data, call_body, msg_body, opaque_auth, reply_body, rpc_msg, RPC_VERSION_2,
};
use nfs3_types::xdr_codec::{Pack, PackedSize, Unpack};

use crate::error::{Error, RpcError};
use crate::io::{AsyncRead, AsyncWrite};

const EOF_FLAG: u32 = 0x8000_0000;

pub struct RpcClient<IO> {
    io: IO,
    xid: u32,
}

impl<IO> RpcClient<IO>
where
    IO: AsyncRead + AsyncWrite,
{
    pub fn new(io: IO) -> Self {
        Self {
            io,
            xid: rand::random(),
        }
    }

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
        if buf.len() - 4 != total_len as usize {
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

        Ok(T::unpack(&mut cursor)?.0)
    }
}
