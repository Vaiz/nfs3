use std::cell::RefCell;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nfs3_client::nfs3_types::nfs3::{
    GETATTR3args, LOOKUP3args, READ3args, READDIRPLUS3args, WRITE3args, cookieverf3, diropargs3,
    nfs_fh3, stable_how,
};
use nfs3_client::nfs3_types::xdr_codec::Opaque;
use nfs3_tests::Server;
use nfs3_tests::perf::{ListFs, ReadFs, WriteFs};
use tokio::io::DuplexStream;
use tokio::runtime::Runtime;

type Client = RefCell<nfs3_client::Nfs3Client<nfs3_client::tokio::TokioIo<DuplexStream>>>;

struct PerfCtx {
    _server_handle: tokio::task::JoinHandle<anyhow::Result<()>>,
    client: Client,
    root_dir: nfs_fh3,
}

fn setup_perf<FS: nfs3_server::vfs::NfsFileSystem + 'static>(fs: FS) -> PerfCtx {
    nfs3_tests::init_logging(tracing::Level::WARN);

    let (server_io, client_io) = tokio::io::duplex(1024 * 1024);
    let server = Server::new(server_io, fs).unwrap();
    let root_dir = server.root_dir();
    let server_handle = tokio::task::spawn(server.run());

    let client_io = nfs3_client::tokio::TokioIo::new(client_io);
    let client = nfs3_client::Nfs3Client::new(client_io);

    PerfCtx {
        _server_handle: server_handle,
        client: RefCell::new(client),
        root_dir,
    }
}

/// Resolve the single file handle through the server's handle converter.
async fn get_file_handle(ctx: &PerfCtx) -> nfs_fh3 {
    ctx.client
        .borrow_mut()
        .lookup(&LOOKUP3args {
            what: diropargs3 {
                dir: ctx.root_dir.clone(),
                name: b"file"[..].into(),
            },
        })
        .await
        .expect("lookup failed")
        .unwrap()
        .object
}

/// GETATTR on root and on a file handle — the most frequently called NFS3 operation.
fn bench_getattr(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let (ctx, fh) = rt.block_on(async {
        let ctx = setup_perf(ReadFs::new(64));
        let fh = get_file_handle(&ctx).await;
        (ctx, fh)
    });

    let mut group = c.benchmark_group("perf_getattr");
    group.bench_function("null_baseline", |b| {
        b.to_async(&rt).iter(|| async {
            ctx.client.borrow_mut().null().await.unwrap();
        });
    });
    group.bench_function("root", |b| {
        b.to_async(&rt).iter(|| async {
            ctx.client
                .borrow_mut()
                .getattr(&GETATTR3args {
                    object: ctx.root_dir.clone(),
                })
                .await
                .unwrap()
                .unwrap();
        });
    });
    group.bench_function("file", |b| {
        b.to_async(&rt).iter(|| async {
            ctx.client
                .borrow_mut()
                .getattr(&GETATTR3args { object: fh.clone() })
                .await
                .unwrap()
                .unwrap();
        });
    });
    group.finish();
}

/// LOOKUP by name from root — exercises name resolution path.
fn bench_lookup(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let ctx = rt.block_on(async { setup_perf(ReadFs::new(64)) });

    c.bench_function("perf_lookup", |b| {
        b.to_async(&rt).iter(|| async {
            ctx.client
                .borrow_mut()
                .lookup(&LOOKUP3args {
                    what: diropargs3 {
                        dir: ctx.root_dir.clone(),
                        name: b"file"[..].into(),
                    },
                })
                .await
                .unwrap()
                .unwrap();
        });
    });
}

/// bench `read` calls throughput for various sizes
fn bench_read(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("perf_read");

    for size in [4 * 1024_usize, 64 * 1024, 256 * 1024, 1024 * 1024] {
        let (ctx, fh) = rt.block_on(async {
            let ctx = setup_perf(ReadFs::new(size));
            let fh = get_file_handle(&ctx).await;
            (ctx, fh)
        });

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &sz| {
            b.to_async(&rt).iter(|| async {
                let ok = ctx
                    .client
                    .borrow_mut()
                    .read(&READ3args {
                        file: fh.clone(),
                        offset: 0,
                        count: sz as u32,
                    })
                    .await
                    .unwrap()
                    .unwrap();

                debug_assert_eq!(ok.count as usize, sz);
            });
        });
    }
    group.finish();
}

/// bench `write` calls throughput for various sizes
fn bench_write(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("perf_write");

    for size in [4 * 1024_usize, 64 * 1024, 256 * 1024, 1024 * 1024] {
        let payload = vec![0xBB_u8; size];

        let (ctx, fh) = rt.block_on(async {
            let ctx = setup_perf(WriteFs::new(size as u64));
            let fh = get_file_handle(&ctx).await;
            (ctx, fh)
        });

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &sz| {
            b.to_async(&rt).iter(|| async {
                let ok = ctx
                    .client
                    .borrow_mut()
                    .write(&WRITE3args {
                        file: fh.clone(),
                        offset: 0,
                        count: sz as u32,
                        stable: stable_how::UNSTABLE,
                        data: Opaque::borrowed(&payload),
                    })
                    .await
                    .unwrap()
                    .unwrap();
                debug_assert_eq!(ok.count as usize, sz);
            });
        });
    }
    group.finish();
}

/// bench `readdirplus` calls throughput for various sizes
fn bench_readdirplus(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("perf_readdirplus");

    for entry_count in [100_usize, 1_000, 10_000] {
        let ctx = rt.block_on(async { setup_perf(ListFs::new(entry_count)) });

        group.bench_with_input(
            BenchmarkId::from_parameter(entry_count),
            &entry_count,
            |b, _| {
                b.to_async(&rt).iter(|| async {
                    let mut cookie: u64 = 0;
                    let mut total = 0usize;
                    loop {
                        let ok = ctx
                            .client
                            .borrow_mut()
                            .readdirplus(&READDIRPLUS3args {
                                dir: ctx.root_dir.clone(),
                                cookie,
                                cookieverf: cookieverf3::default(),
                                dircount: 1024 * 1024,
                                maxcount: 1024 * 1024,
                            })
                            .await
                            .unwrap()
                            .unwrap();

                        let entries = &ok.reply.entries;
                        total += entries.0.len();
                        cookie = entries.0.last().map(|e| e.cookie).unwrap_or_default();
                        if ok.reply.eof {
                            break;
                        }
                    }
                    debug_assert_eq!(total, entry_count);
                });
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_getattr,
    bench_lookup,
    bench_read,
    bench_write,
    bench_readdirplus
);
criterion_main!(benches);
