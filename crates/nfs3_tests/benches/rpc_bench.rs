use criterion::{Criterion, criterion_group, criterion_main};
use nfs3_server::memfs::MemFsConfig;
use nfs3_tests::{JustClientExt, TestContext};
use tokio::runtime::Runtime;

// ---------------------------------------------------------------------------
// bench_read_throughput – single 4 MiB file, repeated NFSPROC3_READ calls
// ---------------------------------------------------------------------------

fn bench_read_throughput(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    let data = vec![0u8; 4 * 1024 * 1024];
    let mut config = MemFsConfig::default();
    config.add_file("/large.bin", &*data);

    let (mut ctx, fh) = rt.block_on(async {
        let mut ctx = TestContext::setup_with_config(config, false, tracing::Level::WARN);
        let root = ctx.root_dir().clone();
        let fh = ctx.just_lookup(&root, "large.bin").await.unwrap();
        (ctx, fh)
    });

    c.bench_function("read_4mib", |b| {
        b.iter(|| {
            rt.block_on(async {
                let _ = ctx.just_read(&fh).await.unwrap();
            });
        });
    });
}

// ---------------------------------------------------------------------------
// bench_readdirplus_10k – directory with 10 000 entries
// ---------------------------------------------------------------------------

fn bench_readdirplus_10k(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    let mut config = MemFsConfig::default();
    config.add_dir("/bigdir");
    for i in 0..10_000_u32 {
        config.add_file(&format!("/bigdir/file_{i:05}.txt"), b"x");
    }

    let (mut ctx, dir_fh) = rt.block_on(async {
        let mut ctx = TestContext::setup_with_config(config, false, tracing::Level::WARN);
        let root = ctx.root_dir().clone();
        let dir_fh = ctx.just_lookup(&root, "bigdir").await.unwrap();
        (ctx, dir_fh)
    });

    c.bench_function("readdirplus_10k", |b| {
        b.iter(|| {
            rt.block_on(async {
                let _ = ctx.just_readdir(&dir_fh).await.unwrap();
            });
        });
    });
}

// ---------------------------------------------------------------------------
// bench_write_throughput – stream of 1 MiB writes to a single file
// ---------------------------------------------------------------------------

fn bench_write_throughput(c: &mut Criterion) {
    use nfs3_client::nfs3_types::nfs3::{WRITE3args, stable_how};
    use nfs3_client::nfs3_types::xdr_codec::Opaque;
    use nfs3_tests::JustClient;

    let rt = Runtime::new().unwrap();

    let chunk = vec![0u8; 1024 * 1024]; // 1 MiB per write
    let iterations = 4u64;

    let (mut ctx, fh) = rt.block_on(async {
        let config = MemFsConfig::default();
        let mut ctx = TestContext::setup_with_config(config, false, tracing::Level::WARN);
        let root = ctx.root_dir().clone();
        let fh = ctx.just_create(&root, "sink.bin", b"").await.unwrap();
        (ctx, fh)
    });

    c.bench_function("write_4mib", |b| {
        b.iter(|| {
            rt.block_on(async {
                for i in 0..iterations {
                    let offset = i * chunk.len() as u64;
                    let _result = ctx
                        .client()
                        .write(&WRITE3args {
                            file: fh.clone(),
                            offset,
                            count: chunk.len() as u32,
                            stable: stable_how::UNSTABLE,
                            data: Opaque::borrowed(&chunk),
                        })
                        .await
                        .unwrap()
                        .unwrap();
                }
            });
        });
    });
}

// ---------------------------------------------------------------------------
// bench_rpc_null_roundtrip – minimum RPC framing overhead
//
// Sends NFSPROC3_NULL and receives an empty reply.  No VFS work is done;
// this measures the cost of RPC framing, fragment header parsing, XDR
// encode/decode of the call/reply headers, and the in-process duplex I/O.
// ---------------------------------------------------------------------------

fn bench_rpc_null_roundtrip(c: &mut Criterion) {
    use nfs3_client::nfs3_types::nfs3::{NFS_PROGRAM, PROGRAM, VERSION};
    use nfs3_client::nfs3_types::rpc::{RPC_VERSION_2, call_body, msg_body, opaque_auth, rpc_msg};
    use nfs3_client::nfs3_types::xdr_codec::Void;
    use nfs3_tests::RpcTestContext;

    let rt = Runtime::new().unwrap();
    let mut ctx = rt.block_on(async {
        RpcTestContext::setup_with_config(MemFsConfig::default(), tracing::Level::WARN)
    });

    // Each iteration must use a unique XID to avoid the server's retransmission
    // detector treating repeated calls as duplicates (which would silently drop them).
    let mut xid: u32 = 0;

    c.bench_function("rpc_null_roundtrip", |b| {
        b.iter(|| {
            xid = xid.wrapping_add(1);
            let call = rpc_msg {
                xid,
                body: msg_body::CALL(call_body {
                    rpcvers: RPC_VERSION_2,
                    prog: PROGRAM,
                    vers: VERSION,
                    proc: NFS_PROGRAM::NFSPROC3_NULL as u32,
                    cred: opaque_auth::default(),
                    verf: opaque_auth::default(),
                }),
            };
            rt.block_on(async {
                ctx.send_call(&call, &Void).await.unwrap();
                let (_, _) = ctx.recv_reply::<Void>().await.unwrap();
            });
        });
    });
}

// ---------------------------------------------------------------------------
// bench_rpc_getattr_roundtrip – framing + codec cost for a small NFS reply
//
// Issues a NFSPROC3_GETATTR for the root directory.  The server looks up
// file attributes in MemFs and returns an fattr3 struct (84 bytes).  This
// benchmark measures the full path: XDR encode → I/O → VFS lookup →
// XDR encode of reply → I/O → XDR decode.
// ---------------------------------------------------------------------------

fn bench_rpc_getattr_roundtrip(c: &mut Criterion) {
    use nfs3_client::nfs3_types::nfs3::{GETATTR3args, GETATTR3res, NFS_PROGRAM, PROGRAM, VERSION};
    use nfs3_client::nfs3_types::rpc::{RPC_VERSION_2, call_body, msg_body, opaque_auth, rpc_msg};
    use nfs3_tests::RpcTestContext;

    let rt = Runtime::new().unwrap();
    let mut ctx = rt.block_on(async {
        RpcTestContext::setup_with_config(MemFsConfig::default(), tracing::Level::WARN)
    });

    let root = ctx.root_dir().clone();
    let getattr_args = GETATTR3args { object: root };
    let mut xid: u32 = 0;

    c.bench_function("rpc_getattr_roundtrip", |b| {
        b.iter(|| {
            xid = xid.wrapping_add(1);
            let call = rpc_msg {
                xid,
                body: msg_body::CALL(call_body {
                    rpcvers: RPC_VERSION_2,
                    prog: PROGRAM,
                    vers: VERSION,
                    proc: NFS_PROGRAM::NFSPROC3_GETATTR as u32,
                    cred: opaque_auth::default(),
                    verf: opaque_auth::default(),
                }),
            };
            rt.block_on(async {
                ctx.send_call(&call, &getattr_args).await.unwrap();
                let (_, _) = ctx.recv_reply::<GETATTR3res>().await.unwrap();
            });
        });
    });
}

criterion_group!(
    benches,
    bench_read_throughput,
    bench_readdirplus_10k,
    bench_write_throughput,
    bench_rpc_null_roundtrip,
    bench_rpc_getattr_roundtrip,
);
criterion_main!(benches);
