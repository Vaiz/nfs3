use std::sync::Arc;

use bytes::Buf;
use nfs3_types::rpc::{
    RPC_VERSION_2, accept_stat_data, accepted_reply, auth_stat, auth_unix, call_body,
    fragment_header, msg_body, opaque_auth, rejected_reply, reply_body, rpc_msg,
};
use nfs3_types::xdr_codec::{Decoder, Encoder, Error, Opaque, Pack, Unpack};

use super::{RpcCallOutcome, handle_rpc_message};
use crate::context::RPCContext;
use crate::memfs::{MemFs, MemFsConfig};

#[tokio::test]
async fn auth_sys_limits() -> anyhow::Result<()> {
    let fs = MemFs::new(MemFsConfig::default()).expect("create test filesystem");
    let context = RPCContext::test_ctx("/", Arc::new(fs));

    for (xid, (name_len, gids_len, body_len, valid)) in (1u32..).zip([
        (255, 16, None, true),
        (256, 16, None, false),
        (255, 17, None, false),
        (255, 16, Some(400), false),
        (255, 16, Some(401), false),
    ]) {
        let mut cred = opaque_auth::auth_unix(&auth_unix {
            machinename: Opaque::owned(vec![b'm'; name_len]),
            gids: vec![42; gids_len],
            ..auth_unix::default()
        });
        let trailing = body_len.map(|len| {
            let mut body = cred.body.as_ref().to_vec();
            let trailing = len - body.len();
            body.resize(len, 0);
            cred.body = Opaque::owned(body);
            trailing
        });
        let call = call_body {
            rpcvers: RPC_VERSION_2,
            prog: nfs3_types::nfs3::PROGRAM,
            vers: 3,
            proc: 0,
            cred,
            verf: opaque_auth::default(),
        };
        if let Some(trailing) = trailing {
            // A valid AUTH_SYS body is at most 340 bytes. At 400 bytes the
            // body passes the outer limit but the extra bytes fail decoding.
            let err = context.clone().set_auth(&call);
            if body_len == Some(400) {
                assert!(matches!(err, Err(Error::TrailingBytes(n)) if n == trailing));
            } else {
                assert!(matches!(err, Err(Error::InvalidLength(401))));
            }
        }

        let request = rpc_msg {
            xid,
            body: msg_body::CALL(call),
        };
        let mut out = Encoder::new(request.packed_size());
        request.pack(&mut out)?;
        let mut record = out.finish_record()?;
        record.advance(4);
        let input = Decoder::copying(record.copy_to_bytes(record.remaining()));
        let RpcCallOutcome::Reply(mut reply) = handle_rpc_message(context.clone(), input).await?
        else {
            panic!("expected a reply for XID {xid}");
        };
        let mut input = Decoder::copying(reply.copy_to_bytes(reply.remaining()));
        fragment_header::unpack(&mut input)?;
        let reply = rpc_msg::unpack(&mut input)?;
        assert_eq!(reply.xid, xid);
        assert!(
            match reply.body {
                msg_body::REPLY(reply_body::MSG_ACCEPTED(accepted_reply {
                    reply_data: accept_stat_data::SUCCESS,
                    ..
                })) => valid,
                msg_body::REPLY(reply_body::MSG_DENIED(rejected_reply::AUTH_ERROR(
                    auth_stat::AUTH_BADCRED,
                ))) => !valid,
                _ => false,
            },
            "unexpected reply: {reply:?}"
        );
    }
    Ok(())
}
