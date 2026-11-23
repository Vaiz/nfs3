//! Profile with `cargo bench -p nfs3_tests --bench opaque_xdr -- --gungraun --allocations`.

use bytes::{BufMut, Bytes};
use criterion::{BatchSize, Criterion};
use nfs3_client::nfs3_types::rpc::RecordBuf;
use nfs3_client::nfs3_types::xdr_codec::{
    BoundedOpaque, Decoder, Encoded, Encoder, Opaque, Pack, Unpack,
};

const GROUP: &str = "opaque_xdr";
const BULK: usize = 64 * 1024;
const SMALL_WIRE: &[u8] = b"\0\0\0\x05name1\0\0\0";
const HANDLE_WIRE: &[u8] = b"\0\0\0\x100123456789abcdef";
const INLINE_WIRE: &[u8] = b"\0\0\0\x160123456789abcdefghijkl\0\0";
const HEAP_WIRE: &[u8] = b"\0\0\0\x170123456789abcdefghijklm\0";

fn pack(value: &Opaque<'_>) -> Encoded {
    let mut encoder = Encoder::new(value.packed_size());
    value
        .pack(&mut encoder)
        .expect("benchmark value fits in XDR");
    encoder
        .finish_record()
        .expect("benchmark value fits in an RPC record")
}

#[metabench::benchmark(PACK_BORROWED, GROUP, "pack_borrowed")]
#[bench::empty(b"")]
#[bench::padded(b"name1")]
#[bench::handle(b"0123456789abcdef")]
fn pack_borrowed(data: &'static [u8]) -> Encoded {
    pack(&Opaque::borrowed(data))
}

fn owned_bytes(len: usize) -> Bytes {
    Bytes::from(vec![0xAB; len].into_boxed_slice())
}

#[metabench::benchmark(PACK_SHARED, GROUP, "pack_shared", gungraun_setup = owned_bytes)]
#[bench::small(32)]
#[bench::bulk(BULK)]
#[bench::bulk_padded(BULK + 1)]
fn pack_shared(data: Bytes) -> (Opaque<'static>, Encoded) {
    let value = Opaque::from_bytes(data);
    let encoded = pack(&value);
    (value, encoded)
}

fn copying(data: &'static [u8]) -> Decoder {
    Decoder::copying(data)
}

#[metabench::benchmark(UNPACK_BOUNDED, GROUP, "unpack_bounded", gungraun_setup = copying)]
#[bench::handle(HANDLE_WIRE)]
fn unpack_bounded(mut decoder: Decoder) -> BoundedOpaque<'static, 64> {
    BoundedOpaque::unpack(&mut decoder).expect("valid XDR")
}

#[metabench::benchmark(UNPACK, GROUP, "unpack", gungraun_setup = copying)]
#[bench::small(SMALL_WIRE)]
#[bench::inline_edge(INLINE_WIRE)]
#[bench::heap_edge(HEAP_WIRE)]
fn unpack(mut decoder: Decoder) -> Opaque<'static> {
    Opaque::unpack(&mut decoder).expect("valid XDR")
}

fn sharing(len: usize) -> Decoder {
    let padded = len.next_multiple_of(4);
    let record_len = u32::try_from(4 + padded).expect("benchmark value fits on the wire");
    let mut record = RecordBuf::new(4 + padded);
    record
        .start_fragment((record_len | 0x8000_0000).to_be_bytes())
        .expect("valid record marker");
    let mut buffer = record.buf_mut().expect("record has space for payload");
    buffer.put_u32(u32::try_from(len).expect("benchmark value fits on the wire"));
    buffer.put_bytes(0xAB, len);
    buffer.put_bytes(0, padded - len);
    record.into_decoder()
}

#[metabench::benchmark(UNPACK_SHARED, GROUP, "unpack_shared", gungraun_setup = sharing)]
#[bench::bulk(BULK)]
#[bench::bulk_padded(BULK + 1)]
fn unpack_shared(decoder: Decoder) -> Opaque<'static> {
    unpack(decoder)
}

fn criterion_benchmarks(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group(GROUP);
    for (case, data) in [
        ("empty", &b""[..]),
        ("padded", &b"name1"[..]),
        ("handle", &b"0123456789abcdef"[..]),
    ] {
        group.bench_function(format!("pack_borrowed/{case}"), |b| {
            b.iter(|| pack_borrowed(std::hint::black_box(data)));
        });
    }
    for (case, len) in [("small", 32), ("bulk", BULK), ("bulk_padded", BULK + 1)] {
        group.bench_function(format!("pack_shared/{case}"), |b| {
            b.iter_batched(|| owned_bytes(len), pack_shared, BatchSize::PerIteration);
        });
    }
    group.bench_function("unpack_bounded/handle", |b| {
        b.iter_batched(
            || copying(HANDLE_WIRE),
            unpack_bounded,
            BatchSize::SmallInput,
        );
    });
    for (case, wire) in [
        ("small", SMALL_WIRE),
        ("inline_edge", INLINE_WIRE),
        ("heap_edge", HEAP_WIRE),
    ] {
        group.bench_function(format!("unpack/{case}"), |b| {
            b.iter_batched(|| copying(wire), unpack, BatchSize::SmallInput);
        });
    }
    for (case, len) in [("bulk", BULK), ("bulk_padded", BULK + 1)] {
        group.bench_function(format!("unpack_shared/{case}"), |b| {
            b.iter_batched(|| sharing(len), unpack_shared, BatchSize::PerIteration);
        });
    }
    group.finish();
}

metabench::main!(
    criterion = criterion_benchmarks,
    benchmarks = [
        PACK_BORROWED,
        PACK_SHARED,
        UNPACK_BOUNDED,
        UNPACK,
        UNPACK_SHARED
    ],
);
