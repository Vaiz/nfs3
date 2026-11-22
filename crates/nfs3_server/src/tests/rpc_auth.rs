use std::sync::Arc;

use bytes::Buf;
use nfs3_types::rpc::{
    RPC_VERSION_2, auth_flavor, auth_stat, auth_unix, call_body, fragment_header, msg_body,
    opaque_auth, rejected_reply, reply_body, rpc_msg,
};
use nfs3_types::xdr_codec::{Decoder, Encoder, Error, Opaque, Pack, Unpack};

use crate::context::RPCContext;
use crate::memfs::{MemFs, MemFsConfig};
use crate::rpcwire::call::RpcCallOutcome;
use crate::rpcwire::handle_rpc_message;

fn context() -> RPCContext<MemFs> {
    let fs = MemFs::new(MemFsConfig::default()).expect("create test filesystem");
    RPCContext::test_ctx("/", Arc::new(fs))
}

fn credential(name_len: usize, gids_len: usize) -> opaque_auth<'static> {
    opaque_auth::auth_unix(&auth_unix {
        machinename: Opaque::owned(vec![b'm'; name_len]),
        gids: vec![42; gids_len],
        ..auth_unix::default()
    })
}

fn call(cred: opaque_auth<'static>) -> call_body<'static> {
    call_body {
        rpcvers: RPC_VERSION_2,
        prog: nfs3_types::nfs3::PROGRAM,
        vers: 3,
        proc: 0,
        cred,
        verf: opaque_auth::default(),
    }
}

#[test]
fn auth_sys_limits() {
    let mut context = context();
    let cases = [
        (credential(255, 16), Ok(true)),
        (credential(256, 16), Err(Error::InvalidLength(256))),
        (credential(255, 17), Err(Error::InvalidLength(17))),
        // 400 bytes passes the size check. The decoder reads the 20-byte
        // empty credential and rejects the trailing bytes.
        (
            opaque_auth {
                flavor: auth_flavor::AUTH_UNIX,
                body: Opaque::owned(vec![0; 400]),
            },
            Err(Error::TrailingBytes(380)),
        ),
        (
            opaque_auth {
                flavor: auth_flavor::AUTH_UNIX,
                body: Opaque::owned(vec![0; 401]),
            },
            Err(Error::InvalidLength(401)),
        ),
    ];
    for (cred, expected) in cases {
        assert_eq!(context.set_auth(&call(cred)), expected);
    }
}

#[tokio::test]
async fn over_limit_credential_gets_badcred_with_call_xid() -> anyhow::Result<()> {
    let xid = 42;
    let request = rpc_msg {
        xid,
        body: msg_body::CALL(call(credential(255, 17))),
    };
    let mut out = Encoder::new(request.packed_size());
    request.pack(&mut out)?;
    let mut record = out.finish_record()?;
    record.advance(4);
    let input = Decoder::copying(record.copy_to_bytes(record.remaining()));
    let RpcCallOutcome::Reply(mut reply) = handle_rpc_message(context(), input).await? else {
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
