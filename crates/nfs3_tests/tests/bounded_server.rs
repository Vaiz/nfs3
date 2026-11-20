#![allow(clippy::unwrap_used)]
use std::sync::Arc;
use std::time::Duration;

use nfs3_client::bounded::{BoundedRpcClient, RpcLimits};
use nfs3_client::bounded_nfs::BoundedNfsClient;
use nfs3_client::tokio::TokioIo;
use nfs3_server::bounded::{BoundedListener, DirectoryLimits, ServerLimits};
use nfs3_server::memfs::{MemFs, MemFsConfig};
use nfs3_server::nfs3_types::nfs3::*;
use nfs3_server::nfs3_types::rpc::opaque_auth;
use nfs3_server::nfs3_types::xdr_codec::*;
use nfs3_server::nfs3_types::{self};
use nfs3_types::async_io::RecordLimits;
use tokio::net::TcpStream;
mod bounded_server_support;
use bounded_server_support::{Controlled, State};

fn fixture(xid: u32, program: u32, procedure: u32, args: &[u8]) -> Vec<u8> {
    let mut record = Vec::new();
    for word in [
        xid,
        0,
        2,
        program,
        if program == nfs3_types::portmap::PROGRAM {
            2
        } else {
            3
        },
        procedure,
        0,
        0,
        0,
        0,
    ] {
        record.extend_from_slice(&word.to_be_bytes());
    }
    record.extend_from_slice(args);
    record
}

async fn raw_reply(socket: &mut TcpStream) -> Vec<u8> {
    use tokio::io::AsyncReadExt;
    let marker = socket.read_u32().await.unwrap();
    assert_ne!(marker & 0x8000_0000, 0);
    let len = (marker & 0x7fff_ffff) as usize;
    assert!(len <= 8192);
    let mut result = vec![0; len];
    socket.read_exact(&mut result).await.unwrap();
    result
}

async fn raw_send(socket: &mut TcpStream, record: &[u8]) {
    use tokio::io::AsyncWriteExt;
    socket
        .write_all(&(0x8000_0000 | record.len() as u32).to_be_bytes())
        .await
        .unwrap();
    socket.write_all(record).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn graceful_drain_delivers_admitted_write_reply_before_releasing_backing() {
    let state = State::new();
    let _release_on_failure = bounded_server_support::ReleaseOnDrop(Arc::clone(&state));
    let server = Arc::new(
        BoundedListener::bind(
            "127.0.0.1:0",
            Controlled {
                fs: Arc::new(fs()),
                state: Arc::clone(&state),
                return_early: false,
            },
            "/".to_owned(),
            limits(),
        )
        .await
        .unwrap(),
    );
    let mut rpc = client(&server).await;
    let root = root(&mut rpc).await;
    let file = file(&mut rpc, &root).await;
    let call = tokio::spawn(async move {
        rpc.call::<_, WRITE3res>(
            PROGRAM,
            VERSION,
            7,
            &Write3Args {
                file,
                offset: 0,
                count: 3,
                stable: stable_how::FILE_SYNC,
                data: Opaque::borrowed(b"new"),
            },
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(2), state.started.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    let draining = Arc::clone(&server);
    let mut drain = tokio::spawn(async move { draining.drain().await });
    assert!(
        tokio::time::timeout(Duration::from_millis(30), &mut drain)
            .await
            .is_err()
    );
    assert!(server.backing_used() > 0);
    state.release();
    let reply = tokio::time::timeout(Duration::from_secs(2), call)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(matches!(
        reply,
        Nfs3Result::Ok(WRITE3resok { count: 3, .. })
    ));
    tokio::time::timeout(Duration::from_secs(2), drain)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(server.backing_used(), 0);
}

#[tokio::test]
async fn run_and_ready_accept_observe_shutdown_without_another_socket() {
    for ready_socket in [false, true] {
        let server = Arc::new(
            BoundedListener::bind("127.0.0.1:0", fs(), "/".to_owned(), limits())
                .await
                .unwrap(),
        );
        let socket = if ready_socket {
            Some(
                TcpStream::connect(server.local_addr().unwrap())
                    .await
                    .unwrap(),
            )
        } else {
            None
        };
        let running = Arc::clone(&server);
        let run = tokio::spawn(async move { running.run().await });
        if !ready_socket {
            tokio::task::yield_now().await;
        }
        server.drain().await;
        let error = tokio::time::timeout(Duration::from_secs(1), run)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
        assert_eq!(
            server.accept().await.unwrap_err().kind(),
            std::io::ErrorKind::Interrupted
        );
        assert_eq!(server.backing_used(), 0);
        drop(socket);
    }
}

#[tokio::test]
async fn partial_first_marker_uses_progress_and_whole_record_deadlines_without_admission() {
    use tokio::io::AsyncWriteExt;
    for whole in [false, true] {
        let mut l = limits();
        l.idle_timeout = Duration::from_secs(2);
        l.progress_timeout = Duration::from_millis(if whole { 100 } else { 50 });
        l.receive_timeout = Duration::from_millis(if whole { 120 } else { 500 });
        let server = BoundedListener::bind("127.0.0.1:0", fs(), "/".to_owned(), l)
            .await
            .unwrap();
        let mut socket = TcpStream::connect(server.local_addr().unwrap())
            .await
            .unwrap();
        let task = server.accept().await.unwrap().unwrap();
        socket.write_all(&[0x80]).await.unwrap();
        if whole {
            for _ in 0..2 {
                tokio::time::sleep(Duration::from_millis(40)).await;
                socket.write_all(&[0]).await.unwrap();
            }
        }
        tokio::time::timeout(Duration::from_millis(if whole { 60 } else { 200 }), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(server.active_operations(), 0);
        assert_eq!(server.backing_used(), 0);
        server.drain().await;
    }
}

#[tokio::test]
async fn readonly_policy_precedes_count_mismatch_and_listener_drop_stops_idle_reader() {
    let server = BoundedListener::bind_ro("127.0.0.1:0", fs(), "/".to_owned(), limits())
        .await
        .unwrap();
    let mut rpc = client(&server).await;
    let root = root(&mut rpc).await;
    let file = file(&mut rpc, &root).await;
    let result: WRITE3res = rpc
        .call(
            PROGRAM,
            VERSION,
            7,
            &Write3Args {
                file: nfs_fh3::default(),
                offset: 0,
                count: 10,
                stable: stable_how::FILE_SYNC,
                data: Opaque::borrowed(b"x"),
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        result,
        Nfs3Result::Err((nfsstat3::NFS3ERR_ROFS, _))
    ));
    let result: READ3resPayload = rpc
        .call(
            PROGRAM,
            VERSION,
            6,
            &READ3args {
                file,
                offset: 0,
                count: 3,
            },
        )
        .await
        .unwrap();
    assert!(matches!(result, Nfs3Result::Ok(_)));
    let mut idle = TcpStream::connect(server.local_addr().unwrap())
        .await
        .unwrap();
    let task = server.accept().await.unwrap().unwrap();
    drop(server);
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap();
    use tokio::io::AsyncReadExt;
    assert_eq!(idle.read(&mut [0]).await.unwrap(), 0);
    drop(rpc);
}

#[tokio::test]
async fn maximum_bulk_roundtrip_and_directory_host_metadata_bound() {
    let mut l = limits();
    l.directory.max_entries = 128;
    l.directory.max_metadata_bytes = 255;
    let server = BoundedListener::bind("127.0.0.1:0", fs(), "/".to_owned(), l)
        .await
        .unwrap();
    let mut rpc = client(&server).await;
    let root = root(&mut rpc).await;
    let file = file(&mut rpc, &root).await;
    let mut nfs = BoundedNfsClient::new(rpc, 4096).unwrap();
    let mut expected = Vec::new();
    expected.extend((0..4096).map(|i| (i % 251) as u8));
    let data = Payload::try_from_chunks(
        expected.chunks(127).map(<[u8]>::to_vec).collect(),
        PayloadLimits {
            max_logical_bytes: 4096,
            max_backing_capacity: 4096,
            max_segments: 128,
        },
    )
    .unwrap();
    let Nfs3Result::Ok(result) = nfs
        .write(file.clone(), 0, stable_how::FILE_SYNC, data)
        .await
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(result.count, 4096);
    let Nfs3Result::Ok(result) = nfs
        .read(&READ3args {
            file,
            offset: 0,
            count: 4096,
        })
        .await
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(result.data.try_to_vec().unwrap(), expected);
    assert!(result.eof);
    let listing: READDIRPLUS3res = nfs
        .call(
            NFS_PROGRAM::NFSPROC3_READDIRPLUS,
            &READDIRPLUS3args {
                dir: root,
                cookie: 0,
                cookieverf: cookieverf3::default(),
                dircount: u32::MAX,
                maxcount: u32::MAX,
            },
        )
        .await
        .unwrap();
    let Nfs3Result::Ok(listing) = listing else {
        panic!()
    };
    assert_eq!(listing.reply.entries.0.len(), 3); // names + 64-byte handles
    assert!(!listing.reply.eof);
    drop(nfs);
    server.drain().await;
    assert_eq!(server.backing_used(), 0);
}

#[tokio::test]
async fn shared_metadata_operations_mount_and_rpc_mapping() {
    let server = BoundedListener::bind("127.0.0.1:0", fs(), "/".to_owned(), limits())
        .await
        .unwrap();
    let mut rpc = client(&server).await;
    let root = root(&mut rpc).await;
    let file = file(&mut rpc, &root).await;
    let getattr: GETATTR3res = rpc
        .call(
            PROGRAM,
            VERSION,
            1,
            &GETATTR3args {
                object: file.clone(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(getattr, Nfs3Result::Ok(_)));
    let access: ACCESS3res = rpc
        .call(
            PROGRAM,
            VERSION,
            4,
            &ACCESS3args {
                object: file.clone(),
                access: ACCESS3_READ | ACCESS3_MODIFY,
            },
        )
        .await
        .unwrap();
    assert!(matches!(access, Nfs3Result::Ok(_)));
    let pathconf: PATHCONF3res = rpc
        .call(
            PROGRAM,
            VERSION,
            20,
            &PATHCONF3args {
                object: root.clone(),
            },
        )
        .await
        .unwrap();
    let Nfs3Result::Ok(pathconf) = pathconf else {
        panic!()
    };
    assert_eq!(pathconf.name_max, 255);
    let fsstat: FSSTAT3res = rpc
        .call(
            PROGRAM,
            VERSION,
            18,
            &FSSTAT3args {
                fsroot: root.clone(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(fsstat, Nfs3Result::Ok(_)));
    let readlink: READLINK3res = rpc
        .call(PROGRAM, VERSION, 5, &READLINK3args { symlink: file })
        .await
        .unwrap();
    assert!(matches!(
        readlink,
        Nfs3Result::Err((nfsstat3::NFS3ERR_NOTSUPP, _))
    ));
    let name = |name: &'static [u8]| diropargs3 {
        dir: root.clone(),
        name: name.into(),
    };
    let created: CREATE3res = rpc
        .call(
            PROGRAM,
            VERSION,
            8,
            &CREATE3args {
                where_: name(b"created"),
                how: createhow3::EXCLUSIVE(createverf3([3; 8])),
            },
        )
        .await
        .unwrap();
    let Nfs3Result::Ok(created) = created else {
        panic!()
    };
    let post_op_fh3::Some(created) = created.obj else {
        panic!()
    };
    let setattr: SETATTR3res = rpc
        .call(
            PROGRAM,
            VERSION,
            2,
            &SETATTR3args {
                object: created,
                new_attributes: sattr3 {
                    mode: set_mode3::Some(0o644),
                    ..sattr3::default()
                },
                guard: sattrguard3::None,
            },
        )
        .await
        .unwrap();
    assert!(matches!(setattr, Nfs3Result::Ok(_)));
    let renamed: RENAME3res = rpc
        .call(
            PROGRAM,
            VERSION,
            14,
            &RENAME3args {
                from: name(b"created"),
                to: name(b"renamed"),
            },
        )
        .await
        .unwrap();
    assert!(matches!(renamed, Nfs3Result::Ok(_)));
    let removed: REMOVE3res = rpc
        .call(
            PROGRAM,
            VERSION,
            12,
            &REMOVE3args {
                object: name(b"renamed"),
            },
        )
        .await
        .unwrap();
    assert!(matches!(removed, Nfs3Result::Ok(_)));
    let mkdir: MKDIR3res = rpc
        .call(
            PROGRAM,
            VERSION,
            9,
            &MKDIR3args {
                where_: name(b"dir"),
                attributes: sattr3::default(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(mkdir, Nfs3Result::Ok(_)));
    let rmdir: REMOVE3res = rpc
        .call(
            PROGRAM,
            VERSION,
            13,
            &REMOVE3args {
                object: name(b"dir"),
            },
        )
        .await
        .unwrap();
    assert!(matches!(rmdir, Nfs3Result::Ok(_)));
    let symlink: SYMLINK3res = rpc
        .call(
            PROGRAM,
            VERSION,
            10,
            &SYMLINK3args {
                where_: name(b"link"),
                symlink: symlinkdata3 {
                    symlink_attributes: sattr3::default(),
                    symlink_data: b"file".as_slice().into(),
                },
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        symlink,
        Nfs3Result::Err((nfsstat3::NFS3ERR_NOTSUPP, _))
    ));
    let listing: READDIR3res = rpc
        .call(
            PROGRAM,
            VERSION,
            16,
            &READDIR3args {
                dir: root.clone(),
                cookie: 0,
                cookieverf: cookieverf3::default(),
                count: u32::MAX,
            },
        )
        .await
        .unwrap();
    let Nfs3Result::Ok(listing) = listing else {
        panic!()
    };
    assert_eq!(listing.reply.entries.0.len(), 4);
    assert!(!listing.reply.eof);
    let too_small: READDIRPLUS3res = rpc
        .call(
            PROGRAM,
            VERSION,
            17,
            &READDIRPLUS3args {
                dir: root,
                cookie: 0,
                cookieverf: cookieverf3::default(),
                dircount: 0,
                maxcount: 4096,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        too_small,
        Nfs3Result::Err((nfsstat3::NFS3ERR_TOOSMALL, _))
    ));
    let exports: nfs3_types::mount::exports = rpc
        .call(nfs3_types::mount::PROGRAM, 3, 5, &Void)
        .await
        .unwrap();
    assert_eq!(exports.0.len(), 1);
    let _: Void = rpc
        .call(
            nfs3_types::mount::PROGRAM,
            3,
            3,
            &nfs3_types::mount::dirpath(Opaque::borrowed(b"/")),
        )
        .await
        .unwrap();
    let _: Void = rpc
        .call(nfs3_types::mount::PROGRAM, 3, 4, &Void)
        .await
        .unwrap();
    let _: Void = rpc
        .call(nfs3_types::mount::PROGRAM, 3, 0, &Void)
        .await
        .unwrap();
    let _: Void = rpc
        .call(nfs3_types::portmap::PROGRAM, 2, 0, &Void)
        .await
        .unwrap();
    for (program, version, procedure, expected) in
        [(PROGRAM, 2, 0, 2), (PROGRAM, 3, 11, 3), (12345, 3, 0, 1)]
    {
        let error = rpc
            .call::<_, Void>(program, version, procedure, &Void)
            .await
            .unwrap_err();
        let nfs3_client::bounded::BoundedErrorKind::NotAccepted(stat) = error.kind else {
            panic!()
        };
        assert!(matches!(
            (stat, expected),
            (
                nfs3_types::rpc::accept_stat_data::PROG_MISMATCH { low: 3, high: 3 },
                2
            ) | (nfs3_types::rpc::accept_stat_data::PROC_UNAVAIL, 3)
                | (nfs3_types::rpc::accept_stat_data::PROG_UNAVAIL, 1)
        ));
        assert!(rpc.is_usable());
    }
    drop(rpc);
    server.drain().await;
    assert_eq!(server.backing_used(), 0);
}

#[tokio::test]
async fn exact_worst_case_write_envelope_and_checked_configuration() {
    let mut l = limits();
    l.max_write_bytes = 8192 - 928;
    l.decode.payload.max_logical_bytes = l.max_write_bytes as usize;
    assert!(l.validate().is_ok());
    let mut too_large = l;
    too_large.max_write_bytes += 1;
    assert!(too_large.validate().is_err()); // padding also must fit
    too_large = l;
    too_large.backing_bytes = l.operation_envelope().unwrap() - 1;
    assert!(too_large.validate().is_err());
    too_large = l;
    too_large.directory.max_metadata_bytes = usize::MAX;
    assert!(too_large.validate().is_err());
    too_large = l;
    too_large.receive_timeout = Duration::ZERO;
    assert!(too_large.validate().is_err());
    too_large = l;
    too_large.max_read_bytes = u32::MAX;
    assert!(too_large.validate().is_err());
    too_large = l;
    too_large.encode.max_metadata_bytes = 100;
    assert!(too_large.validate().is_err());
    too_large = l;
    too_large.encode.max_chunks = 2;
    assert!(too_large.validate().is_err());

    let server = BoundedListener::bind("127.0.0.1:0", fs(), "/".to_owned(), l)
        .await
        .unwrap();
    let mut socket = TcpStream::connect(server.local_addr().unwrap())
        .await
        .unwrap();
    server.accept().await.unwrap();
    let mut record = fixture(51, PROGRAM, 7, &[])[..24].to_vec();
    for _ in 0..2 {
        record.extend_from_slice(&2_u32.to_be_bytes()); // AUTH_SHORT: opaque body
        record.extend_from_slice(&400_u32.to_be_bytes());
        record.extend_from_slice(&[0x5a; 400]);
    }
    let handle = nfs_fh3 {
        data: Opaque::owned(vec![0; 64]),
    };
    record.extend(write_args(
        &handle,
        l.max_write_bytes,
        2,
        &vec![0x5a; l.max_write_bytes as usize],
    ));
    assert_eq!(record.len(), 8192);
    raw_send(&mut socket, &record).await;
    let reply = raw_reply(&mut socket).await;
    assert_eq!(&reply[20..24], &[0; 4]); // all maxima structurally accepted
    assert_ne!(&reply[24..28], &[0; 4]); // deliberately stale/invalid handle
    drop(socket);
    let mut rpc = client(&server).await;
    let root = root(&mut rpc).await;
    let lookup = |len| LOOKUP3args {
        what: diropargs3 {
            dir: root.clone(),
            name: filename3(Opaque::owned(vec![b'x'; len])),
        },
    };
    let result: LOOKUP3res = rpc.call(PROGRAM, VERSION, 3, &lookup(255)).await.unwrap();
    assert!(matches!(
        result,
        Nfs3Result::Err((nfsstat3::NFS3ERR_NOENT, _))
    ));
    let result: LOOKUP3res = rpc.call(PROGRAM, VERSION, 3, &lookup(256)).await.unwrap();
    assert!(matches!(
        result,
        Nfs3Result::Err((nfsstat3::NFS3ERR_NAMETOOLONG, _))
    ));
    drop(rpc);
    server.drain().await;
    assert_eq!(server.backing_used(), 0);
}

#[tokio::test]
async fn overlimit_names_return_typed_failure_bodies_without_mutation() {
    use nfs3_server::vfs::{NfsFileSystem, NfsReadFileSystem};
    let backend = Arc::new(fs());
    let state = State::new();
    let server = BoundedListener::bind(
        "127.0.0.1:0",
        Controlled {
            fs: Arc::clone(&backend),
            state,
            return_early: false,
        },
        "/".to_owned(),
        limits(),
    )
    .await
    .unwrap();
    let mut rpc = client(&server).await;
    let root = root(&mut rpc).await;
    let name = |long| diropargs3 {
        dir: root.clone(),
        name: if long {
            filename3(Opaque::owned(vec![b'x'; 256]))
        } else {
            b"file".as_slice().into()
        },
    };
    // The backend deliberately supports this name. Any accidental call could
    // mutate it, so verify its handle and mode after REMOVE/RENAME/CREATE/etc.
    let long_name = name(true).name;
    let (long_id, before) = backend
        .create(
            &backend.root_dir(),
            &long_name,
            sattr3 {
                mode: set_mode3::Some(0o600),
                ..sattr3::default()
            },
        )
        .await
        .unwrap();
    let mut socket = TcpStream::connect(server.local_addr().unwrap())
        .await
        .unwrap();
    server.accept().await.unwrap();
    let mut cases = Vec::new();
    macro_rules! case {
        ($proc:expr, $args:expr, $body:expr) => {{
            let mut args = Vec::new();
            $args.pack(&mut args).unwrap();
            cases.push(($proc, args, $body));
        }};
    }
    case!(3, LOOKUP3args { what: name(true) }, 4);
    case!(
        8,
        CREATE3args {
            where_: name(true),
            how: createhow3::UNCHECKED(sattr3 {
                mode: set_mode3::Some(0),
                ..sattr3::default()
            })
        },
        8
    );
    case!(
        9,
        MKDIR3args {
            where_: name(true),
            attributes: sattr3::default()
        },
        8
    );
    case!(
        10,
        SYMLINK3args {
            where_: name(true),
            symlink: symlinkdata3 {
                symlink_attributes: sattr3::default(),
                symlink_data: b"target".as_slice().into(),
            }
        },
        8
    );
    for proc in [12, 13] {
        case!(proc, REMOVE3args { object: name(true) }, 8);
    }
    case!(
        14,
        RENAME3args {
            from: name(false),
            to: name(true)
        },
        16
    );
    case!(
        14,
        RENAME3args {
            from: name(true),
            to: name(false)
        },
        16
    );
    for (proc, args, body_bytes) in cases {
        raw_send(&mut socket, &fixture(90, PROGRAM, proc, &args)).await;
        let reply = raw_reply(&mut socket).await;
        assert_eq!(&reply[20..24], &[0; 4]); // RPC SUCCESS
        assert_eq!(&reply[24..28], &63_u32.to_be_bytes()); // NAMETOOLONG
        assert_eq!(reply.len(), 28 + body_bytes); // includes both RENAME WCCs
        assert!(reply[28..].iter().all(|b| *b == 0));
    }
    assert_eq!(
        backend
            .lookup(&backend.root_dir(), &long_name)
            .await
            .unwrap(),
        long_id
    );
    assert_eq!(backend.getattr(&long_id).await.unwrap().mode, before.mode);
    let _ = file(&mut rpc, &root).await;
    drop(socket);
    drop(rpc);
    server.drain().await;
    assert_eq!(server.backing_used(), 0);
}

#[tokio::test]
async fn directory_envelope_rejects_one_below_metadata_and_reservation_before_io() {
    let mut l = limits();
    let bound = l.directory.reply_metadata_bound().unwrap();
    assert_eq!(bound, 132 + 4 * (184 + 256));
    l.encode.max_metadata_bytes = bound;
    l.encode.max_reservation_bytes = Encoder::metadata_reservation_bound(bound).unwrap();
    assert!(l.validate().is_ok());
    let mut invalid = l;
    invalid.encode.max_metadata_bytes -= 1;
    assert!(invalid.validate().is_err());
    assert!(
        BoundedListener::bind("127.0.0.1:0", fs(), "/".to_owned(), invalid)
            .await
            .err()
            .is_some_and(|e| e.kind() == std::io::ErrorKind::InvalidInput)
    );
    invalid = l;
    invalid.encode.max_reservation_bytes -= 1;
    assert!(invalid.validate().is_err());
    invalid = l;
    // Construction work is constant even for an enormous entry policy.
    invalid.directory.max_entries = usize::MAX;
    invalid.encode = limits().encode;
    assert!(invalid.validate().is_ok());
    invalid.directory.max_name_bytes = usize::MAX;
    assert!(invalid.validate().is_err());

    // Full independent maximum-entry fixture: 64-byte wire handle, full attrs,
    // maximum names, and both list termination words. No backend-size shortcut.
    let attr = fattr3 {
        type_: ftype3::NF3DIR,
        mode: 0,
        nlink: 0,
        uid: 0,
        gid: 0,
        size: 0,
        used: 0,
        rdev: specdata3::default(),
        fsid: 0,
        fileid: 0,
        atime: nfstime3::default(),
        mtime: nfstime3::default(),
        ctime: nfstime3::default(),
    };
    let mut entries = Vec::new();
    for cookie in 1..=4 {
        entries.push(entryplus3 {
            fileid: 1,
            name: filename3(Opaque::owned(vec![b'x'; 255])),
            cookie,
            name_attributes: post_op_attr::Some(attr.clone()),
            name_handle: post_op_fh3::Some(nfs_fh3 {
                data: Opaque::owned(vec![0; 64]),
            }),
        });
    }
    let reply: READDIRPLUS3res = Nfs3Result::Ok(READDIRPLUS3resok {
        dir_attributes: post_op_attr::Some(attr),
        cookieverf: cookieverf3::default(),
        reply: dirlistplus3 {
            entries: List(entries),
            eof: false,
        },
    });
    let mut encoder = Encoder::new(l.encode);
    for word in [99_u32, 1, 0, 0, 0, 0] {
        encoder.encode(&word).unwrap();
    }
    encoder.encode(&reply).unwrap();
    let plan = encoder.finish().unwrap();
    assert_eq!(plan.len(), 132 + 4 * (184 + 256));
    let mut wire = Vec::new();
    for chunk in plan.chunks() {
        wire.extend_from_slice(chunk);
    }
    assert_eq!(&wire[wire.len() - 8..], &[0; 8]);
    use std::io::Cursor;
    let (decoded, _) = READDIRPLUS3res::unpack(&mut Cursor::new(&wire[24..])).unwrap();
    let Nfs3Result::Ok(decoded) = decoded else {
        panic!()
    };
    assert_eq!(decoded.reply.entries.0.len(), 4);
    let server = BoundedListener::bind("127.0.0.1:0", fs(), "/".to_owned(), l)
        .await
        .unwrap();
    let mut rpc = client(&server).await;
    let root = root(&mut rpc).await;
    let result: READDIRPLUS3res = rpc
        .call(
            PROGRAM,
            VERSION,
            17,
            &READDIRPLUS3args {
                dir: root,
                cookie: 0,
                cookieverf: cookieverf3::default(),
                dircount: u32::MAX,
                maxcount: u32::MAX,
            },
        )
        .await
        .unwrap();
    assert!(matches!(result, Nfs3Result::Ok(_)));
    drop(rpc);
    server.drain().await;
    assert_eq!(server.backing_used(), 0);
}

#[tokio::test]
async fn authorization_preserves_none_sys_and_portmap_exemption() {
    use std::sync::atomic::Ordering;

    use nfs3_types::rpc::{auth_flavor, auth_unix};
    let state = State::new();
    let server = BoundedListener::bind(
        "127.0.0.1:0",
        Controlled {
            fs: Arc::new(fs()),
            state: Arc::clone(&state),
            return_early: false,
        },
        "/".to_owned(),
        limits(),
    )
    .await
    .unwrap();
    let mut rpc = client(&server).await;
    let _: Void = rpc.call(PROGRAM, VERSION, 0, &Void).await.unwrap();
    state.reject_auth.store(true, Ordering::Release);
    let error = rpc
        .call::<_, Void>(PROGRAM, VERSION, 0, &Void)
        .await
        .unwrap_err();
    assert!(matches!(
        error.kind,
        nfs3_client::bounded::BoundedErrorKind::Rejected(
            nfs3_types::rpc::rejected_reply::AUTH_ERROR(nfs3_types::rpc::auth_stat::AUTH_TOOWEAK)
        )
    ));
    let port: u32 = rpc
        .call(
            nfs3_types::portmap::PROGRAM,
            2,
            3,
            &nfs3_types::portmap::mapping {
                prog: PROGRAM,
                vers: VERSION,
                prot: 6,
                port: 0,
            },
        )
        .await
        .unwrap();
    assert_eq!(port, u32::from(server.local_addr().unwrap().port()));
    state.reject_auth.store(false, Ordering::Release);
    // AUTH_SYS max machine/group fields and non-default uid reach authorize.
    let mut auth = auth_unix {
        stamp: 1,
        machinename: Opaque::owned(vec![b'x'; 255]),
        uid: 42,
        gid: 43,
        gids: vec![44; 16],
    };
    for (uid, accepted) in [(42, true), (41, false)] {
        auth.uid = uid;
        let body = opaque_auth::auth_unix(&auth);
        assert_eq!(body.flavor, auth_flavor::AUTH_UNIX);
        let mut record = fixture(67, PROGRAM, 0, &[])[..24].to_vec();
        record.extend_from_slice(&1_u32.to_be_bytes());
        record.extend_from_slice(&(body.body.len() as u32).to_be_bytes());
        record.extend_from_slice(&body.body);
        record.extend_from_slice(&[0; 8]);
        let mut socket = TcpStream::connect(server.local_addr().unwrap())
            .await
            .unwrap();
        server.accept().await.unwrap();
        raw_send(&mut socket, &record).await;
        let reply = raw_reply(&mut socket).await;
        assert_eq!(&reply[8..12], &u32::from(!accepted).to_be_bytes());
    }
    drop(rpc);
    server.drain().await;
}

fn write_args(handle: &nfs_fh3, count: u32, stable: u32, data: &[u8]) -> Vec<u8> {
    let mut args = (handle.data.len() as u32).to_be_bytes().to_vec();
    args.extend_from_slice(&handle.data);
    while !args.len().is_multiple_of(4) {
        args.push(0);
    }
    args.extend_from_slice(&0_u64.to_be_bytes());
    args.extend_from_slice(&count.to_be_bytes());
    args.extend_from_slice(&stable.to_be_bytes());
    args.extend_from_slice(&(data.len() as u32).to_be_bytes());
    args.extend_from_slice(data);
    while !args.len().is_multiple_of(4) {
        args.push(0);
    }
    args
}

#[tokio::test]
async fn standard_file_direct_fill_stability_commit_and_short_zero_eof() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("file");
    std::fs::write(&path, b"hello world").unwrap();
    let state = State::new();
    state.file.set(path.clone()).unwrap();
    let server = BoundedListener::bind(
        "127.0.0.1:0",
        Controlled {
            fs: Arc::new(fs()),
            state,
            return_early: false,
        },
        "/".to_owned(),
        limits(),
    )
    .await
    .unwrap();
    let mut rpc = client(&server).await;
    let root = root(&mut rpc).await;
    let file = file(&mut rpc, &root).await;
    let mut nfs = BoundedNfsClient::new(rpc, 4096).unwrap();
    for (offset, count, expected, eof) in [
        (0, 0, b"".as_slice(), false),
        (6, 4096, b"world".as_slice(), true),
        (11, 0, b"".as_slice(), true),
        (99, 4096, b"".as_slice(), true),
    ] {
        let Nfs3Result::Ok(result) = nfs
            .read(&READ3args {
                file: file.clone(),
                offset,
                count,
            })
            .await
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(result.data.try_to_vec().unwrap(), expected);
        assert_eq!(result.eof, eof);
    }
    for stable in [
        stable_how::UNSTABLE,
        stable_how::DATA_SYNC,
        stable_how::FILE_SYNC,
    ] {
        let data = Payload::try_from_chunks(
            vec![b"n".to_vec(), b"ew".to_vec()],
            PayloadLimits {
                max_logical_bytes: 3,
                max_backing_capacity: 3,
                max_segments: 2,
            },
        )
        .unwrap();
        let Nfs3Result::Ok(result) = nfs.write(file.clone(), 0, stable, data).await.unwrap() else {
            panic!()
        };
        assert_eq!(result.committed, stable);
        assert_eq!(result.count, 3);
        let commit: COMMIT3res = nfs
            .call(
                NFS_PROGRAM::NFSPROC3_COMMIT,
                &COMMIT3args {
                    file: file.clone(),
                    offset: 0,
                    count: 0,
                },
            )
            .await
            .unwrap();
        let Nfs3Result::Ok(commit) = commit else {
            panic!()
        };
        assert_eq!(commit.verf, result.verf);
    }
    assert_eq!(std::fs::read(path).unwrap(), b"newlo world");
    drop(nfs);
    server.drain().await;
    assert_eq!(server.backing_used(), 0);
}

#[tokio::test]
async fn malformed_auth_padding_trailing_and_count_policy_precede_mutation_and_xid_is_not_dedup() {
    let state = State::new();
    state.release();
    let server = BoundedListener::bind(
        "127.0.0.1:0",
        Controlled {
            fs: Arc::new(fs()),
            state: Arc::clone(&state),
            return_early: false,
        },
        "/".to_owned(),
        limits(),
    )
    .await
    .unwrap();
    let mut rpc = client(&server).await;
    let root = root(&mut rpc).await;
    let file = file(&mut rpc, &root).await;
    let mut socket = TcpStream::connect(server.local_addr().unwrap())
        .await
        .unwrap();
    server.accept().await.unwrap();
    let valid = write_args(&file, 3, 2, b"new");
    let mut trailing = fixture(33, PROGRAM, 7, &valid);
    trailing.extend_from_slice(&[0; 4]);
    let mut padding = valid.clone();
    *padding.last_mut().unwrap() = 1;
    for (record, rpc_status, nfs_status) in [
        (trailing, 4_u32, None),
        (fixture(33, PROGRAM, 7, &padding), 4, None),
        (
            fixture(33, PROGRAM, 7, &write_args(&file, 3, 3, b"new")),
            4,
            None,
        ),
        (
            fixture(33, PROGRAM, 7, &write_args(&file, 4, 2, b"new")),
            0,
            Some(22),
        ),
    ] {
        raw_send(&mut socket, &record).await;
        let reply = raw_reply(&mut socket).await;
        assert_eq!(&reply[20..24], &rpc_status.to_be_bytes());
        if let Some(status) = nfs_status {
            assert_eq!(&reply[24..28], &u32::to_be_bytes(status));
        }
    }
    // Independent AUTH_SYS body contains only stamp, missing mandatory fields.
    let mut auth = fixture(33, PROGRAM, 7, &[])[..24].to_vec();
    for word in [1_u32, 4, 0, 0, 0] {
        auth.extend_from_slice(&word.to_be_bytes());
    }
    auth.extend_from_slice(&valid);
    raw_send(&mut socket, &auth).await;
    let reply = raw_reply(&mut socket).await;
    assert_eq!(&reply[8..], &[0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1]); // DENIED/AUTH_ERROR/BADCRED
    assert_eq!(state.started.available_permits(), 0);
    // Same complete record, same XID, twice: both execute and return outcomes.
    for _ in 0..2 {
        raw_send(&mut socket, &fixture(33, PROGRAM, 7, &valid)).await;
        let reply = raw_reply(&mut socket).await;
        assert_eq!(&reply[20..28], &[0; 8]);
    }
    assert_eq!(state.started.available_permits(), 2);
    drop(socket);
    drop(rpc);
    server.drain().await;
    assert_eq!(server.backing_used(), 0);
}

#[tokio::test]
async fn read_direct_fill_prefix_eof_zero_and_configured_maximum() {
    let state = State::new();
    let server = BoundedListener::bind(
        "127.0.0.1:0",
        Controlled {
            fs: Arc::new(fs()),
            state,
            return_early: false,
        },
        "/".to_owned(),
        limits(),
    )
    .await
    .unwrap();
    let mut rpc = client(&server).await;
    let root = root(&mut rpc).await;
    let file = file(&mut rpc, &root).await;
    let mut nfs = BoundedNfsClient::new(rpc, 4096).unwrap();
    for (offset, count, expected, eof) in [
        (0, 3, b"hel".as_slice(), false),
        (6, 4096, b"world".as_slice(), true),
        (0, 0, b"".as_slice(), false),
        (11, 0, b"".as_slice(), true),
        (u64::MAX, 4096, b"".as_slice(), true),
    ] {
        let Nfs3Result::Ok(result) = nfs
            .read(&READ3args {
                file: file.clone(),
                offset,
                count,
            })
            .await
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(result.data.try_to_vec().unwrap(), expected);
        assert_eq!(result.eof, eof);
        assert_eq!(result.count as usize, expected.len());
    }
    drop(nfs);
    server.drain().await;
    assert_eq!(server.backing_used(), 0);
}

#[tokio::test]
async fn encode_failure_falls_back_only_for_nonmutation() {
    use nfs3_server::vfs::NfsReadFileSystem;
    let backend = Arc::new(fs());
    let state = State::new();
    state.release();
    let mut l = limits();
    l.encode.max_metadata_bytes = 160;
    l.directory.max_wire_bytes = 128;
    let server = BoundedListener::bind(
        "127.0.0.1:0",
        Controlled {
            fs: Arc::clone(&backend),
            state,
            return_early: false,
        },
        "/".to_owned(),
        l,
    )
    .await
    .unwrap();
    let mut rpc = client(&server).await;
    let root = root(&mut rpc).await;
    let error = rpc
        .call::<_, LOOKUP3res>(
            PROGRAM,
            VERSION,
            3,
            &LOOKUP3args {
                what: diropargs3 {
                    dir: root.clone(),
                    name: b"file".as_slice().into(),
                },
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error.kind,
        nfs3_client::bounded::BoundedErrorKind::NotAccepted(
            nfs3_types::rpc::accept_stat_data::SYSTEM_ERR
        )
    ));
    assert!(rpc.is_usable());
    let error = rpc
        .call::<_, CREATE3res>(
            PROGRAM,
            VERSION,
            8,
            &CREATE3args {
                where_: diropargs3 {
                    dir: root,
                    name: b"created".as_slice().into(),
                },
                how: createhow3::UNCHECKED(sattr3::default()),
            },
        )
        .await
        .unwrap_err();
    assert!(error.io_started && !error.connection_usable);
    assert!(
        backend
            .lookup(&backend.root_dir(), &b"created".as_slice().into())
            .await
            .is_ok()
    );
    drop(rpc);
    server.drain().await;
    assert_eq!(server.backing_used(), 0);
}

#[tokio::test]
async fn whole_envelope_failfast_before_body_and_first_record_makes_progress() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut l = limits();
    let envelope = l.operation_envelope().unwrap();
    l.backing_bytes = envelope + envelope / 2;
    let server = BoundedListener::bind("127.0.0.1:0", fs(), "/".to_owned(), l)
        .await
        .unwrap();
    let mut first = TcpStream::connect(server.local_addr().unwrap())
        .await
        .unwrap();
    server.accept().await.unwrap();
    // Both records exceed half the request-backing allowance. The domain can
    // admit only one whole envelope: there is no incremental credit deadlock.
    let mut record = fixture(17, PROGRAM, 0, &[]);
    record.resize(6000, 0);
    first
        .write_all(&0x8000_1770_u32.to_be_bytes())
        .await
        .unwrap();
    first.write_all(&record[..1]).await.unwrap();
    for _ in 0..100 {
        if server.active_operations() == 1 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(server.active_operations(), 1);
    let mut second = TcpStream::connect(server.local_addr().unwrap())
        .await
        .unwrap();
    let task = server.accept().await.unwrap().unwrap();
    second
        .write_all(&0x8000_1770_u32.to_be_bytes())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(second.read(&mut [0; 1]).await.unwrap(), 0);
    // First can finish from its initial reservation, no new global grant.
    first.write_all(&record[1..]).await.unwrap();
    let reply = raw_reply(&mut first).await;
    assert_eq!(&reply[..4], &17_u32.to_be_bytes());
    assert_eq!(&reply[20..24], &4_u32.to_be_bytes()); // exact finish => GARBAGE_ARGS
    drop(first);
    server.drain().await;
    assert_eq!(server.backing_used(), 0);
}

#[tokio::test]
async fn connection_and_operation_slot_exhaustion_close_without_body() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut l = limits();
    l.max_connections = 1;
    let server = BoundedListener::bind("127.0.0.1:0", fs(), "/".to_owned(), l)
        .await
        .unwrap();
    let first = TcpStream::connect(server.local_addr().unwrap())
        .await
        .unwrap();
    server.accept().await.unwrap();
    let mut second = TcpStream::connect(server.local_addr().unwrap())
        .await
        .unwrap();
    assert!(server.accept().await.unwrap().is_none());
    assert_eq!(second.read(&mut [0]).await.unwrap(), 0);
    drop(first);
    server.drain().await;

    l.max_connections = 3;
    l.max_operations = 1;
    let server = BoundedListener::bind("127.0.0.1:0", fs(), "/".to_owned(), l)
        .await
        .unwrap();
    let mut first = TcpStream::connect(server.local_addr().unwrap())
        .await
        .unwrap();
    server.accept().await.unwrap();
    first
        .write_all(&0x8000_1770_u32.to_be_bytes())
        .await
        .unwrap();
    while server.active_operations() == 0 {
        tokio::task::yield_now().await;
    }
    let mut second = TcpStream::connect(server.local_addr().unwrap())
        .await
        .unwrap();
    let task = server.accept().await.unwrap().unwrap();
    second
        .write_all(&0x8000_0040_u32.to_be_bytes())
        .await
        .unwrap();
    task.await.unwrap();
    assert_eq!(second.read(&mut [0]).await.unwrap(), 0);
    assert_eq!(server.active_operations(), 1);
    drop(first);
    server.drain().await;
    assert_eq!(server.backing_used(), 0);
}

fn limits() -> ServerLimits {
    let payload = PayloadLimits {
        max_logical_bytes: 4096,
        max_backing_capacity: 8192,
        max_segments: 128,
    };
    ServerLimits {
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
            max_wire_bytes: 4096,
            max_entries: 4,
            max_name_bytes: 255,
            max_metadata_bytes: 2048,
        },
        max_connections: 8,
        max_operations: 2,
        backing_bytes: 1024 * 1024,
        control_bytes: 16384,
        idle_timeout: Duration::from_secs(2),
        progress_timeout: Duration::from_millis(150),
        receive_timeout: Duration::from_millis(400),
        send_timeout: Duration::from_secs(2),
    }
}

fn fs() -> MemFs {
    let mut config = MemFsConfig::default();
    config.add_file("/file", b"hello world");
    for i in 0..10 {
        config.add_file(&format!("/entry{i}"), b"");
    }
    MemFs::new(config).unwrap()
}

async fn client<T: nfs3_server::vfs::NfsOwnedIo + 'static>(
    server: &BoundedListener<T>,
) -> BoundedRpcClient<TokioIo<TcpStream>> {
    let socket = TcpStream::connect(server.local_addr().unwrap())
        .await
        .unwrap();
    server.accept().await.unwrap();
    let l = limits();
    BoundedRpcClient::new(
        TokioIo::new(socket),
        RpcLimits {
            block_bytes: l.record.block_bytes,
            max_record_bytes: l.record.max_record_bytes,
            max_fragments: l.record.max_fragments,
            backing_bytes: 32768,
            max_builder_reservation_bytes: l.record.max_builder_reservation_bytes,
            decode: l.decode,
            encode: l.encode,
        },
        opaque_auth::default(),
        opaque_auth::default(),
    )
    .unwrap()
}

async fn root(rpc: &mut BoundedRpcClient<TokioIo<TcpStream>>) -> nfs_fh3 {
    let result: nfs3_types::mount::mountres3 = rpc
        .call(
            nfs3_types::mount::PROGRAM,
            3,
            1,
            &nfs3_types::mount::dirpath(Opaque::borrowed(b"/")),
        )
        .await
        .unwrap();
    let nfs3_types::mount::mountres3::Ok(result) = result else {
        panic!()
    };
    nfs_fh3 {
        data: result.fhandle.0,
    }
}

async fn file(rpc: &mut BoundedRpcClient<TokioIo<TcpStream>>, root: &nfs_fh3) -> nfs_fh3 {
    let result: LOOKUP3res = rpc
        .call(
            PROGRAM,
            VERSION,
            3,
            &LOOKUP3args {
                what: diropargs3 {
                    dir: root.clone(),
                    name: b"file".as_slice().into(),
                },
            },
        )
        .await
        .unwrap();
    let Nfs3Result::Ok(result) = result else {
        panic!()
    };
    result.object
}

#[tokio::test]
async fn live_owned_read_write_stable_commit_and_directory() {
    let server = BoundedListener::bind("127.0.0.1:0", fs(), "/".to_owned(), limits())
        .await
        .unwrap();
    let mut rpc = client(&server).await;
    let root = root(&mut rpc).await;
    let file = file(&mut rpc, &root).await;
    let mut nfs = BoundedNfsClient::new(rpc, 4096).unwrap();
    let Nfs3Result::Ok(read) = nfs
        .read(&READ3args {
            file: file.clone(),
            offset: 0,
            count: 100,
        })
        .await
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(read.data.try_to_vec().unwrap(), b"hello world");
    assert!(read.eof);
    for stable in [
        stable_how::UNSTABLE,
        stable_how::DATA_SYNC,
        stable_how::FILE_SYNC,
    ] {
        let Nfs3Result::Ok(written) = nfs
            .write(file.clone(), 0, stable, read.data.try_clone().unwrap())
            .await
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(written.count, 11);
        assert_eq!(written.committed, stable_how::FILE_SYNC);
        let commit: COMMIT3res = nfs
            .call(
                NFS_PROGRAM::NFSPROC3_COMMIT,
                &COMMIT3args {
                    file: file.clone(),
                    offset: 0,
                    count: 0,
                },
            )
            .await
            .unwrap();
        let Nfs3Result::Ok(commit) = commit else {
            panic!()
        };
        assert_eq!(commit.verf, written.verf);
    }
    let result: READDIRPLUS3res = nfs
        .call(
            NFS_PROGRAM::NFSPROC3_READDIRPLUS,
            &READDIRPLUS3args {
                dir: root.clone(),
                cookie: 0,
                cookieverf: cookieverf3::default(),
                dircount: u32::MAX,
                maxcount: u32::MAX,
            },
        )
        .await
        .unwrap();
    let Nfs3Result::Ok(result) = result else {
        panic!()
    };
    assert_eq!(result.reply.entries.0.len(), 4);
    assert!(!result.reply.eof);
    assert_ne!(result.reply.entries.0.last().unwrap().cookie, 0);
    let info: FSINFO3res = nfs
        .call(
            NFS_PROGRAM::NFSPROC3_FSINFO,
            &FSINFO3args {
                fsroot: root.clone(),
            },
        )
        .await
        .unwrap();
    let Nfs3Result::Ok(info) = info else { panic!() };
    assert_eq!((info.rtmax, info.wtmax, info.dtpref), (4096, 4096, 4096));
    drop(nfs);
    drop(read);
    tokio::time::timeout(Duration::from_secs(2), server.drain())
        .await
        .unwrap();
    assert_eq!(server.backing_used(), 0);
}

#[tokio::test]
async fn whole_record_trickle_expires_while_healthy_connection_progresses() {
    use tokio::io::AsyncWriteExt;
    let server = Arc::new(
        BoundedListener::bind("127.0.0.1:0", fs(), "/".to_owned(), limits())
            .await
            .unwrap(),
    );
    let mut slow = TcpStream::connect(server.local_addr().unwrap())
        .await
        .unwrap();
    server.accept().await.unwrap();
    slow.write_all(&0x8000_1000_u32.to_be_bytes())
        .await
        .unwrap();
    let trickle = tokio::spawn(async move {
        for _ in 0..12 {
            if slow.write_all(&[0]).await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(60)).await;
        }
    });
    let mut healthy = client(&server).await;
    let _: Void = healthy.call(PROGRAM, VERSION, 0, &Void).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(server.active_operations(), 0);
    trickle.await.unwrap();
    drop(healthy);
    server.drain().await;
    assert_eq!(server.backing_used(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_write_job_survives_disconnect_cancellation_and_early_outer_return() {
    use std::sync::atomic::Ordering;
    for (cancel_connection, return_early) in [(false, false), (true, false), (true, true)] {
        let state = State::new();
        let _release_on_failure = bounded_server_support::ReleaseOnDrop(Arc::clone(&state));
        let backend = Arc::new(fs());
        let server = BoundedListener::bind(
            "127.0.0.1:0",
            Controlled {
                fs: Arc::clone(&backend),
                state: Arc::clone(&state),
                return_early,
            },
            "/".to_owned(),
            limits(),
        )
        .await
        .unwrap();
        // A setup connection obtains this listener's actual file handle.
        let mut rpc = client(&server).await;
        let root = root(&mut rpc).await;
        let file = file(&mut rpc, &root).await;
        drop(rpc);
        let socket = TcpStream::connect(server.local_addr().unwrap())
            .await
            .unwrap();
        let connection = server.accept().await.unwrap().unwrap();
        let l = limits();
        let mut rpc = BoundedRpcClient::new(
            TokioIo::new(socket),
            RpcLimits {
                block_bytes: 128,
                max_record_bytes: 8192,
                max_fragments: 256,
                backing_bytes: 16384,
                max_builder_reservation_bytes: 65536,
                decode: l.decode,
                encode: l.encode,
            },
            opaque_auth::default(),
            opaque_auth::default(),
        )
        .unwrap();
        let args = Write3Args {
            file,
            offset: 0,
            count: 3,
            stable: stable_how::FILE_SYNC,
            data: Opaque::borrowed(b"new"),
        };
        let call =
            tokio::spawn(async move { rpc.call::<_, WRITE3res>(PROGRAM, VERSION, 7, &args).await });
        state.started.acquire().await.unwrap().forget();
        let charged = server.backing_used();
        assert!(charged > 8192);
        if !cancel_connection {
            let mut healthy = client(&server).await;
            let _: Void = healthy.call(PROGRAM, VERSION, 0, &Void).await.unwrap();
            drop(healthy);
            tokio::time::sleep(l.receive_timeout + Duration::from_millis(20)).await;
            assert!(!call.is_finished()); // IO deadlines never time out backend execution
        }
        if cancel_connection {
            connection.abort();
        }
        call.abort(); // drops socket, never the admitted job
        tokio::task::yield_now().await;
        assert!(!state.complete.load(Ordering::Acquire));
        assert_eq!(server.active_operations(), 1);
        assert!(server.backing_used() > 0);
        assert!(
            tokio::time::timeout(Duration::from_millis(30), server.drain())
                .await
                .is_err()
        );
        state.release();
        tokio::time::timeout(Duration::from_secs(2), server.drain())
            .await
            .unwrap();
        assert!(state.complete.load(Ordering::Acquire));
        assert_eq!(server.backing_used(), 0);
        use nfs3_server::vfs::NfsReadFileSystem;
        let id = backend
            .lookup(&backend.root_dir(), &b"file".as_slice().into())
            .await
            .unwrap();
        assert_eq!(backend.read(&id, 0, 3).await.unwrap().0, b"new");
    }
}
