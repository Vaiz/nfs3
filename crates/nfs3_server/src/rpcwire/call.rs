use anyhow::bail;
use bytes::Bytes;
use nfs3_types::rpc::{
    accept_stat_data, accepted_reply, auth_stat, call_body, msg_body, opaque_auth, rejected_reply,
    reply_body, rpc_msg,
};
use nfs3_types::xdr_codec::{Decoder, Encoded, Encoder, Error, Pack, Unpack, Void};

/// A decoded RPC call header and its still-encoded procedure arguments.
pub(crate) struct RpcCall {
    xid: u32,
    body: call_body<'static>,
    args: Decoder,
}

impl TryFrom<Bytes> for RpcCall {
    type Error = anyhow::Error;

    fn try_from(record: Bytes) -> Result<Self, Self::Error> {
        let mut args = Decoder::new(record);
        let rpc = match rpc_msg::unpack(&mut args) {
            Ok(ok) => ok,
            Err(err) => {
                bail!("Failed to unpack RPC message: {err}");
            }
        };

        let xid = rpc.xid;
        let body = match rpc.body {
            msg_body::CALL(call) => call,
            msg_body::REPLY(_) => {
                bail!("Expected a CALL message, got REPLY. XID: {xid}");
            }
        };

        Ok(Self { xid, body, args })
    }
}

impl RpcCall {
    pub(crate) const fn xid(&self) -> u32 {
        self.xid
    }
    pub(crate) const fn body(&self) -> &call_body<'static> {
        &self.body
    }

    /// Decodes the procedure arguments, which must span the rest of the record.
    ///
    /// Releases this call's reference to the record buffer.
    pub(crate) fn unpack_args<I: Unpack>(&mut self) -> Result<I, Error> {
        let mut args = std::mem::replace(&mut self.args, Decoder::new(Bytes::new()));
        let value = I::unpack(&mut args)?;
        if !args.is_empty() {
            return Err(Error::TrailingBytes(args.remaining()));
        }
        Ok(value)
    }

    pub(crate) fn into_success_reply<T>(self, message: &T) -> anyhow::Result<RpcCallOutcome>
    where
        T: Pack + 'static,
    {
        let rpc = rpc_msg {
            xid: self.xid,
            body: msg_body::REPLY(reply_body::MSG_ACCEPTED(accepted_reply {
                verf: opaque_auth::default(),
                reply_data: accept_stat_data::SUCCESS,
            })),
        };

        pack(&rpc, message).map(RpcCallOutcome::Reply)
    }

    pub(crate) fn into_rpc_mismatch(self) -> anyhow::Result<RpcCallOutcome> {
        use nfs3_types::rpc::RPC_VERSION_2;

        let reply =
            reply_body::MSG_DENIED(rejected_reply::rpc_mismatch(RPC_VERSION_2, RPC_VERSION_2));

        let rpc = rpc_msg {
            xid: self.xid,
            body: msg_body::REPLY(reply),
        };
        pack(&rpc, &Void).map(RpcCallOutcome::Reply)
    }

    pub(crate) fn into_auth_error(self, stat: auth_stat) -> anyhow::Result<RpcCallOutcome> {
        let rpc = rpc_msg {
            xid: self.xid,
            body: msg_body::REPLY(reply_body::MSG_DENIED(rejected_reply::auth_error(stat))),
        };
        pack(&rpc, &Void).map(RpcCallOutcome::Reply)
    }

    pub(crate) fn into_error_reply(self, err: accept_stat_data) -> anyhow::Result<RpcCallOutcome> {
        let rpc = rpc_msg {
            xid: self.xid,
            body: msg_body::REPLY(reply_body::MSG_ACCEPTED(accepted_reply {
                verf: opaque_auth::default(),
                reply_data: err,
            })),
        };
        pack(&rpc, &Void).map(RpcCallOutcome::Reply)
    }
}

pub(crate) enum RpcCallOutcome {
    /// An encoded reply record, including its record marker, as chunks to write in order.
    Reply(Encoded),
    NoReply,
}

fn pack<T>(rpc: &rpc_msg, message: &T) -> anyhow::Result<Encoded>
where
    T: Pack + 'static,
{
    let mut out = Encoder::with_size_hint(rpc.packed_size().saturating_add(message.packed_size()));
    rpc.pack(&mut out)?;
    message.pack(&mut out)?;
    Ok(out.finish_record()?)
}
