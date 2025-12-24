use anyhow::{Context, bail};
use nfs3_client::nfs3_types;
use nfs3_tests::RpcTestContext;
use nfs3_types::nfs3::{LOOKUP3res, NFS_PROGRAM};
use nfs3_types::rpc::{call_body, msg_body, opaque_auth, reply_body, rpc_msg};
use nfs3_types::xdr_codec::Void;

fn nfs3_call(xid: u32, proc: NFS_PROGRAM) -> rpc_msg<'static, 'static> {
    rpc_msg {
        xid,
        body: msg_body::CALL(call_body {
            rpcvers: nfs3_types::rpc::RPC_VERSION_2,
            prog: nfs3_types::nfs3::PROGRAM,
            vers: nfs3_types::nfs3::VERSION,
            proc: proc as u32,
            cred: opaque_auth::default(),
            verf: opaque_auth::default(),
        }),
    }
}

fn check_reply_header(xid: u32, msg: &rpc_msg<'_, '_>) -> anyhow::Result<()> {
    if msg.xid != xid {
        bail!("XID mismatch: expected {xid}, got {}", msg.xid);
    }
    match &msg.body {
        msg_body::REPLY(reply) => match reply {
            reply_body::MSG_ACCEPTED(accepted) => {
                if matches!(
                    accepted.reply_data,
                    nfs3_types::rpc::accept_stat_data::SUCCESS
                ) {
                    Ok(())
                } else {
                    bail!("RPC call failed with status: {:?}", accepted.reply_data);
                }
            }
            reply_body::MSG_DENIED(denied) => {
                bail!("RPC call denied with status: {denied:?}");
            }
        },
        _ => bail!("Expected REPLY message, got: {:?}", msg.body),
    }
}

fn check_garbage_args_reply(xid: u32, msg: &rpc_msg<'_, '_>) -> anyhow::Result<()> {
    if msg.xid != xid {
        bail!("XID mismatch: expected {xid}, got {}", msg.xid);
    }
    match &msg.body {
        msg_body::REPLY(reply) => match reply {
            reply_body::MSG_ACCEPTED(accepted) => {
                if matches!(
                    accepted.reply_data,
                    nfs3_types::rpc::accept_stat_data::GARBAGE_ARGS
                ) {
                    Ok(())
                } else {
                    bail!("RPC call failed with status: {:?}", accepted.reply_data);
                }
            }
            reply_body::MSG_DENIED(denied) => {
                bail!("RPC call denied with status: {denied:?}");
            }
        },
        _ => bail!("Expected REPLY message, got: {:?}", msg.body),
    }
}

#[tokio::test]
async fn sequential_requests() -> anyhow::Result<()> {
    let mut client = RpcTestContext::setup();

    let rpc_msg = nfs3_call(1, NFS_PROGRAM::NFSPROC3_NULL);
    client.send_call(&rpc_msg, &Void).await?;
    let (msg, body) = client.recv_reply::<Void>().await?;
    check_reply_header(1, &msg)?;
    assert!(body.is_some());

    let rpc_msg = nfs3_call(2, NFS_PROGRAM::NFSPROC3_NULL);
    client.send_call(&rpc_msg, &Void).await?;
    let (msg, body) = client.recv_reply::<Void>().await?;
    check_reply_header(2, &msg)?;
    assert!(body.is_some());

    client.shutdown().await
}

#[tokio::test]
async fn concurrent_requests() -> anyhow::Result<()> {
    let mut client = RpcTestContext::setup();

    let rpc_msg1 = nfs3_call(1, NFS_PROGRAM::NFSPROC3_NULL);
    let rpc_msg2 = nfs3_call(2, NFS_PROGRAM::NFSPROC3_NULL);

    client.send_call(&rpc_msg1, &Void).await?;
    client.send_call(&rpc_msg2, &Void).await?;

    let (msg, body) = client.recv_reply::<Void>().await?;
    assert!([1u32, 2].contains(&msg.xid));
    check_reply_header(msg.xid, &msg)?;
    assert!(body.is_some());
    let prev_xid = msg.xid;

    let (msg, body) = client.recv_reply::<Void>().await?;
    assert!([1u32, 2].contains(&msg.xid));
    assert_ne!(msg.xid, prev_xid);
    check_reply_header(msg.xid, &msg)?;
    assert!(body.is_some());

    client.shutdown().await
}

#[tokio::test]
async fn repeated_requests() -> anyhow::Result<()> {
    let mut client = RpcTestContext::setup();

    let rpc_msg = nfs3_call(1, NFS_PROGRAM::NFSPROC3_NULL);
    client.send_call(&rpc_msg, &Void).await?;
    let (msg, body) = client.recv_reply::<Void>().await?;
    check_reply_header(1, &msg)?;
    assert!(body.is_some());

    // Send the same request again
    client.send_call(&rpc_msg, &Void).await?;
    // the server will ignore the repeated request, so we need to send another one
    // btw, this is not the correct implementation of the NFS protocol

    let rpc_msg2 = nfs3_call(2, NFS_PROGRAM::NFSPROC3_NULL);
    client.send_call(&rpc_msg2, &Void).await?;
    let (msg, body) = client.recv_reply::<Void>().await?;
    check_reply_header(2, &msg)?;
    assert!(body.is_some());

    client.shutdown().await.context("failed to shutdown server")
}

#[tokio::test]
async fn out_of_order_requests() -> anyhow::Result<()> {
    let mut client = RpcTestContext::setup();

    let rpc_msg1 = nfs3_call(200, NFS_PROGRAM::NFSPROC3_NULL);
    let rpc_msg2 = nfs3_call(100, NFS_PROGRAM::NFSPROC3_NULL);

    client.send_call(&rpc_msg1, &Void).await?;
    let (msg, body) = client.recv_reply::<Void>().await?;
    check_reply_header(200, &msg)?;
    assert!(body.is_some());

    client.send_call(&rpc_msg2, &Void).await?;
    let (msg, body) = client.recv_reply::<Void>().await?;
    check_reply_header(100, &msg)?;
    assert!(body.is_some());

    client.shutdown().await
}

#[tokio::test]
async fn null_with_body() -> anyhow::Result<()> {
    let mut client = RpcTestContext::setup();

    let rpc_msg = nfs3_call(1, NFS_PROGRAM::NFSPROC3_NULL);
    let null_body = 0x1234_5678u32;
    client.send_call(&rpc_msg, &null_body).await?;

    let (msg, body) = client.recv_reply::<Void>().await?;
    check_garbage_args_reply(1, &msg)?;
    assert_eq!(body, None);

    client.shutdown().await
}

#[tokio::test]
async fn invalid_body() -> anyhow::Result<()> {
    let mut client = RpcTestContext::setup();

    let rpc_msg = nfs3_call(1, NFS_PROGRAM::NFSPROC3_LOOKUP);
    client.send_call(&rpc_msg, &Void).await?;

    let (msg, body) = client.recv_reply::<LOOKUP3res>().await?;
    check_garbage_args_reply(1, &msg)?;
    assert!(body.is_none());

    client.shutdown().await
}

#[tokio::test]
async fn invalid_rpc_ver() -> anyhow::Result<()> {
    let mut client = RpcTestContext::setup();

    let rpc_msg = rpc_msg {
        xid: 1,
        body: msg_body::CALL(call_body {
            rpcvers: 0x1234_5678u32,
            prog: nfs3_types::nfs3::PROGRAM,
            vers: nfs3_types::nfs3::VERSION,
            proc: NFS_PROGRAM::NFSPROC3_NULL as u32,
            cred: opaque_auth::default(),
            verf: opaque_auth::default(),
        }),
    };

    client.send_call(&rpc_msg, &Void).await?;

    let (msg, body) = client.recv_reply::<Void>().await?;
    tracing::debug!("{msg:?}");
    assert_eq!(msg.xid, 1);
    let msg_body::REPLY(reply) = &msg.body else {
        panic!("Expected REPLY message, got: {:?}", msg.body);
    };
    let reply_body::MSG_DENIED(denied) = reply else {
        panic!("Expected MSG_DENIED, got: {reply:?}");
    };
    assert!(matches!(
        denied,
        nfs3_types::rpc::rejected_reply::RPC_MISMATCH {
            low: nfs3_types::rpc::RPC_VERSION_2,
            high: nfs3_types::rpc::RPC_VERSION_2
        }
    ));
    assert!(body.is_none());

    client.shutdown().await
}

#[tokio::test]
async fn unknown_program() -> anyhow::Result<()> {
    let mut client = RpcTestContext::setup();

    let rpc_msg = rpc_msg {
        xid: 1,
        body: msg_body::CALL(call_body {
            rpcvers: nfs3_types::rpc::RPC_VERSION_2,
            prog: 0x1234_5678u32,
            vers: nfs3_types::nfs3::VERSION,
            proc: NFS_PROGRAM::NFSPROC3_NULL as u32,
            cred: opaque_auth::default(),
            verf: opaque_auth::default(),
        }),
    };

    client.send_call(&rpc_msg, &Void).await?;

    let (msg, body) = client.recv_reply::<Void>().await?;
    tracing::debug!("{msg:?}");
    assert_eq!(msg.xid, 1);
    let msg_body::REPLY(reply) = &msg.body else {
        panic!("Expected REPLY message, got: {:?}", msg.body);
    };
    let reply_body::MSG_ACCEPTED(accepted) = reply else {
        panic!("Expected MSG_ACCEPTED, got: {reply:?}");
    };
    assert!(matches!(
        accepted.reply_data,
        nfs3_types::rpc::accept_stat_data::PROG_UNAVAIL
    ));
    assert!(body.is_none());

    client.shutdown().await
}

#[tokio::test]
async fn invalid_nfs_version() -> anyhow::Result<()> {
    let mut client = RpcTestContext::setup();

    let rpc_msg = rpc_msg {
        xid: 1,
        body: msg_body::CALL(call_body {
            rpcvers: nfs3_types::rpc::RPC_VERSION_2,
            prog: nfs3_types::nfs3::PROGRAM,
            vers: 0x1234_5678u32,
            proc: NFS_PROGRAM::NFSPROC3_NULL as u32,
            cred: opaque_auth::default(),
            verf: opaque_auth::default(),
        }),
    };

    client.send_call(&rpc_msg, &Void).await?;
    let (msg, body) = client.recv_reply::<Void>().await?;
    tracing::debug!("{msg:?}");
    assert_eq!(msg.xid, 1);
    let msg_body::REPLY(reply) = &msg.body else {
        panic!("Expected REPLY message, got: {:?}", msg.body);
    };
    let reply_body::MSG_ACCEPTED(accepted) = reply else {
        panic!("Expected MSG_ACCEPTED, got: {reply:?}");
    };
    assert!(matches!(
        accepted.reply_data,
        nfs3_types::rpc::accept_stat_data::PROG_MISMATCH {
            low: nfs3_types::nfs3::VERSION,
            high: nfs3_types::nfs3::VERSION
        }
    ));
    assert!(body.is_none());

    client.shutdown().await
}

#[tokio::test]
async fn invalid_procedure() -> anyhow::Result<()> {
    let mut client = RpcTestContext::setup();

    let rpc_msg = rpc_msg {
        xid: 1,
        body: msg_body::CALL(call_body {
            rpcvers: nfs3_types::rpc::RPC_VERSION_2,
            prog: nfs3_types::nfs3::PROGRAM,
            vers: nfs3_types::nfs3::VERSION,
            proc: 0x1234_5678u32,
            cred: opaque_auth::default(),
            verf: opaque_auth::default(),
        }),
    };

    client.send_call(&rpc_msg, &Void).await?;

    let (msg, body) = client.recv_reply::<Void>().await?;
    tracing::debug!("{msg:?}");
    assert_eq!(msg.xid, 1);
    let msg_body::REPLY(reply) = &msg.body else {
        panic!("Expected REPLY message, got: {:?}", msg.body);
    };
    let reply_body::MSG_ACCEPTED(accepted) = reply else {
        panic!("Expected MSG_ACCEPTED, got: {reply:?}");
    };
    assert!(matches!(
        accepted.reply_data,
        nfs3_types::rpc::accept_stat_data::PROC_UNAVAIL
    ));
    assert!(body.is_none());

    client.shutdown().await
}
