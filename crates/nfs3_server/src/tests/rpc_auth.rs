use std::sync::Arc;

use bytes::Buf;
use nfs3_types::rpc::{
    RPC_VERSION_2, auth_flavor, auth_stat, call_body, fragment_header, msg_body, opaque_auth,
    rejected_reply, reply_body, rpc_msg,
};
use nfs3_types::xdr_codec::{Decoder, Encoder, Opaque, Pack, Unpack};

use crate::context::RPCContext;
use crate::memfs::{MemFs, MemFsConfig};
use crate::rpcwire::call::RpcCallOutcome;
use crate::rpcwire::handle_rpc_message;

#[tokio::test]
async fn over_limit_credential_gets_badcred_with_call_xid() -> anyhow::Result<()> {
    let xid = 42;
    let fs = MemFs::new(MemFsConfig::default()).expect("create test filesystem");
    let context = RPCContext::test_ctx("/", Arc::new(fs)).await?;
    let mut credential = vec![0; 20 + 17 * 4];
    credential[19] = 17; // Group count after the four zero-valued fields.
    let request = rpc_msg {
        xid,
        body: msg_body::CALL(call_body {
            rpcvers: RPC_VERSION_2,
            prog: nfs3_types::nfs3::PROGRAM,
            vers: 3,
            proc: 0,
            cred: opaque_auth {
                flavor: auth_flavor::AUTH_UNIX,
                body: Opaque::owned(credential),
            },
            verf: opaque_auth::default(),
        }),
    };
    let mut out = Encoder::new(request.packed_size());
    request.pack(&mut out)?;
    let mut record = out.finish_record()?;
    record.advance(4);
    let input = Decoder::copying(record.copy_to_bytes(record.remaining()));
    let RpcCallOutcome::Reply(mut reply) = handle_rpc_message(context, input).await? else {
        panic!("expected a reply for XID {xid}");
    };
    let mut input = Decoder::copying(reply.copy_to_bytes(reply.remaining()));
    fragment_header::unpack(&mut input)?;
    let reply = rpc_msg::unpack(&mut input)?;
    assert_eq!(reply.xid, xid);
    assert!(
        matches!(
            reply.body,
            msg_body::REPLY(reply_body::MSG_DENIED(rejected_reply::AUTH_ERROR(
                auth_stat::AUTH_BADCRED
            )))
        ),
        "unexpected reply: {reply:?}"
    );
    Ok(())
}
