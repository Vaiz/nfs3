use std::io;
use std::sync::Arc;
use std::time::Duration;

use nfs3_types::portmap;
use nfs3_types::rpc::{RPC_VERSION_2, call_body, fragment_header, msg_body, opaque_auth, rpc_msg};
use nfs3_types::xdr_codec::Pack;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::process_socket;
use crate::context::RPCContext;
use crate::memfs::{MemFs, MemFsConfig};

#[tokio::test]
async fn truncated_record_closes_connection_during_pending_reply() {
    let fs = MemFs::new(MemFsConfig::default()).expect("in-memory filesystem");
    let context = RPCContext::test_ctx("/", Arc::new(fs));
    let (mut client, server) = tokio::io::duplex(8);
    let mut call = Vec::new();
    rpc_msg {
        xid: 1,
        body: msg_body::CALL(call_body {
            rpcvers: RPC_VERSION_2,
            prog: portmap::PROGRAM,
            vers: portmap::VERSION,
            proc: portmap::PMAP_PROG::PMAPPROC_NULL as u32,
            cred: opaque_auth::default(),
            verf: opaque_auth::default(),
        }),
    }
    .pack(&mut call)
    .expect("encode NULL call");
    let marker =
        fragment_header::new(u32::try_from(call.len()).expect("small call"), true).into_xdr_buf();

    let client_io = async {
        client.write_all(&marker).await.expect("send marker");
        client.write_all(&call).await.expect("send NULL call");
        client
            .write_all(&marker[..2])
            .await
            .expect("start next marker");
        // Read only one byte, so the reply still cannot fit in the duplex buffer.
        client.read_u8().await.expect("reply started");
        client.shutdown().await.expect("finish writing");
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(process_socket(server, context, None), client_io)
    })
    .await
    .expect("parser must not wait for the reply to finish");
    let err = result.expect_err("partial marker is a parser failure");
    assert_eq!(
        err.downcast_ref::<io::Error>().map(io::Error::kind),
        Some(io::ErrorKind::UnexpectedEof)
    );
}
