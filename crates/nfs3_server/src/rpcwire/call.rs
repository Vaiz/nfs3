use anyhow::bail;
use nfs3_types::rpc::{
    RPC_VERSION_2, accept_stat_data, accepted_reply, auth_stat, call_body, msg_body, msg_type,
    opaque_auth, rejected_reply, reply_body, rpc_msg,
};
use nfs3_types::xdr_codec::{Decoder, Encoded, Encoder, Pack, Unpack, Void};
use tracing::warn;

pub(super) enum DecodedCall {
    Call(RpcCall),
    Rejected(RpcCallOutcome),
}

/// A decoded RPC call header.
pub(crate) struct RpcCall {
    xid: u32,
    body: call_body<'static>,
}

impl RpcCall {
    /// Decodes the call header, leaving `input` at the procedure arguments.
    pub(super) fn unpack(input: &mut Decoder) -> anyhow::Result<DecodedCall> {
        let xid = u32::unpack(input)?;
        if msg_type::unpack(input)? != msg_type::CALL {
            bail!("Expected a CALL message, got REPLY. XID: {xid}");
        }

        let rpcvers = u32::unpack(input)?;
        if rpcvers != RPC_VERSION_2 {
            warn!("Invalid RPC version {rpcvers} != {RPC_VERSION_2}, XID: {xid}");
            return Self::reject(
                xid,
                rejected_reply::rpc_mismatch(RPC_VERSION_2, RPC_VERSION_2),
            )
            .map(DecodedCall::Rejected);
        }
        let program = u32::unpack(input)?;
        let version = u32::unpack(input)?;
        let procedure = u32::unpack(input)?;
        let credential = match opaque_auth::unpack(input) {
            Ok(credential) => credential,
            Err(err) => {
                warn!("Invalid RPC credential for XID {xid}: {err}");
                return Self::reject(xid, rejected_reply::auth_error(auth_stat::AUTH_BADCRED))
                    .map(DecodedCall::Rejected);
            }
        };
        let verifier = match opaque_auth::unpack(input) {
            Ok(verifier) => verifier,
            Err(err) => {
                warn!("Invalid RPC verifier for XID {xid}: {err}");
                return Self::reject(xid, rejected_reply::auth_error(auth_stat::AUTH_BADVERF))
                    .map(DecodedCall::Rejected);
            }
        };
        Ok(DecodedCall::Call(Self {
            xid,
            body: call_body {
                rpcvers,
                prog: program,
                vers: version,
                proc: procedure,
                cred: credential,
                verf: verifier,
            },
        }))
    }

    pub(crate) const fn xid(&self) -> u32 {
        self.xid
    }
    pub(crate) const fn body(&self) -> &call_body<'static> {
        &self.body
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

    pub(crate) fn into_auth_error(self, stat: auth_stat) -> anyhow::Result<RpcCallOutcome> {
        Self::reject(self.xid, rejected_reply::auth_error(stat))
    }

    fn reject(xid: u32, reply: rejected_reply) -> anyhow::Result<RpcCallOutcome> {
        let rpc = rpc_msg {
            xid,
            body: msg_body::REPLY(reply_body::MSG_DENIED(reply)),
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
    let mut out = Encoder::new(rpc.packed_size().saturating_add(message.packed_size()));
    rpc.pack(&mut out)?;
    message.pack(&mut out)?;
    Ok(out.finish_record()?)
}
