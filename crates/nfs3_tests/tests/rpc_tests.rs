use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::num::NonZeroUsize;
use std::time::Duration;

use anyhow::{Context, bail};
use bytes::{Buf, Bytes};
use nfs3_client::nfs3_types;
use nfs3_server::memfs::{MemFs, MemFsConfig};
use nfs3_server::tcp::{Builder, NFSTcp};
use nfs3_tests::{RpcTestContext, Server};
use nfs3_types::mount::MNTPATHLEN;
use nfs3_types::nfs3::{LOOKUP3res, NFS_PROGRAM};
use nfs3_types::rpc::{call_body, fragment_header, msg_body, opaque_auth, reply_body, rpc_msg};
use nfs3_types::xdr_codec::{Decoder, Encoder, Pack, Unpack, Void};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

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

/// Encodes a NULL call as a single-fragment RPC record.
fn null_call_record(xid: u32) -> anyhow::Result<Bytes> {
    let call = nfs3_call(xid, NFS_PROGRAM::NFSPROC3_NULL);
    let mut out = Encoder::new(call.packed_size());
    call.pack(&mut out)?;
    let mut record = out.finish_record()?;
    Ok(record.copy_to_bytes(record.remaining()))
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
        msg_body::CALL(_) => bail!("Expected REPLY message, got: {:?}", msg.body),
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
        msg_body::CALL(_) => bail!("Expected REPLY message, got: {:?}", msg.body),
    }
}

#[tokio::test]
async fn tcp_listener_checks_normalized_export_length_before_binding() -> anyhow::Result<()> {
    let fs = MemFs::new(MemFsConfig::default()).expect("initialize the test filesystem");
    let listener = Builder::new()
        .export_name(format!("//{}//", "a".repeat(MNTPATHLEN - 1)))
        .filesystem(fs)
        .bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await?;
    let addr = SocketAddr::new(listener.get_listen_ip(), listener.get_listen_port());

    let fs = MemFs::new(MemFsConfig::default()).expect("initialize the test filesystem");
    let result = Builder::new()
        .export_name("a".repeat(MNTPATHLEN))
        .filesystem(fs)
        .bind(addr)
        .await;
    let Err(err) = result else {
        bail!("an oversized normalized export path was accepted");
    };
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("export path is 1025 bytes"));
    Ok(())
}

#[tokio::test]
#[expect(
    deprecated,
    reason = "verify the legacy export setter enforces the limit"
)]
#[should_panic(expected = "export path is 1025 bytes after normalization; maximum is 1024")]
async fn legacy_export_setter_checks_normalized_length() {
    let fs = MemFs::new(MemFsConfig::default()).expect("initialize the test filesystem");
    let mut listener = Builder::new()
        .filesystem(fs)
        .bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await
        .expect("bind the test listener");
    listener.with_export_name("a".repeat(MNTPATHLEN - 1));
    listener.with_export_name("a".repeat(MNTPATHLEN));
}

#[tokio::test]
async fn tcp_listener_enforces_rpc_record_payload_limit() -> anyhow::Result<()> {
    let payload = null_call_record(1)?.slice(4..);
    let limit = NonZeroUsize::new(payload.len()).context("NULL call has an RPC header")?;
    let fs = MemFs::new(MemFsConfig::default()).expect("initialize the test filesystem");
    let listener = Builder::new()
        .max_rpc_record_size(limit)
        .filesystem(fs)
        .bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await?;
    let addr = SocketAddr::new(listener.get_listen_ip(), listener.get_listen_port());

    let client = async {
        let mut stream = TcpStream::connect(addr).await?;
        let (head, tail) = payload.split_at(payload.len() / 2);
        for (fragment, last) in [(head, false), (tail, true)] {
            let header = fragment_header::new(fragment.len(), last)?;
            stream.write_all(&header.into_xdr_buf()).await?;
            stream.write_all(fragment).await?;
        }
        let mut marker = [0; 4];
        stream.read_exact(&mut marker).await?;
        let reply_header = fragment_header::from(marker);
        assert!(reply_header.eof());
        let mut reply = vec![0; reply_header.fragment_length() as usize];
        stream.read_exact(&mut reply).await?;
        let reply = rpc_msg::unpack(&mut Decoder::copying(reply))?;
        check_reply_header(1, &reply)?;

        // The reader should close the connection as soon as the marker declares
        // a record over the limit, without waiting for the payload.
        let oversized = fragment_header::new(limit.get() + 1, true)?;
        stream.write_all(&oversized.into_xdr_buf()).await?;
        assert_eq!(stream.read(&mut marker).await?, 0);
        Ok::<_, anyhow::Error>(())
    };

    timeout(Duration::from_secs(5), async {
        tokio::select! {
            result = client => result,
            result = listener.handle_forever() => {
                result?;
                bail!("TCP listener stopped early");
            }
        }
    })
    .await
    .context("TCP listener did not enforce the record limit")?
}

#[tokio::test]
async fn tcp_fragment_limit_closes_only_the_offending_connection() -> anyhow::Result<()> {
    async fn null_call(stream: &mut TcpStream, xid: u32) -> anyhow::Result<()> {
        stream.write_all(&null_call_record(xid)?).await?;
        let mut marker = [0; 4];
        stream.read_exact(&mut marker).await?;
        let header = fragment_header::from(marker);
        assert!(header.eof());
        let mut payload = vec![0; header.fragment_length() as usize];
        stream.read_exact(&mut payload).await?;
        let reply = rpc_msg::unpack(&mut Decoder::copying(payload))?;
        check_reply_header(xid, &reply)
    }

    let fs = MemFs::new(MemFsConfig::default()).expect("initialize the test filesystem");
    let listener = Builder::new()
        .filesystem(fs)
        .bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await?;
    let addr = SocketAddr::new(listener.get_listen_ip(), listener.get_listen_port());

    let clients = async {
        let mut healthy = TcpStream::connect(addr).await?;
        null_call(&mut healthy, 1).await?;
        let mut offending = TcpStream::connect(addr).await?;
        for xid in [2, 3] {
            offending.write_all(&[0; 255 * 4]).await?;
            null_call(&mut offending, xid).await?;
        }

        offending.write_all(&[0; 256 * 4]).await?;
        // The 257th marker alone must close the connection without waiting
        // for its advertised payload or a final fragment.
        let marker = fragment_header::new(1, false)?;
        offending.write_all(&marker.into_xdr_buf()).await?;
        assert_eq!(offending.read(&mut [0; 4]).await?, 0);

        null_call(&mut healthy, 4).await?;
        let mut fresh = TcpStream::connect(addr).await?;
        null_call(&mut fresh, 5).await?;
        Ok::<_, anyhow::Error>(())
    };

    timeout(Duration::from_secs(5), async {
        tokio::select! {
            result = clients => result,
            result = listener.handle_forever() => {
                result?;
                bail!("TCP listener stopped early");
            }
        }
    })
    .await
    .context("TCP listener did not enforce the fragment limit")?
}

#[tokio::test]
async fn socket_reads_and_writes_progress_independently() -> anyhow::Result<()> {
    // The one-byte capacity forces the server to consume part of any marker
    // prefix written below before the client's write can complete.
    let (server, mut client) = tokio::io::duplex(1);
    let fs = MemFs::new(MemFsConfig::default()).expect("initialize the test filesystem");
    let server = tokio::spawn(Server::new(server, fs).await?.run());
    timeout(Duration::from_secs(5), async {
        client.write_all(&null_call_record(1)?).await?;
        let second = null_call_record(2)?;
        client.write_all(&second[..3]).await?;

        for xid in [1, 2] {
            let mut marker = [0; 4];
            client.read_exact(&mut marker).await?;
            let marker = fragment_header::from(marker);
            assert!(marker.eof());
            let mut payload = vec![0; marker.fragment_length() as usize];
            client.read_exact(&mut payload).await?;
            let reply = rpc_msg::unpack(&mut Decoder::copying(payload))?;
            check_reply_header(xid, &reply)?;
            if xid == 1 {
                client.write_all(&second[3..]).await?;
            }
        }

        client.write_all(&null_call_record(3)?).await?;
        // The writer has started the reply but cannot finish it while we stop
        // reading after one byte. Half-close with an incomplete next marker.
        client.read_u8().await?;
        client.write_all(&second[..3]).await?;
        client.shutdown().await?;
        let err = server.await?.expect_err("a partial marker must fail");
        assert_eq!(
            err.downcast_ref::<io::Error>().map(io::Error::kind),
            Some(io::ErrorKind::UnexpectedEof)
        );
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("socket made no progress")?
}

#[tokio::test]
async fn sequential_requests() -> anyhow::Result<()> {
    let mut client = RpcTestContext::setup().await;

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
    let mut client = RpcTestContext::setup().await;

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
    let mut client = RpcTestContext::setup().await;

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
    let mut client = RpcTestContext::setup().await;

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
    let mut client = RpcTestContext::setup().await;

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
    let mut client = RpcTestContext::setup().await;

    let rpc_msg = nfs3_call(1, NFS_PROGRAM::NFSPROC3_LOOKUP);
    client.send_call(&rpc_msg, &Void).await?;

    let (msg, body) = client.recv_reply::<LOOKUP3res>().await?;
    check_garbage_args_reply(1, &msg)?;
    assert!(body.is_none());

    client.shutdown().await
}

#[tokio::test]
async fn invalid_rpc_ver() -> anyhow::Result<()> {
    let mut client = RpcTestContext::setup().await;

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
    let mut client = RpcTestContext::setup().await;

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
    let mut client = RpcTestContext::setup().await;

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
    let mut client = RpcTestContext::setup().await;

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
