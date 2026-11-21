use std::io::Cursor;

use anyhow::bail;
use nfs3_types::rpc::{
    accept_stat_data, accepted_reply, auth_stat, call_body, msg_body, opaque_auth, rejected_reply,
    reply_body, rpc_msg,
};
use nfs3_types::xdr_codec::{Pack, Unpack, Void};

use super::messages::RpcRecord;

/// A decoded RPC call header and its still-encoded procedure arguments.
pub(crate) struct RpcCall {
    xid: u32,
    body: call_body<'static>,
    data: Option<Vec<u8>>,
    message_start: usize, // offset of the start of the message in the data buffer
}

impl TryFrom<RpcRecord> for RpcCall {
    type Error = anyhow::Error;

    fn try_from(packed: RpcRecord) -> Result<Self, Self::Error> {
        let packed = packed.into_inner();
        let (rpc, pos) = match rpc_msg::unpack(&mut Cursor::new(&packed)) {
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

        Ok(Self {
            xid,
            body,
            data: Some(packed),
            message_start: pos,
        })
    }
}

impl RpcCall {
    pub(crate) const fn xid(&self) -> u32 {
        self.xid
    }
    pub(crate) const fn body(&self) -> &call_body<'static> {
        &self.body
    }

    pub(crate) const fn take_data(&mut self) -> Cursor<Vec<u8>> {
        let data = self.data.take().expect("Data already taken");
        let mut cursor = Cursor::new(data);
        cursor.set_position(self.message_start as u64);
        cursor
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
    Reply(RpcRecord),
    NoReply,
}

fn pack<T>(rpc: &rpc_msg, message: &T) -> anyhow::Result<RpcRecord>
where
    T: Pack + 'static,
{
    let size = rpc
        .packed_size()
        .checked_add(message.packed_size())
        .expect("Failed to calculate size");

    let mut cursor = Cursor::new(Vec::with_capacity(size));
    rpc.pack(&mut cursor)?;
    message.pack(&mut cursor)?;
    Ok(RpcRecord::new(cursor.into_inner()))
}
