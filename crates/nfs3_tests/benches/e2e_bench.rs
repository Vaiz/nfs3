use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nfs3_client::nfs3_types::nfs3::{
    GETATTR3args, GETATTR3res, NFS_PROGRAM, PROGRAM, READ3args, REMOVE3args, WRITE3args,
    VERSION, diropargs3, nfs_fh3, stable_how,
};
use nfs3_client::nfs3_types::rpc::{RPC_VERSION_2, call_body, msg_body, opaque_auth, rpc_msg};
use nfs3_client::nfs3_types::xdr_codec::{Opaque, Void};
use nfs3_server::memfs::MemFsConfig;
use nfs3_tests::{JustClient, JustClientExt, RpcTestContext, TestContext};
use tokio::runtime::Runtime;

// ---------------------------------------------------------------------------
// bench_pipelined_requests
//
// Sends PIPELINE_DEPTH requests without waiting for any reply, then collects
// all PIPELINE_DEPTH replies.  Exercises the server's concurrent task
// dispatcher and the duplex I/O buffer together.
//
// Two variants:
//   - null:    no VFS work, pure framing cost
//   - getattr: involves a MemFs lookup per request
//
// The sequential `rpc_null_roundtrip` / `rpc_getattr_roundtrip` benchmarks in
// rpc_bench.rs act as the baseline.  Dividing time-per-iteration by
// PIPELINE_DEPTH gives the effective per-request latency under pipelining.
// ---------------------------------------------------------------------------

const PIPELINE_DEPTH: usize = 16;

fn make_call(xid: u32, proc: NFS_PROGRAM) -> rpc_msg<'static, 'static> {
    rpc_msg {
        xid,
        body: msg_body::CALL(call_body {
            rpcvers: RPC_VERSION_2,
            prog: PROGRAM,
            vers: VERSION,
            proc: proc as u32,
            cred: opaque_auth::default(),
            verf: opaque_auth::default(),
        }),
    }
}

fn bench_pipelined_null(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut ctx = rt.block_on(async {
        RpcTestContext::setup_with_config(MemFsConfig::default(), tracing::Level::WARN)
    });

    let mut base_xid: u32 = 0;

    c.bench_function("pipelined_null_x16", |b| {
        b.iter(|| {
            base_xid = base_xid.wrapping_add(PIPELINE_DEPTH as u32);
            rt.block_on(async {
                // Send PIPELINE_DEPTH calls without waiting for replies
                for i in 0..PIPELINE_DEPTH {
                    let call = make_call(base_xid.wrapping_add(i as u32), NFS_PROGRAM::NFSPROC3_NULL);
                    ctx.send_call(&call, &Void).await.unwrap();
                }
                // Collect all replies (server may return them in any order)
                for _ in 0..PIPELINE_DEPTH {
                    ctx.recv_reply::<Void>().await.unwrap();
                }
            });
        });
    });
}

fn bench_pipelined_getattr(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut ctx = rt.block_on(async {
        RpcTestContext::setup_with_config(MemFsConfig::default(), tracing::Level::WARN)
    });

    let root = ctx.root_dir().clone();
    let args = GETATTR3args {
        object: root.clone(),
    };
    let mut base_xid: u32 = 0;

    c.bench_function("pipelined_getattr_x16", |b| {
        b.iter(|| {
            base_xid = base_xid.wrapping_add(PIPELINE_DEPTH as u32);
            rt.block_on(async {
                for i in 0..PIPELINE_DEPTH {
                    let call =
                        make_call(base_xid.wrapping_add(i as u32), NFS_PROGRAM::NFSPROC3_GETATTR);
                    ctx.send_call(&call, &args).await.unwrap();
                }
                for _ in 0..PIPELINE_DEPTH {
                    ctx.recv_reply::<GETATTR3res>().await.unwrap();
                }
            });
        });
    });
}

// ---------------------------------------------------------------------------
// bench_small_reads_sequential
//
// Reads a 1 MiB pre-created file as 256 × 4 KiB READ3 calls via TestContext.
// Exposes the per-request overhead (RPC framing, XDR encode/decode, VFS path
// resolution for each call) that a single bulk-read benchmark hides.
//
// Compare to `read_4mib` in rpc_bench.rs which reads the same data in a
// single large call; the difference is the request-dispatch overhead × 256.
// ---------------------------------------------------------------------------

const CHUNK_SIZE: u64 = 4096;
const FILE_SIZE: usize = 1024 * 1024; // 1 MiB
const CHUNKS: u64 = (FILE_SIZE as u64) / CHUNK_SIZE;

fn bench_small_reads_sequential(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    let data = vec![0xAA_u8; FILE_SIZE];
    let mut config = MemFsConfig::default();
    config.add_file("/4k_reads.bin", &*data);

    let (mut ctx, fh) = rt.block_on(async {
        let mut ctx = TestContext::setup_with_config(config, false, tracing::Level::WARN);
        let root = ctx.root_dir().clone();
        let fh = ctx.just_lookup(&root, "4k_reads.bin").await.unwrap();
        (ctx, fh)
    });

    c.bench_function("small_reads_256x4k", |b| {
        b.iter(|| {
            rt.block_on(async {
                for chunk in 0..CHUNKS {
                    let _ = ctx
                        .client()
                        .read(&READ3args {
                            file: fh.clone(),
                            offset: chunk * CHUNK_SIZE,
                            count: CHUNK_SIZE as u32,
                        })
                        .await
                        .unwrap();
                }
            });
        });
    });
}

// ---------------------------------------------------------------------------
// bench_write_read_cycle
//
// Per iteration: write N bytes to offset 0 of a pre-created file, then read
// them back.  Measures end-to-end symmetric write+read latency for three
// payload sizes that represent typical NFS I/O (64 KiB, 256 KiB, 1 MiB).
//
// Each iteration reuses the same file and overwrites offset 0 so there is no
// state accumulation across iterations.
// ---------------------------------------------------------------------------

fn bench_write_read_cycle(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("write_read_cycle");

    for size in [64 * 1024_usize, 256 * 1024, 1024 * 1024] {
        let payload = vec![0xBB_u8; size];

        let (mut ctx, fh): (TestContext<_>, nfs_fh3) = rt.block_on(async {
            let config = MemFsConfig::default();
            let mut ctx = TestContext::setup_with_config(config, false, tracing::Level::WARN);
            let root = ctx.root_dir().clone();
            let fh = ctx
                .just_create(&root, "cycle.bin", &payload)
                .await
                .unwrap();
            (ctx, fh)
        });

        group.throughput(Throughput::Bytes(size as u64 * 2)); // write + read
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &size,
            |b, &sz| {
                b.iter(|| {
                    rt.block_on(async {
                        // Write
                        ctx.client()
                            .write(&WRITE3args {
                                file: fh.clone(),
                                offset: 0,
                                count: sz as u32,
                                stable: stable_how::UNSTABLE,
                                data: Opaque::borrowed(&payload),
                            })
                            .await
                            .unwrap();
                        // Read back
                        let mut offset = 0u64;
                        let mut remaining = sz as u64;
                        while remaining > 0 {
                            let count = remaining.min(1024 * 1024) as u32;
                            let res = ctx
                                .client()
                                .read(&READ3args {
                                    file: fh.clone(),
                                    offset,
                                    count,
                                })
                                .await
                                .unwrap();
                            use nfs3_client::nfs3_types::nfs3::Nfs3Result;
                            match res {
                                Nfs3Result::Ok(ok) => {
                                    let got = ok.count as u64;
                                    offset += got;
                                    remaining = remaining.saturating_sub(got);
                                    if ok.eof {
                                        break;
                                    }
                                }
                                Nfs3Result::Err((stat, _)) => {
                                    panic!("read failed: {stat:?}");
                                }
                            }
                        }
                    });
                });
            },
        );
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// bench_file_lifecycle
//
// Per iteration: create a uniquely-named file, write a small payload, read
// the payload back, then remove the file.  This exercises every layer of the
// stack — RPC framing, XDR codec, VFS create/write/read/unlink — together.
//
// "Lifecycle" covers the complete server state transitions: inode allocation,
// data write, data read, inode free.  Comparison against write_read_cycle
// isolates the overhead of creation and deletion.
// ---------------------------------------------------------------------------

fn bench_file_lifecycle(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    const PAYLOAD: &[u8] = &[0xCC_u8; 4096]; // 4 KiB — small enough to keep timing tight

    let (mut ctx, root) = rt.block_on(async {
        let config = MemFsConfig::default();
        let ctx = TestContext::setup_with_config(config, false, tracing::Level::WARN);
        let root = ctx.root_dir().clone();
        (ctx, root)
    });

    let mut counter: u64 = 0;

    c.bench_function("file_lifecycle_4k", |b| {
        b.iter(|| {
            counter = counter.wrapping_add(1);
            let name = format!("bench_{counter}");
            rt.block_on(async {
                // Create + write
                let fh = ctx
                    .just_create(&root, &name, PAYLOAD)
                    .await
                    .unwrap();

                // Read back
                let _ = ctx.just_read(&fh).await.unwrap();

                // Remove
                ctx.client()
                    .remove(&REMOVE3args {
                        object: diropargs3 {
                            dir: root.clone(),
                            name: name.as_bytes().into(),
                        },
                    })
                    .await
                    .unwrap();
            });
        });
    });
}

// ---------------------------------------------------------------------------
// bench_metadata_intensive
//
// Per iteration: perform N sequential lookup + getattr pairs on a set of
// pre-created files.  No data payloads are transferred — this isolates the
// metadata path (VFS stat, RPC framing, XDR encode/decode of fattr3).
//
// Useful for profiling the overhead of the per-request path (transaction
// tracker, RPC context clone, handler dispatch) separate from I/O cost.
// ---------------------------------------------------------------------------

const METADATA_FILES: usize = 20;

fn bench_metadata_intensive(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    let mut config = MemFsConfig::default();
    for i in 0..METADATA_FILES {
        config.add_file(&format!("/meta_{i:03}.txt"), b"x");
    }

    let (mut ctx, root, fhs) = rt.block_on(async {
        let mut ctx = TestContext::setup_with_config(config, false, tracing::Level::WARN);
        let root = ctx.root_dir().clone();
        let mut fhs = Vec::with_capacity(METADATA_FILES);
        for i in 0..METADATA_FILES {
            let name = format!("meta_{i:03}.txt");
            let fh = ctx.just_lookup(&root, &name).await.unwrap();
            fhs.push(fh);
        }
        (ctx, root, fhs)
    });

    c.bench_function("metadata_intensive_20x_lookup_getattr", |b| {
        b.iter(|| {
            rt.block_on(async {
                for fh in &fhs {
                    // getattr — server resolves fh → inode → fattr3
                    let _ = ctx.just_getattr(criterion::black_box(fh)).await.unwrap();
                }
                // Lookup by name from root — exercises name resolution path
                for i in 0..METADATA_FILES {
                    let name = format!("meta_{i:03}.txt");
                    let _ = ctx
                        .just_lookup(criterion::black_box(&root), &name)
                        .await
                        .unwrap();
                }
            });
        });
    });
}

criterion_group!(
    e2e_benches,
    bench_pipelined_null,
    bench_pipelined_getattr,
    bench_small_reads_sequential,
    bench_write_read_cycle,
    bench_file_lifecycle,
    bench_metadata_intensive,
);
criterion_main!(e2e_benches);
