use std::sync::Arc;

use bytes::Buf;
use nfs3_types::rpc::{
    RPC_VERSION_2, auth_flavor, auth_stat, call_body, fragment_header, msg_body, msg_type,
    opaque_auth, rejected_reply, reply_body, rpc_msg,
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
                body: Opaque::owned(credential).try_into()?,
            },
            verf: opaque_auth::default(),
        }),
    };
    let mut out = Encoder::new(request.packed_size());
    request.pack(&mut out)?;
    assert_auth_error(context, out, xid, auth_stat::AUTH_BADCRED).await
}

#[tokio::test]
async fn oversized_auth_bodies_get_rpc_auth_errors() -> anyhow::Result<()> {
    let fs = MemFs::new(MemFsConfig::default()).expect("create test filesystem");
    let context = RPCContext::test_ctx("/", Arc::new(fs)).await?;
    let xid = 42;
    for error in [auth_stat::AUTH_BADCRED, auth_stat::AUTH_BADVERF] {
        let mut out = Encoder::new(0);
        xid.pack(&mut out)?;
        msg_type::CALL.pack(&mut out)?;
        RPC_VERSION_2.pack(&mut out)?;
        nfs3_types::nfs3::PROGRAM.pack(&mut out)?;
        3u32.pack(&mut out)?;
        0u32.pack(&mut out)?;
        if error == auth_stat::AUTH_BADVERF {
            opaque_auth::default().pack(&mut out)?;
        }
        auth_flavor::AUTH_UNIX.pack(&mut out)?;
        Opaque::borrowed(&[0; 401]).pack(&mut out)?;
        if error == auth_stat::AUTH_BADCRED {
            opaque_auth::default().pack(&mut out)?;
        }
        assert_auth_error(context.clone(), out, xid, error).await?;
    }
    Ok(())
}

async fn assert_auth_error(
    context: RPCContext<MemFs>,
    out: Encoder,
    xid: u32,
    expected: auth_stat,
) -> anyhow::Result<()> {
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
            msg_body::REPLY(reply_body::MSG_DENIED(rejected_reply::AUTH_ERROR(stat)))
                if stat == expected
        ),
        "unexpected reply: {reply:?}"
    );
    Ok(())
}
