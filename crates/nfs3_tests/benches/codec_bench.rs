use std::io::Cursor;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nfs3_client::nfs3_types::nfs3::{
    GETATTR3args, GETATTR3res, GETATTR3resok, NFS_PROGRAM, PROGRAM, READ3args,
    READDIRPLUS3res, READDIRPLUS3resok, VERSION, WRITE3args, cookieverf3, dirlistplus3,
    entryplus3, fattr3, filename3, ftype3, nfs_fh3, nfstime3, post_op_attr, post_op_fh3,
    specdata3, stable_how,
};
use nfs3_client::nfs3_types::rpc::{RPC_VERSION_2, call_body, msg_body, opaque_auth, rpc_msg};
use nfs3_client::nfs3_types::xdr_codec::{List, Opaque, Pack, Unpack};

// ---------------------------------------------------------------------------
// Shared test-data constructors
// ---------------------------------------------------------------------------

fn make_fh(size: usize) -> nfs_fh3 {
    nfs_fh3 {
        data: Opaque::owned(vec![0xAB_u8; size]),
    }
}

fn make_fattr3() -> fattr3 {
    fattr3 {
        type_: ftype3::NF3REG,
        mode: 0o644,
        nlink: 1,
        uid: 1000,
        gid: 1000,
        size: 4096,
        used: 4096,
        rdev: specdata3 {
            specdata1: 0,
            specdata2: 0,
        },
        fsid: 1,
        fileid: 42,
        atime: nfstime3 {
            seconds: 1_700_000_000,
            nseconds: 0,
        },
        mtime: nfstime3 {
            seconds: 1_700_000_000,
            nseconds: 0,
        },
        ctime: nfstime3 {
            seconds: 1_700_000_000,
            nseconds: 0,
        },
    }
}

/// Build a typical RPC call header (40 bytes on the wire with AUTH_NULL creds).
fn make_rpc_call(proc: u32) -> rpc_msg<'static, 'static> {
    rpc_msg {
        xid: 0x1234_5678,
        body: msg_body::CALL(call_body {
            rpcvers: RPC_VERSION_2,
            prog: PROGRAM,
            vers: VERSION,
            proc,
            cred: opaque_auth::default(),
            verf: opaque_auth::default(),
        }),
    }
}

fn make_readdirplus_res(n: usize) -> READDIRPLUS3res<'static> {
    let entries: Vec<entryplus3<'static>> = (0..n as u64)
        .map(|i| entryplus3 {
            fileid: i + 100,
            name: filename3::from(format!("file_{i:05}.txt").into_bytes()),
            cookie: i,
            name_attributes: post_op_attr::Some(make_fattr3()),
            name_handle: post_op_fh3::Some(make_fh(32)),
        })
        .collect();
    nfs3_client::nfs3_types::nfs3::Nfs3Result::Ok(READDIRPLUS3resok {
        dir_attributes: post_op_attr::None,
        cookieverf: cookieverf3::default(),
        reply: dirlistplus3 {
            entries: List(entries),
            eof: true,
        },
    })
}

// ---------------------------------------------------------------------------
// RPC call header – pack  (40 bytes, precedes every NFS request)
// ---------------------------------------------------------------------------

fn bench_pack_rpc_call_header(c: &mut Criterion) {
    let msg = make_rpc_call(NFS_PROGRAM::NFSPROC3_GETATTR as u32);
    let mut buf = Vec::with_capacity(msg.packed_size());

    c.bench_function("pack_rpc_call_header", |b| {
        b.iter(|| {
            buf.clear();
            criterion::black_box(&msg).pack(&mut buf).unwrap();
        });
    });
}

// ---------------------------------------------------------------------------
// RPC call header – unpack  (server's recv hot path, decodes every request)
// ---------------------------------------------------------------------------

fn bench_unpack_rpc_call_header(c: &mut Criterion) {
    let msg = make_rpc_call(NFS_PROGRAM::NFSPROC3_GETATTR as u32);
    let mut raw = Vec::with_capacity(msg.packed_size());
    msg.pack(&mut raw).unwrap();

    c.bench_function("unpack_rpc_call_header", |b| {
        b.iter(|| {
            let mut cur = Cursor::new(criterion::black_box(raw.as_slice()));
            let _ = rpc_msg::unpack(&mut cur).unwrap();
        });
    });
}

// ---------------------------------------------------------------------------
// READ3args – pack  (client encodes a READ request)
// ---------------------------------------------------------------------------

fn bench_pack_read3args(c: &mut Criterion) {
    let args = READ3args {
        file: make_fh(32),
        offset: 0,
        count: 4096,
    };
    let mut buf = Vec::with_capacity(args.packed_size());

    c.bench_function("pack_read3args", |b| {
        b.iter(|| {
            buf.clear();
            criterion::black_box(&args).pack(&mut buf).unwrap();
        });
    });
}

// ---------------------------------------------------------------------------
// READ3args – unpack  (server decodes an incoming READ request)
// ---------------------------------------------------------------------------

fn bench_unpack_read3args(c: &mut Criterion) {
    let args = READ3args {
        file: make_fh(32),
        offset: 0,
        count: 4096,
    };
    let mut raw = Vec::with_capacity(args.packed_size());
    args.pack(&mut raw).unwrap();

    c.bench_function("unpack_read3args", |b| {
        b.iter(|| {
            let mut cur = Cursor::new(criterion::black_box(raw.as_slice()));
            let _ = READ3args::unpack(&mut cur).unwrap();
        });
    });
}

// ---------------------------------------------------------------------------
// WRITE3args – pack at different payload sizes
//
// Measures XDR encode + data copy for the three sizes most relevant to
// production workloads.  Uses borrowed data to isolate the codec cost from
// the allocation cost of the payload buffer.
// ---------------------------------------------------------------------------

fn bench_pack_write3args(c: &mut Criterion) {
    let mut group = c.benchmark_group("pack_write3args");

    for size in [1024_usize, 4096, 65536] {
        let data = vec![0u8; size];
        let args = WRITE3args {
            file: make_fh(32),
            offset: 0,
            count: size as u32,
            stable: stable_how::UNSTABLE,
            data: Opaque::borrowed(&data),
        };
        let mut buf = Vec::with_capacity(args.packed_size());

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                buf.clear();
                criterion::black_box(&args).pack(&mut buf).unwrap();
            });
        });
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// WRITE3args – unpack at different payload sizes
//
// Measures XDR decode (includes allocation for the owned data copy).
// This is the server's hot path when receiving WRITE calls.
// ---------------------------------------------------------------------------

fn bench_unpack_write3args(c: &mut Criterion) {
    let mut group = c.benchmark_group("unpack_write3args");

    for size in [1024_usize, 4096, 65536] {
        let data = vec![0u8; size];
        let args = WRITE3args {
            file: make_fh(32),
            offset: 0,
            count: size as u32,
            stable: stable_how::UNSTABLE,
            data: Opaque::borrowed(&data),
        };
        let mut raw = Vec::with_capacity(args.packed_size());
        args.pack(&mut raw).unwrap();

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                let mut cur = Cursor::new(criterion::black_box(raw.as_slice()));
                // WRITE3args<'static> is the only Unpack impl (Opaque::owned)
                let (_, _) = <WRITE3args<'static>>::unpack(&mut cur).unwrap();
            });
        });
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// GETATTR3res – pack  (most frequent server reply, carries fattr3 = 84 bytes)
// ---------------------------------------------------------------------------

fn bench_pack_getattr3res(c: &mut Criterion) {
    let args = GETATTR3args {
        object: make_fh(32),
    };
    let res: GETATTR3res =
        nfs3_client::nfs3_types::nfs3::Nfs3Result::Ok(GETATTR3resok {
            obj_attributes: make_fattr3(),
        });

    // Pack the outgoing request too, so we can measure encode on both sides.
    let mut req_buf = Vec::with_capacity(args.packed_size());
    let mut res_buf = Vec::with_capacity(res.packed_size());

    let mut group = c.benchmark_group("pack_getattr");
    group.bench_function("args", |b| {
        b.iter(|| {
            req_buf.clear();
            criterion::black_box(&args).pack(&mut req_buf).unwrap();
        });
    });
    group.bench_function("res", |b| {
        b.iter(|| {
            res_buf.clear();
            criterion::black_box(&res).pack(&mut res_buf).unwrap();
        });
    });
    group.finish();
}

// ---------------------------------------------------------------------------
// GETATTR3res – unpack  (client decodes a GETATTR reply)
// ---------------------------------------------------------------------------

fn bench_unpack_getattr3res(c: &mut Criterion) {
    let res: GETATTR3res =
        nfs3_client::nfs3_types::nfs3::Nfs3Result::Ok(GETATTR3resok {
            obj_attributes: make_fattr3(),
        });
    let mut raw = Vec::with_capacity(res.packed_size());
    res.pack(&mut raw).unwrap();

    c.bench_function("unpack_getattr3res", |b| {
        b.iter(|| {
            let mut cur = Cursor::new(criterion::black_box(raw.as_slice()));
            let _ = GETATTR3res::unpack(&mut cur).unwrap();
        });
    });
}

// ---------------------------------------------------------------------------
// READDIRPLUS3res – pack at different directory sizes
//
// Serialising a large directory listing is one of the heaviest codec
// operations in the server.  Each entry includes a full fattr3 (84 bytes)
// and a file handle.  Use 10 / 100 / 1000 entries to capture the scaling
// behaviour.
// ---------------------------------------------------------------------------

fn bench_pack_readdirplus_res(c: &mut Criterion) {
    let mut group = c.benchmark_group("pack_readdirplus_res");

    for n in [10_usize, 100, 1000] {
        let res = make_readdirplus_res(n);
        let cap = res.packed_size();

        // Approximate on-wire bytes for throughput labelling
        group.throughput(Throughput::Bytes(cap as u64));

        let mut buf = Vec::with_capacity(cap);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                buf.clear();
                criterion::black_box(&res).pack(&mut buf).unwrap();
            });
        });
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// READDIRPLUS3res – unpack at different directory sizes
//
// Deserialising a listing allocates owned strings and file handles for every
// entry.  Useful for comparing against the pack benchmark to understand the
// allocation overhead.
// ---------------------------------------------------------------------------

fn bench_unpack_readdirplus_res(c: &mut Criterion) {
    let mut group = c.benchmark_group("unpack_readdirplus_res");

    for n in [10_usize, 100, 1000] {
        let res = make_readdirplus_res(n);
        let mut raw = Vec::with_capacity(res.packed_size());
        res.pack(&mut raw).unwrap();

        group.throughput(Throughput::Bytes(raw.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                let mut cur = Cursor::new(criterion::black_box(raw.as_slice()));
                let _ = READDIRPLUS3res::unpack(&mut cur).unwrap();
            });
        });
    }
    group.finish();
}

criterion_group!(
    codec_benches,
    bench_pack_rpc_call_header,
    bench_unpack_rpc_call_header,
    bench_pack_read3args,
    bench_unpack_read3args,
    bench_pack_write3args,
    bench_unpack_write3args,
    bench_pack_getattr3res,
    bench_unpack_getattr3res,
    bench_pack_readdirplus_res,
    bench_unpack_readdirplus_res,
);
criterion_main!(codec_benches);
