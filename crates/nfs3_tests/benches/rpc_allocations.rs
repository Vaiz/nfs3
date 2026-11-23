//! Measure RPCs with `cargo bench -p nfs3_tests --bench rpc_allocations -- --gungraun
//! --allocations`.

#![expect(
    clippy::unwrap_used,
    reason = "A failed benchmark fixture or RPC invalidates the result"
)]

use std::time::Duration;

use criterion::{BenchmarkId, Criterion};
use nfs3_client::Nfs3Client;
use nfs3_client::nfs3_types::nfs3::{
    LOOKUP3args, READ3args, READDIRPLUS3args, WRITE3args, cookieverf3, diropargs3, stable_how,
};
use nfs3_client::nfs3_types::xdr_codec::Opaque;
use nfs3_client::tokio::TokioIo;
use nfs3_server::memfs::{MemFs, MemFsConfig};
use nfs3_tests::Server;
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::{Builder, Runtime};

const GROUP: &str = "tcp_connection";
const FILE_NAME: &[u8] = b"file";
const SMALL: u32 = 4 * 1024;
const LARGE: u32 = 1024 * 1024;
const DIRECTORY_ENTRIES: usize = 64;

#[derive(Clone, Copy)]
enum Operation {
    Null,
    Lookup,
    Readdirplus,
    Read,
    Write,
}

/// Operation, number of requests, and READ/WRITE payload size.
type Workload = (Operation, usize, u32);

struct Fixture {
    runtime: Runtime,
    server: Server<TcpStream, MemFs>,
    client: Nfs3Client<TokioIo<TcpStream>>,
    workload: Workload,
    // Allocated during setup so the measured run only counts RPC allocations.
    payload: Vec<u8>,
}

fn prepare(workload: Workload) -> Fixture {
    let runtime = Builder::new_current_thread().enable_all().build().unwrap();
    let (server, client) = runtime.block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (client, accepted) = tokio::join!(
            TcpStream::connect(listener.local_addr().unwrap()),
            listener.accept()
        );
        let client = client.unwrap();
        let (server, _) = accepted.unwrap();
        server.set_nodelay(true).unwrap();
        client.set_nodelay(true).unwrap();
        (server, client)
    });
    let payload = vec![0xAB; workload.2 as usize];
    let mut config = MemFsConfig::default();
    if matches!(workload.0, Operation::Readdirplus) {
        for index in 0..DIRECTORY_ENTRIES {
            config.add_file(&format!("/entry_{index:05}"), Vec::new());
        }
    } else {
        config.add_file("/file", payload.clone());
    }
    let server = runtime
        .block_on(Server::new(server, MemFs::new(config).unwrap()))
        .unwrap();
    let client = Nfs3Client::new(TokioIo::new(client));
    Fixture {
        runtime,
        server,
        client,
        workload,
        payload,
    }
}

#[metabench::benchmark(RPC, GROUP, "rpc", gungraun_setup = prepare)]
#[bench::one_null((Operation::Null, 1, SMALL))]
#[bench::hundred_nulls((Operation::Null, 100, SMALL))]
#[bench::hundred_lookups((Operation::Lookup, 100, SMALL))]
#[bench::hundred_readdirplus((Operation::Readdirplus, 100, 0))]
#[bench::hundred_reads((Operation::Read, 100, SMALL))]
#[bench::hundred_writes((Operation::Write, 100, SMALL))]
#[bench::hundred_large_reads((Operation::Read, 100, LARGE))]
#[bench::hundred_large_writes((Operation::Write, 100, LARGE))]
fn measure(fixture: Fixture) -> Runtime {
    let Fixture {
        runtime,
        server,
        client,
        workload,
        payload,
    } = fixture;
    let lookup = LOOKUP3args {
        what: diropargs3 {
            dir: server.root_dir(),
            name: FILE_NAME.into(),
        },
    };
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(5), async {
            let (result, ()) = tokio::join!(
                server.run(),
                run_client(client, &lookup, workload, &payload)
            );
            result.unwrap();
            // Let the base revision's detached parser observe the connection closing.
            tokio::task::yield_now().await;
        })
        .await
        .expect("RPC benchmark timed out");
    });
    runtime
}

async fn run_client(
    mut client: Nfs3Client<TokioIo<TcpStream>>,
    lookup: &LOOKUP3args<'_>,
    (operation, requests, count): Workload,
    payload: &[u8],
) {
    match operation {
        Operation::Null => {
            for _ in 0..requests {
                client.null().await.unwrap();
            }
        }
        Operation::Lookup => {
            for _ in 0..requests {
                client.lookup(lookup).await.unwrap().unwrap();
            }
        }
        Operation::Readdirplus => {
            let args = READDIRPLUS3args {
                dir: lookup.what.dir.clone(),
                cookie: 0,
                cookieverf: cookieverf3::default(),
                dircount: LARGE,
                maxcount: LARGE,
            };
            for _ in 0..requests {
                let reply = client.readdirplus(&args).await.unwrap().unwrap().reply;
                assert!(reply.eof);
                assert_eq!(reply.entries.0.len(), DIRECTORY_ENTRIES);
            }
        }
        Operation::Read => {
            let file = client.lookup(lookup).await.unwrap().unwrap().object;
            let args = READ3args {
                file,
                offset: 0,
                count,
            };
            for _ in 0..requests {
                let reply = client.read(&args).await.unwrap().unwrap();
                assert_eq!(reply.count, count);
                assert_eq!(&*reply.data, payload);
            }
        }
        Operation::Write => {
            let file = client.lookup(lookup).await.unwrap().unwrap().object;
            let args = WRITE3args {
                file,
                offset: 0,
                count,
                stable: stable_how::UNSTABLE,
                data: Opaque::borrowed(payload),
            };
            for _ in 0..requests {
                let reply = client.write(&args).await.unwrap().unwrap();
                assert_eq!(reply.count, count);
            }
        }
    }
}

fn criterion_benchmarks(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group(RPC.group_name());
    for (case, workload) in [
        ("one_null", (Operation::Null, 1, SMALL)),
        ("hundred_nulls", (Operation::Null, 100, SMALL)),
        ("hundred_lookups", (Operation::Lookup, 100, SMALL)),
        ("hundred_readdirplus", (Operation::Readdirplus, 100, 0)),
        ("hundred_reads", (Operation::Read, 100, SMALL)),
        ("hundred_writes", (Operation::Write, 100, SMALL)),
        ("hundred_large_reads", (Operation::Read, 100, LARGE)),
        ("hundred_large_writes", (Operation::Write, 100, LARGE)),
    ] {
        group.bench_with_input(
            BenchmarkId::new(RPC.benchmark_name(), case),
            &workload,
            |b, &workload| {
                b.iter_batched(
                    || prepare(workload),
                    measure,
                    criterion::BatchSize::PerIteration,
                );
            },
        );
    }
    group.finish();
}

metabench::main!(criterion = criterion_benchmarks, benchmarks = [RPC],);
