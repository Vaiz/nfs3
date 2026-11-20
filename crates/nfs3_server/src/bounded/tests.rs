#![allow(clippy::unwrap_used)]
use nfs3_types::nfs3::*;
use nfs3_types::xdr_codec::{Pack, PayloadLimits};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;
use crate::memfs::{MemFs, MemFsConfig};
use crate::vfs::NfsReadFileSystem;

const fn limits() -> ServerLimits {
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
            payload: PayloadLimits {
                max_logical_bytes: 4096,
                max_backing_capacity: 8192,
                max_segments: 128,
            },
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
        max_connections: 4,
        max_operations: 2,
        backing_bytes: 1024 * 1024,
        control_bytes: 16384,
        idle_timeout: Duration::from_secs(2),
        progress_timeout: Duration::from_millis(200),
        receive_timeout: Duration::from_secs(1),
        send_timeout: Duration::from_millis(600),
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // End-to-end timeout/release oracle for both slow-peer modes.
async fn bounded_server_stalled_and_trickle_send_release_lease_and_allow_healthy_peer() {
    // A 128-byte duplex buffer deterministically blocks a 4KiB actual READ
    // reply, without kernel buffer assumptions or transfer-sized stress data.
    for trickle in [false, true] {
        let l = limits();
        let mut config = MemFsConfig::default();
        config.add_file("/file", [0x5a; 4096]);
        let server = BoundedListener::bind(
            "127.0.0.1:0",
            MemFs::new(config).unwrap(),
            "/".to_owned(),
            l,
        )
        .await
        .unwrap();
        let shared = Arc::clone(&server.shared);
        let id = shared
            .vfs
            .lookup(&shared.vfs.root_dir(), &b"file".as_slice().into())
            .await
            .unwrap();
        let file = shared.converter.fh_to_nfs(&id);
        let (mut client, io) = tokio::io::duplex(128);
        let connection_slot = Arc::clone(&shared.connections).try_acquire_owned().unwrap();
        let mut task = tokio::spawn(async move {
            let _slot = connection_slot;
            connection_io(io, shared, "controlled".to_owned()).await;
        });
        // Independent RPC header, followed by ordinary READ arguments.
        let mut record = Vec::new();
        for word in [17_u32, 0, 2, PROGRAM, VERSION, 6, 0, 0, 0, 0] {
            record.extend_from_slice(&word.to_be_bytes());
        }
        READ3args {
            file,
            offset: 0,
            count: 4096,
        }
        .pack(&mut record)
        .unwrap();
        client
            .write_u32(0x8000_0000 | u32::try_from(record.len()).unwrap())
            .await
            .unwrap();
        client.write_all(&record).await.unwrap();
        let started = tokio::time::Instant::now();
        let marker = client.read_u32().await.unwrap();
        assert!(marker & 0x7fff_ffff > 4096);
        assert_eq!(server.active_operations(), 1);
        assert!(server.backing_used() > 0);

        // Another real TCP peer progresses while the controlled peer blocks.
        let mut healthy = TcpStream::connect(server.local_addr().unwrap())
            .await
            .unwrap();
        server.accept().await.unwrap();
        let mut null = Vec::new();
        for word in [18_u32, 0, 2, PROGRAM, VERSION, 0, 0, 0, 0, 0] {
            null.extend_from_slice(&word.to_be_bytes());
        }
        healthy
            .write_u32(0x8000_0000 | u32::try_from(null.len()).unwrap())
            .await
            .unwrap();
        healthy.write_all(&null).await.unwrap();
        let len = timeout(Duration::from_secs(1), healthy.read_u32())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(len, 0x8000_0018);
        let mut reply = [0; 24];
        healthy.read_exact(&mut reply).await.unwrap();
        assert_eq!(&reply[..4], &18_u32.to_be_bytes());
        drop(healthy);

        let mut reads = 0;
        timeout(Duration::from_secs(2), async {
            if trickle {
                loop {
                    tokio::select! {
                        result = &mut task => { result.unwrap(); break; }
                        () = tokio::time::sleep(Duration::from_millis(25)) => {
                            let mut bytes = [0; 32];
                            let n = client.read(&mut bytes).await.unwrap();
                            assert!(n > 0);
                            reads += 1;
                        }
                    }
                }
            } else {
                (&mut task).await.unwrap();
            }
        })
        .await
        .unwrap();
        if trickle {
            assert!(
                reads >= 8,
                "writes must keep making progress past the per-write deadline"
            );
            assert!(
                started.elapsed()
                    >= l.send_timeout
                        .checked_sub(Duration::from_millis(100))
                        .unwrap()
            );
        } else {
            assert!(started.elapsed() < l.send_timeout);
        }
        assert_eq!(server.active_operations(), 0);
        assert_eq!(server.backing_used(), 0);
        timeout(Duration::from_secs(1), server.drain())
            .await
            .unwrap();
    }
}
