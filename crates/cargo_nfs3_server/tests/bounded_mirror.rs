#![allow(clippy::unwrap_used, dead_code)]
#[path = "../src/mirror/mod.rs"]
mod mirror;
#[path = "../src/string_ext.rs"]
mod string_ext;
#[path = "../src/threshold_logger.rs"]
mod threshold_logger;

use std::collections::HashSet;
use std::time::Duration;

use nfs3_server::bounded::{BoundedListener, DirectoryLimits, ServerLimits};
use nfs3_server::nfs3_types::async_io::RecordLimits;
use nfs3_server::nfs3_types::nfs3::*;
use nfs3_server::nfs3_types::rpc::*;
use nfs3_server::nfs3_types::xdr_codec::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::test]
async fn mirror_lookahead_resume_aliases_are_single_use_and_last_yielded_continues() {
    use nfs3_server::vfs::{NextResult, NfsReadFileSystem, ReadDirIterator};
    let temp = tempfile::tempdir().unwrap();
    for name in ["a", "b", "c"] {
        std::fs::write(temp.path().join(name), b"").unwrap();
    }
    let fs = mirror::Fs::new(temp.path());
    let root = fs.root_dir();
    let mut iter = fs.readdir(&root, 0).await.unwrap();
    let NextResult::Ok(first) = iter.next().await else {
        panic!()
    };
    let NextResult::Ok(held) = iter.next().await else {
        panic!()
    };
    drop(iter);
    let mut replay = fs.readdir(&root, first.cookie).await.unwrap();
    assert!(matches!(
        fs.readdir(&root, held.cookie).await,
        Err(nfsstat3::NFS3ERR_BAD_COOKIE)
    ));
    let NextResult::Ok(again) = replay.next().await else {
        panic!()
    };
    assert_eq!((again.name, again.cookie), (held.name, held.cookie));
    drop(replay);
    let mut continued = fs.readdir(&root, held.cookie).await.unwrap();
    let NextResult::Ok(last) = continued.next().await else {
        panic!()
    };
    assert_ne!(last.cookie, held.cookie);
    assert_ne!(last.name, first.name);
    assert!(matches!(continued.next().await, NextResult::Eof));
    drop(continued);
    for cookie in [first.cookie, held.cookie, last.cookie, u64::MAX] {
        assert!(matches!(
            fs.readdir(&root, cookie).await,
            Err(nfsstat3::NFS3ERR_BAD_COOKIE)
        ));
    }
}

async fn call<A: Pack + Sync, R: Unpack>(socket: &mut TcpStream, proc: u32, args: &A) -> R {
    let mut request = Vec::new();
    rpc_msg {
        xid: 1,
        body: msg_body::CALL(call_body {
            rpcvers: 2,
            prog: PROGRAM,
            vers: VERSION,
            proc,
            cred: opaque_auth::default(),
            verf: opaque_auth::default(),
        }),
    }
    .pack(&mut request)
    .unwrap();
    args.pack(&mut request).unwrap();
    socket
        .write_u32(0x8000_0000 | u32::try_from(request.len()).unwrap())
        .await
        .unwrap();
    socket.write_all(&request).await.unwrap();
    let len = socket.read_u32().await.unwrap() & 0x7fff_ffff;
    assert!(len <= 8192);
    let mut reply = vec![0; len as usize];
    socket.read_exact(&mut reply).await.unwrap();
    let mut input = reply.as_slice();
    let (header, _) = rpc_msg::unpack(&mut input).unwrap();
    assert!(matches!(
        header.body,
        msg_body::REPLY(reply_body::MSG_ACCEPTED(accepted_reply {
            reply_data: accept_stat_data::SUCCESS,
            ..
        }))
    ));
    let (result, _) = R::unpack(&mut input).unwrap();
    assert!(input.is_empty());
    result
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // One pagination oracle shared by READDIR and READDIRPLUS.
async fn bounded_mirror_paginates_host_and_wire_caps_without_lost_or_duplicate_names() {
    // Both procedures consume one entry of lookahead when the page fills.
    for (plus, wire_cap) in [(false, 4096), (true, 4096), (false, 200), (true, 360)] {
        let temp = tempfile::tempdir().unwrap();
        let expected: HashSet<_> = (0..13)
            .map(|i| format!("entry-{i:02}").into_bytes())
            .collect();
        for name in &expected {
            std::fs::write(temp.path().join(std::str::from_utf8(name).unwrap()), b"").unwrap();
        }
        let payload = PayloadLimits {
            max_logical_bytes: 4096,
            max_backing_capacity: 8192,
            max_segments: 128,
        };
        let limits = ServerLimits {
            record: RecordLimits {
                block_bytes: 128,
                max_record_bytes: 8192,
                max_fragments: 256,
                max_builder_reservation_bytes: 65536,
            },
            decode: DecodeLimits {
                max_metadata_bytes: 4096,
                max_collection_elements: 128,
                max_depth: 32,
                max_reservation_bytes: 65536,
                payload,
            },
            encode: EncodeLimits {
                max_wire_bytes: 8192,
                max_metadata_bytes: 8192,
                max_chunks: 256,
                max_retained_capacity: 8192,
                max_depth: 32,
                max_reservation_bytes: 65536,
            },
            max_read_bytes: 4096,
            max_write_bytes: 4096,
            directory: DirectoryLimits {
                max_wire_bytes: wire_cap,
                max_entries: 4,
                max_name_bytes: 255,
                max_metadata_bytes: 2048,
            },
            max_connections: 2,
            max_operations: 1,
            backing_bytes: 1024 * 1024,
            control_bytes: 16384,
            idle_timeout: Duration::from_secs(2),
            progress_timeout: Duration::from_secs(1),
            receive_timeout: Duration::from_secs(2),
            send_timeout: Duration::from_secs(2),
        };
        let server = BoundedListener::bind(
            "127.0.0.1:0",
            mirror::Fs::new(temp.path()),
            "/".to_owned(),
            limits,
        )
        .await
        .unwrap();
        let mut socket = TcpStream::connect(server.local_addr().unwrap())
            .await
            .unwrap();
        server.accept().await.unwrap();
        // The converter's generation is listener-specific: mount on this socket.
        // Use the ordinary MOUNT wire request and decode its success handle.
        let mut request = Vec::new();
        for word in [2_u32, 0, 2, 100_005, 3, 1, 0, 0, 0, 0, 1] {
            request.extend_from_slice(&word.to_be_bytes());
        }
        request.extend_from_slice(b"/\0\0\0");
        socket
            .write_u32(0x8000_0000 | u32::try_from(request.len()).unwrap())
            .await
            .unwrap();
        socket.write_all(&request).await.unwrap();
        let len = socket.read_u32().await.unwrap() & 0x7fff_ffff;
        assert!(len < 1024);
        let mut reply = vec![0; len as usize];
        socket.read_exact(&mut reply).await.unwrap();
        let mut input = &reply[24..];
        let (mount, _) = nfs3_server::nfs3_types::mount::mountres3::unpack(&mut input).unwrap();
        let nfs3_server::nfs3_types::mount::mountres3::Ok(mount) = mount else {
            panic!()
        };
        let dir = nfs_fh3 {
            data: mount.fhandle.0,
        };
        let mut seen = HashSet::new();
        let mut cookie = 0;
        let mut cookieverf = cookieverf3::default();
        let mut finished = false;
        for _ in 0..20 {
            let (entries, eof) = if plus {
                let result: READDIRPLUS3res = call(
                    &mut socket,
                    17,
                    &READDIRPLUS3args {
                        dir: dir.clone(),
                        cookie,
                        cookieverf,
                        dircount: u32::MAX,
                        maxcount: u32::MAX,
                    },
                )
                .await;
                let Nfs3Result::Ok(result) = result else {
                    panic!("{result:?}")
                };
                cookieverf = result.cookieverf;
                (
                    result
                        .reply
                        .entries
                        .0
                        .into_iter()
                        .map(|e| (e.name, e.cookie))
                        .collect::<Vec<_>>(),
                    result.reply.eof,
                )
            } else {
                let result: READDIR3res = call(
                    &mut socket,
                    16,
                    &READDIR3args {
                        dir: dir.clone(),
                        cookie,
                        cookieverf,
                        count: u32::MAX,
                    },
                )
                .await;
                let Nfs3Result::Ok(result) = result else {
                    panic!("{result:?}")
                };
                cookieverf = result.cookieverf;
                (
                    result
                        .reply
                        .entries
                        .0
                        .into_iter()
                        .map(|e| (e.name, e.cookie))
                        .collect::<Vec<_>>(),
                    result.reply.eof,
                )
            };
            assert!(entries.len() <= 4);
            if !eof {
                if wire_cap == 4096 {
                    assert_eq!(entries.len(), 4);
                } else {
                    assert!(
                        entries.len() < 4,
                        "wire limit must fill before the host entry cap"
                    );
                }
            }
            assert!(!entries.is_empty() || eof);
            for (name, next) in entries {
                assert!(seen.insert(name.0.into_owned()), "duplicate entry");
                assert_ne!(cookie, next);
                cookie = next;
            }
            if eof {
                finished = true;
                break;
            }
        }
        assert!(finished);
        assert_eq!(seen, expected);
        drop(socket);
        tokio::time::timeout(Duration::from_secs(2), server.drain())
            .await
            .unwrap();
        assert_eq!(server.backing_used(), 0);
    }
}
