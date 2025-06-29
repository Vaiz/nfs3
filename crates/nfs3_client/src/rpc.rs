//! RPC client implementation

use std::fmt::Debug;

use nfs3_types::rpc::{
    RPC_VERSION_2, accept_stat_data, call_body, fragment_header, msg_body, opaque_auth, reply_body,
    rpc_msg,
};
use nfs3_types::xdr_codec::{Pack, Unpack};

use crate::error::{Error, RpcError};
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
            xid: rand::random(),
            credential,
            verifier,
        }
    }

    /// Call an RPC procedure
    ///
    /// This method uses `Pack` trait to serialize the arguments and `Unpack` trait to deserialize
    /// the reply.
    #[allow(clippy::similar_names)] // prog and proc are part of call_body struct
    pub async fn call<C, R>(&mut self, prog: u32, vers: u32, proc: u32, args: &C) -> Result<R, Error>
    where
        R: Unpack,
        C: Pack,
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

    async fn send_call<T>(io: &mut IO, msg: &rpc_msg<'_, '_>, args: &T) -> Result<(), Error>
    where
        T: Pack,
    {
        let total_len = msg.packed_size() + args.packed_size();
        if total_len % 4 != 0 {
            return Err(RpcError::WrongLength.into());
        }

        let fragment_header = nfs3_types::rpc::fragment_header::new(
            u32::try_from(total_len).expect("message is too large"),
            true,
        );
        let mut buf = Vec::with_capacity(total_len + 4);
        fragment_header.pack(&mut buf)?;
        msg.pack(&mut buf)?;
        args.pack(&mut buf)?;
        if buf.len() - 4 != total_len {
            return Err(RpcError::WrongLength.into());
        }
        io.async_write_all(&buf).await?;
        Ok(())
    }

    async fn recv_reply<T>(io: &mut IO, xid: u32) -> Result<T, Error>
    where
        T: Unpack,
    {
        let mut buf = [0u8; 4];
        io.async_read_exact(&mut buf).await?;
        let fragment_header: fragment_header = buf.into();
        assert!(
            fragment_header.eof(),
            "Fragment header does not have EOF flag"
        );

        let total_len = fragment_header.fragment_length();
        let mut buf = vec![0u8; total_len as usize];
        io.async_read_exact(&mut buf).await?;

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

        if !matches!(reply.reply_data, accept_stat_data::SUCCESS) {
            return Err(crate::error::RpcError::try_from(reply.reply_data)
                .expect("accept_stat_data::SUCCESS is not a valid error")
                .into());
        }

        let (final_value, _) = T::unpack(&mut cursor)?;
        if cursor.position() != u64::from(total_len) {
            let pos = cursor.position();
            return Err(RpcError::NotFullyParsed {
                buf: cursor.into_inner(),
                pos,
            }
            .into());
        }
        Ok(final_value)
    }
}
