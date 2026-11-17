# Buffering Research D1-R1

Date: 2026-11-17. Returned by Buffering Docs using its configured model, without
override. Source research only; no NFS measurements or cross-target builds.

## Evidence and Candidate Comparison

### Microsoft http_headers

Pin: [oxidizer 6230675b0c2c8a759e0ef5e9db92d11958f5e55e](https://github.com/microsoft/oxidizer/tree/6230675b0c2c8a759e0ef5e9db92d11958f5e55e/crates/http_headers).

- [field_value.rs](https://github.com/microsoft/oxidizer/blob/6230675b0c2c8a759e0ef5e9db92d11958f5e55e/crates/http_headers/src/field_value.rs): `FieldValueRef` is a borrowed raw-byte view; `FieldValue` owns validated values, stores up to 64 bytes inline, and shares longer `Bytes` or retains a `HeaderValue` owner.
- [FieldSource](https://github.com/microsoft/oxidizer/blob/6230675b0c2c8a759e0ef5e9db92d11958f5e55e/crates/http_headers/src/source/field_source.rs) / `FieldLines` and [FieldSink](https://github.com/microsoft/oxidizer/blob/6230675b0c2c8a759e0ef5e9db92d11958f5e55e/crates/http_headers/src/sink/field_sink.rs) separate access/encoding from storage.
- [Allocation tests](https://github.com/microsoft/oxidizer/blob/6230675b0c2c8a759e0ef5e9db92d11958f5e55e/crates/http_headers/tests/field_value_allocations.rs) and [ownership tests](https://github.com/microsoft/oxidizer/blob/6230675b0c2c8a759e0ef5e9db92d11958f5e55e/crates/http_headers/tests/benchmark_ownership.rs) check allocation and owner retention/release.
- Custom sources cap each field name at 64 KiB, 128 lines, and 1,024 list items; `HeaderMap` is exempt. HTTP validation, repeated-field semantics, static field descriptors, and comma lists do not transfer to binary XDR.
- [PERF.md](https://github.com/microsoft/oxidizer/blob/6230675b0c2c8a759e0ef5e9db92d11958f5e55e/crates/http_headers/docs/PERF.md) explicitly contains historical, not rerun, results after benchmark migration. Example: borrowed Content-Type 42.9 ns/0 allocations versus owned 118.2 ns/0 allocations. These are HTTP results, not an NFS performance claim.
- Manifest has `publish = false`; use as design inspiration, not a dependency.

### bytes

Pin: [bytes 1.12.1, 76c0fbb54ed4336caf9d2311658a2f4a5627c21d](https://github.com/tokio-rs/bytes/tree/76c0fbb54ed4336caf9d2311658a2f4a5627c21d).

- [bytes.rs](https://github.com/tokio-rs/bytes/blob/76c0fbb54ed4336caf9d2311658a2f4a5627c21d/src/bytes.rs): contiguous cheaply cloned views; slicing and splitting can share storage. A tiny slice can retain the complete allocation.
- [Buf implementation](https://github.com/tokio-rs/bytes/blob/76c0fbb54ed4336caf9d2311658a2f4a5627c21d/src/buf/buf_impl.rs): `chunk()` exposes only a contiguous prefix; cross-chunk scalar access can use a small temporary; `copy_to_slice` copies. A generic `Buf` is not a contiguous slice.
- `BytesMut::freeze` can share without copying. `reserve` may allocate, shift, or copy; `resize` fills bytes. `try_reclaim` is not fallible reserve.
- [Chain tests](https://github.com/tokio-rs/bytes/blob/76c0fbb54ed4336caf9d2311658a2f4a5627c21d/tests/test_chain.rs) and [Limit tests](https://github.com/tokio-rs/bytes/blob/76c0fbb54ed4336caf9d2311658a2f4a5627c21d/tests/test_limit.rs) cover segmented operations and bounded writes, not receive memory budgets.
- [Bytes benchmarks](https://github.com/tokio-rs/bytes/blob/76c0fbb54ed4336caf9d2311658a2f4a5627c21d/benches/bytes.rs) and [BytesMut benchmarks](https://github.com/tokio-rs/bytes/blob/76c0fbb54ed4336caf9d2311658a2f4a5627c21d/benches/bytes_mut.rs) cover clone/slice/growth, not NFS.
- Selected independently for maintained releases, broad adoption (Docs reported 207M release downloads), focused tests, and runtime-neutral ownership primitives. Download counts are time-sensitive selection context, not correctness evidence.

### tokio-util

Pin: [0.7.20, 5ec9e6b8d5a7111fe9cd35a637bd8b4e4eb1e483](https://github.com/tokio-rs/tokio/tree/5ec9e6b8d5a7111fe9cd35a637bd8b4e4eb1e483/tokio-util).

- [LengthDelimitedCodec](https://github.com/tokio-rs/tokio/blob/5ec9e6b8d5a7111fe9cd35a637bd8b4e4eb1e483/tokio-util/src/codec/length_delimited.rs) checks max length before reserving; default max is 8 MiB. It assembles contiguous frames, not multi-fragment RPC records.
- [FramedRead](https://github.com/tokio-rs/tokio/blob/5ec9e6b8d5a7111fe9cd35a637bd8b4e4eb1e483/tokio-util/src/codec/framed_read.rs) and [framing tests](https://github.com/tokio-rs/tokio/blob/5ec9e6b8d5a7111fe9cd35a637bd8b4e4eb1e483/tokio-util/tests/length_delimited.rs) cover pending reads, multi-packet frames, incomplete data, and length rejection.
- Independently selected for actively maintained framing and concrete async/error tests; Docs reported 963M total downloads, not a performance argument.
- MSRV 1.85 fits, but 0.7.20 requires Tokio 1.51 versus workspace lock 1.49. The workspace range allows updating, CI `--locked` does not. 0.7.19 requires 1.47. Tokio-only integration does not cover smol unchanged.

### bytesbuf

Pin: [0.5.0, oxidizer 074a147ab947fb44aa468805c59f6793b4fd34a6](https://github.com/microsoft/oxidizer/tree/074a147ab947fb44aa468805c59f6793b4fd34a6/crates/bytesbuf).

- [BytesView](https://github.com/microsoft/oxidizer/blob/074a147ab947fb44aa468805c59f6793b4fd34a6/crates/bytesbuf/src/view.rs) is segmented immutable storage. Clone/range/append share spans; `first_slice` and `slices` expose segments. Tests include cross-span scalar access, multi-span ranges, Send/Sync, and cross-thread release.
- [Read/BufRead adapter](https://github.com/microsoft/oxidizer/blob/074a147ab947fb44aa468805c59f6793b4fd34a6/crates/bytesbuf/src/view_read.rs) copies to caller storage; `fill_buf` exposes only the first segment.
- [GlobalPool](https://github.com/microsoft/oxidizer/blob/074a147ab947fb44aa468805c59f6793b4fd34a6/crates/bytesbuf/src/mem/global.rs) pools uninitialized 1, 4, 16, 64 KiB blocks. 70 KiB rounds to two 64 KiB blocks. Reserve is infallible at the API boundary. Span metadata uses `SmallVec`; fragmentation increases metadata/clone work. Tiny views retain blocks; multi-span conversion to `Bytes` can copy.
- [bytesbuf_vs_bytes](https://github.com/microsoft/oxidizer/blob/074a147ab947fb44aa468805c59f6793b4fd34a6/crates/bytesbuf/benches/bytesbuf_vs_bytes.rs) supplies 32-span copy/scalar benchmark scenarios, not NFS results.
- 0.5.0 is Rust 1.88/MIT; current 0.14.0 requires Rust 1.95 and cannot satisfy workspace MSRV. Docs reported 93 downloads of 0.5.0, materially weaker adoption evidence than bytes. Matching Rust-1.88 published bytesbuf_io/runtime adapters remain unverified. Published docs.rs metadata supplied the release commit; GitHub generated VCS metadata path returned 404.

### Serde

Pin: [1.0.229, 7fc3b4c30c94f73a96ebd1553f2b090d928fc3a8](https://github.com/serde-rs/serde/tree/7fc3b4c30c94f73a96ebd1553f2b090d928fc3a8).
[Deserialize/Visitor](https://github.com/serde-rs/serde/blob/7fc3b4c30c94f73a96ebd1553f2b090d928fc3a8/serde_core/src/de/mod.rs)
separate borrowed, transient, and owned bytes. [Borrow tests](https://github.com/serde-rs/serde/blob/7fc3b4c30c94f73a96ebd1553f2b090d928fc3a8/test_suite/tests/test_borrow.rs)
show borrowed outputs reject transient/owned input and encode Cow lifetime rules.
Selected as additional established lifetime-contract evidence, not an XDR engine.
Rust 1.56, MIT OR Apache-2.0; workspace locks 1.0.228. A borrow cannot independently
outlive its owner or become a cross-chunk contiguous slice.

## Protocol and Compatibility

- [RFC 5531 section 11](https://www.rfc-editor.org/rfc/rfc5531#section-11): one message per record, one or more fragments, four-byte markers, final bit and 31-bit length. Sender fragments are not receiver allocation boundaries.
- [RFC 5531 section 8.2](https://www.rfc-editor.org/rfc/rfc5531#section-8.2): opaque authentication body maximum 400 bytes.
- [RFC 4506 sections 3, 4.10, 8](https://www.rfc-editor.org/rfc/rfc4506): four-byte XDR alignment, u32 opaque length, zero padding, provisioned-buffer bounds for untrusted sizes.
- Docs inspected root manifest/CI: Rust 1.88, edition 2024; configured toolchain 1.97.0; CI Windows/Linux x86_64, Linux/macOS aarch64. No WASM CI target found. Tokio server, Tokio/smol client adapters must remain supported.
- Crates inspected use MIT or MIT OR Apache-2.0; workspace is Unlicense. Normal dependency use must retain applicable distribution notices. Actual source copying would require applicable attribution/license preservation. No implementation source was copied by this research.

## Local Observations and Gaps

Server receive eagerly resizes/zero-fills a contiguous Vec per fragment; client
allocates a single advertised reply and rejects multiple fragments. Opaque decode
allocates/zero-fills a new Vec through `Read`, copies payload, and ignores padding
values. These are source observations, not measured byte/allocation counts.

No required official RPC/XDR or pinned source was unavailable to Docs. No Rust
checks, targets, NFS benchmark or memory measurement were run. Resource ceilings,
payload ownership, safe async lifetime boundaries, and measured applicability
remain architecture/verification responsibilities.

## D1-R2: User-Recommended rallocator

Identity: Microsoft Oxidizer `rallocator`, not historic Redox `ralloc`.
Pin: [rallocator 0.4.0, 6021c37b6b8e5459d3e487d75d1fb2d5834fc02b](https://github.com/microsoft/oxidizer/tree/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates/rallocator).
[Release metadata](https://crates.io/api/v1/crates/rallocator/0.4.0) reports release
2026-11-06, Rust 1.95 and MIT. [Version history](https://crates.io/api/v1/crates/rallocator/versions)
requires Rust 1.93.1 for 0.1.0 and 1.95 for subsequent releases; none fits workspace
MSRV 1.88. Seismograph 0.5.0 also requires 1.95. The crate is experimental, with
recent breaking changes; active development is not maturity evidence.

### APIs and Evidence

- [lib.rs](https://github.com/microsoft/oxidizer/blob/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates/rallocator/src/lib.rs): `rallocator::rallocator!()` installs the executable's global allocator. This changes allocator behavior, not just observation. One global allocator registration is permitted; there is no existing one in the repository per Docs search.
- [recording.rs](https://github.com/microsoft/oxidizer/blob/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates/rallocator/src/recording.rs): successful allocate/free and size-changing realloc events. Same-size realloc has no events; resized allocation is a free/allocation pair. Event addresses are correlation keys, not unique lifetime IDs.
- [Seismograph Policy](https://docs.rs/seismograph/0.5.0/seismograph/recorder/struct.Policy.html): `Policy::all(false)` records all events without backtraces; `all(true)` enables stacks. [Allocation](https://docs.rs/seismograph/0.5.0/seismograph/recorder/alloc/struct.Allocation.html) records requested size/alignment/address/thread and optional stacks. [Events](https://docs.rs/seismograph/0.5.0/seismograph/recorder/event/struct.Events.html) exposes total/lost events. Loss invalidates exact event counts.
- [callers documentation](https://docs.rs/seismograph_rallocator/0.5.0/seismograph_rallocator/events/fn.callers.html) warns unmatched events do not prove live allocations; addresses repeat. [Native snapshots](https://docs.rs/seismograph_rallocator/0.5.0/seismograph_rallocator/native/struct.Snapshot.html) inventory allocator/backend state, may be stale, and are not application-live object counts.
- [Event tests](https://github.com/microsoft/oxidizer/blob/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates/rallocator/tests/events.rs) cover pairing, stacks, cross-thread frees, realloc, sampling and snapshot recursion. [Allocator tests](https://github.com/microsoft/oxidizer/blob/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates/rallocator/tests/allocator_v4.rs) cover randomized resize/free and concurrent ownership transfers.
- [Benchmarks](https://github.com/microsoft/oxidizer/tree/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates/rallocator/benches) separate all-events/no-stacks, sampled and all-backtrace modes. No NFS-appropriate overhead numbers established. Bounded per-thread event buffers can overwrite; check lost events. `without_recording` suppresses synchronous recording on the current thread only, not a migrated async task or process.

### Fit and Limitations

Documented platform families include Linux/Windows MSVC/macOS on x86_64/aarch64
subject to page-size/OS requirements; WASM unsupported. Tokio/smol-specific
integration untested. OS-thread event identity is not task/request identity; the
bridge's allocator-thread/heap IDs are zero. Process-wide events include runtime,
backend, harness and instrumentation allocations. Async attribution requires
explicit operation/origin labels and must not rely on thread-local task identity.

Requested allocation size is not actual allocator usable capacity. There is no
built-in exact NFS peak-live/retained-origin counter. Reconstruction would need
known initial live state, complete loss-free events, validated cross-thread ordering
and address-lifetime reuse. Native pool snapshots cannot replace backing-origin
accounting. Payload copies and zero-fill still require explicit counters.

Docs recommends against direct workspace dependency under MSRV 1.88. Lead's D2
proposal instead uses an isolated, explicitly authorized newer-toolchain diagnostic
harness, with identical allocator for baseline/candidate, and leaves workspace MSRV
unchanged. This is a proposal, not tested integration. Crate and telemetry license
is MIT; preserve notices if distributed or copied. No code copied, packages
installed, builds or benchmarks run. D1-R1 bytesbuf_io compatibility remains open.

## D3-R4: Current bytesbuf and Ownership Under C2

2026-11-17, Buffering Docs, read-only, no builds/measurements/model override.
Supersedes MSRV-based rejection in historical D1-R2: user allows newer MSRV and
pools, recommends bytesbuf, permits worthwhile sound unsafe initialization, and
has not approved numerical limits or performance gates.

### Direct Reuse Evidence

Pin for bytesbuf 0.14.0, bytesbuf_io 0.14.0 and rallocator 0.4.0:
[Oxidizer 6021c37b6b8e5459d3e487d75d1fb2d5834fc02b](https://github.com/microsoft/oxidizer/tree/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates).
MIT; published manifests require Rust 1.95. Increasing MSRV is permitted, not yet
implemented. No ready-made Tokio/smol socket adapters were found.

| Source/symbol | Observed mechanism and implication |
| --- | --- |
| [BytesView](https://github.com/microsoft/oxidizer/blob/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates/bytesbuf/src/view.rs): range, append, from_views, slices, get_u32_be | Owned segmented views, intersection retention and cross-span scalar access. Eight inline spans is an optimization, not a limit. Tiny ranges retain backing blocks. `to_bytes()` copies multi-span views and allocates metadata even for a single span. |
| [BytesBuf](https://github.com/microsoft/oxidizer/blob/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates/bytesbuf/src/buf.rs): put_bytes, first_unfilled_slice, advance, BytesBufVectoredWrite::commit | put_bytes appends owned views without copying. Unfilled slices use MaybeUninit; advance/commit requires initialized sequential prefix. Dropping vectored operation commits nothing. Use bounded receive quanta, never peer advertised length as reserve amount. |
| [VecBlock conversion](https://github.com/microsoft/oxidizer/blob/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates/bytesbuf/src/vec.rs) | Ordinary Vec conversion adopts payload; pointer-identity tests support this. Above BlockSize::MAX conversion uses split_off and may copy suffixes despite unqualified zero-copy docs. Capacity stays retained; 5 GB test checks spans, not copy count. |
| [bytes conversion](https://github.com/microsoft/oxidizer/blob/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates/bytesbuf/src/bytes_compat/from_bytes.rs) | Retains Bytes owner; borrowed slices instead use copied_from_slice. No reason to wrap every bytesbuf owner in Bytes. |
| [bytesbuf_io](https://github.com/microsoft/oxidizer/tree/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates/bytesbuf_io/src): Read::read_at_most_into/read_more_into/read_any, Write::write | Runtime-neutral Send APIs own input/output buffers; Write completes all or errors. read_exactly/read_at_most reserve requested lengths up front. Partial/EOF/exact and pending-cancellation tests exist, but stream cancellation safety is not a trait guarantee. |
| [Tokio ReadBuf](https://docs.rs/tokio/1.53.0/tokio/io/struct.ReadBuf.html): uninit, filled | Public API can fill a bytesbuf uninitialized span; adapter must commit only initialized bytes returned from a successful poll. Consumed stream bytes require state preservation or close on cancellation. API feasibility is not measured benefit. |

Smol's present wrapper accepts initialized slices. Compare initializing its writable
span in place (no extra payload copy), a safe temporary-buffer bridge, and any
proven uninitialized adapter if supported; do not cast uninitialized memory to
`&mut [u8]`. Exact adapter implementation still needs proof and measurement.

### Pool and Accounting Evidence

[GlobalPool](https://github.com/microsoft/oxidizer/blob/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates/bytesbuf/src/mem/global.rs)
uses 1/4/16/64 KiB classes, selects one class per reservation, multiple 64 KiB
blocks above it. Final block reference returns a slot. Tests cover size classes
and cross-thread release. GlobalPool uses unbounded underlying pools and infallible
allocation; no public trim or retained-byte cap. Peak chunks can remain retained.

[plurality 0.2.4, f81226aa8a58dbfe3f6b87b2aedb3250ac53b047](https://github.com/microsoft/oxidizer/blob/f81226aa8a58dbfe3f6b87b2aedb3250ac53b047/crates/plurality/src/pool.rs)
has PoolBuilder chunk_size/max_chunks, try_alloc methods and optional stats. Pool
is !Sync; allocation requires serialization. Chunks are reused until pool teardown,
not individually trimmed. Concurrent live-count stats are approximate.

GlobalPool histograms report requested sizes, class and pool instances, not exact
origin/capacity/live bytes. Its block metadata is private. Logical BytesView IDs
are not allocation IDs. [BlockMeta](https://docs.rs/bytesbuf/0.14.0/bytesbuf/mem/trait.BlockMeta.html)
and [CallbackMemory](https://github.com/microsoft/oxidizer/blob/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates/bytesbuf/src/mem/callback_memory.rs)
allow custom metadata/provider accounting; Memory::reserve remains infallible at
its API boundary. A checked pre-reservation/provider integration needs a concrete
design; do not invent fallible behavior or public GlobalPool capacity hooks.

### Standard Ownership Comparison (R9)

[Rust 1.95 alloc::sync](https://github.com/rust-lang/rust/blob/1.95.0/library/alloc/src/sync.rs):
Arc::new(Chunk { Vec, capacity, identity, ticket }) moves a Vec header into one Arc
allocation, preserving its separate payload allocation. Arc<[u8]>::from(Vec)
creates new Arc slice storage and transfers/copies elements; it does not adopt
the Vec allocation. Arc::new is infallible; Arc::try_new remains unstable.

[bytes 1.12.1, 76c0fbb54ed4336caf9d2311658a2f4a5627c21d](https://github.com/tokio-rs/bytes/tree/76c0fbb54ed4336caf9d2311658a2f4a5627c21d):
Bytes::from(Vec) adopts payload and may allocate shared metadata when spare capacity
exists. An outer Arc<Bytes> adds a wrapper without exposing capacity/identity.
Arc<Chunk> is a useful simple measured comparator, not an automatic preferred
replacement for bytesbuf. BytesMut growth may shift/reallocate/copy.

### Rallocator and Remaining Gaps

Rallocator 0.4.0 now fits a permitted MSRV increase. Register once in a measurement
binary; no need to change the product allocator or force a separate workspace.
Requirements include Linux 4 KiB pages, Windows MSVC, or macOS 14.4+, x86_64/ARM64;
no WASM. D1-R2 event/loss/realloc limitations still apply. Baseline/candidate must
use identical allocator/toolchain per lane. Platform and overhead untested.

Docs recommends prototyping direct current bytesbuf for records/views, local RPC
framing/XDR cursor/socket adapters, and bytesbuf_io only where its contract helps.
No numerical restriction, performance gate, fragmentation policy or mandatory
unsafe path selected. NFS performance, exact custom-provider lifecycle hooks,
runtime adapter cancellation, pool retention and large-Vec split behavior remain
unverified. Pinned http_headers, tokio-util and Serde evidence in D1-R1 remains
applicable; no implementation source copied.

## D3-R5: Incomplete Provider Investigation

Docs returned partial findings, not a finished API feasibility report: custom
blocks/metadata appear capable of lifecycle accounting; construction has unsafe
ownership obligations; configured growth showed no intrinsic GlobalPool fallback.
Memory::reserve and SmallVec metadata growth remain infallible. Exact construction,
range metadata and final-drop signatures were not supplied. These findings do not
establish a usable custom provider. No code/build/measurement. Follow-up D3-R6
requests only the missing concrete signatures; do not broaden research again.

## D3-R6: Custom Block Feasibility

Docs completed the follow-up. Canonical full pin is D3-R4's
`6021c37b6b8e5459d3e487d75d1fb2d5834fc02b`; returned links accidentally omitted
the final character, so use the full pin below.

- [Memory](https://github.com/microsoft/oxidizer/blob/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates/bytesbuf/src/mem/memory.rs) and [CallbackMemory](https://github.com/microsoft/oxidizer/blob/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates/bytesbuf/src/mem/callback_memory.rs): reserve returns BytesBuf, not Result. External payload preflight cannot make metadata allocation fallible.
- [Block](https://github.com/microsoft/oxidizer/blob/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates/bytesbuf/src/mem/block.rs) and [BytesBuf::from_blocks](https://github.com/microsoft/oxidizer/blob/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates/bytesbuf/src/buf.rs): public unsafe constructors accept provider-owned storage. Pointer validity, stable lifetime and exclusive writable access are provider obligations. BlockSize is u32; from_blocks does not require GlobalPool and may use blocks in any order.
- [BlockRef/BlockMeta](https://github.com/microsoft/oxidizer/blob/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates/bytesbuf/src/mem/block_ref.rs): custom downcastable Any+Send+Sync+Debug metadata; unsafe BlockRefDynamic clone/drop dispatch must maintain thread-safe reference lifetime. Final reference can free/return provider-owned storage. Prefer standard ownership machinery over a novel refcount.
- [BytesView::slices](https://github.com/microsoft/oxidizer/blob/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates/bytesbuf/src/view.rs) and BytesBuf::first_unfilled_slice_meta expose optional block metadata. Ranges can identify intersected origins; metadata lifetime follows block lifetime.
- BlockRef::smoke_test checks metadata downcasting and clone/drop counts; BytesBuf::first_unfilled_slice_meta_with_meta covers writable metadata. These establish source feasibility, not NFS adapter correctness.
- Ordinary From<Vec> adoption cannot return its Vec publicly; to_vec copies. A custom block owner must retain the original Vec itself to reclaim it at final release. Unsafe extension contracts apply even when storage is initialized. No safe constructor providing all accounting/recycle behavior was established.

Verdict: accounted pooling is feasible through public unsafe extension points,
not an already implemented safe adapter. No fallible reservation interface or
public Vec reclamation exists. A wrapper can prepare payload blocks fallibly and
explicitly supply them, but must prevent hidden growth and cannot promise full
OOM recovery. Proofs/benchmarks must establish correctness and benefit. No builds,
measurements, files or code changed by Docs.