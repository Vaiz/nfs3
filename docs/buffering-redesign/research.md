# Buffering Evidence

Source research only; no NFS measurements or cross-target builds. Keep pins and
mechanisms supporting current choices, not research-attempt history. Decisions
belong in [design](design.md); remaining proofs belong in [validation](validation-plan.md).

## bytesbuf and IO

Pin: [Oxidizer 6021c37b6b8e5459d3e487d75d1fb2d5834fc02b](https://github.com/microsoft/oxidizer/tree/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates),
bytesbuf/bytesbuf_io 0.14.0, MIT, Rust 1.95. User recommends this candidate and
permits MSRV increases; that does not establish integration or performance.

| Source | Mechanism and limitation |
| --- | --- |
| [BytesView](https://github.com/microsoft/oxidizer/blob/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates/bytesbuf/src/view.rs) | Owned ranges/slices and cross-span scalars; eight inline spans is not a limit. Tiny views retain blocks; multi-span `to_bytes` copies, single-span conversion still allocates metadata. |
| [BytesBuf](https://github.com/microsoft/oxidizer/blob/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates/bytesbuf/src/buf.rs) | Appends views without payload copies; unfilled MaybeUninit spans require exact initialized-prefix advance/commit. Vectored-operation drop commits nothing. |
| [Vec conversion](https://github.com/microsoft/oxidizer/blob/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates/bytesbuf/src/vec.rs) | Pointer-identity tests support ordinary adoption. Above u32 BlockSize::MAX, split_off may copy; capacity remains retained. No public Vec reclaim; to_vec copies. |
| [GlobalPool](https://github.com/microsoft/oxidizer/blob/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates/bytesbuf/src/mem/global.rs) | 1/4/16/64 KiB classes, multiple large blocks, cross-thread return tests. Infallible/unbounded underlying pools; no public trim/retained cap or exact origin stats. |
| [bytesbuf_io](https://github.com/microsoft/oxidizer/tree/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates/bytesbuf_io/src) | Runtime-neutral owned Send buffers; exact/at-most reads reserve requested length. Partial/EOF/cancellation tests do not promise trait-wide cancellation safety. No ready Tokio/smol adapters found. |

[Tokio ReadBuf](https://docs.rs/tokio/1.53.0/tokio/io/struct.ReadBuf.html) can fill
uninitialized spans; adapter must preserve consumed-stream state and commit only
initialized bytes. Smol currently takes initialized slices; compare in-place
initialization, not an uninitialized slice cast. Benefit is unmeasured.

Custom accounting/recycling is source-feasible through public unsafe extensions:

- [Block](https://github.com/microsoft/oxidizer/blob/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates/bytesbuf/src/mem/block.rs)
  and `BytesBuf::from_blocks` accept provider-owned stable storage without GlobalPool.
- [BlockRef/BlockMeta](https://github.com/microsoft/oxidizer/blob/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates/bytesbuf/src/mem/block_ref.rs)
  supply Any+Send+Sync+Debug metadata and dynamic clone/drop dispatch. Provider must
  prove lifetime/exclusivity/refcount correctness and retain original backing to recycle.
  Smoke tests cover metadata/clone/drop, not a finished NFS adapter.
- `slices`/`first_unfilled_slice_meta` expose intersected-origin metadata.
  [Memory](https://github.com/microsoft/oxidizer/blob/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates/bytesbuf/src/mem/memory.rs)
  reserve returns BytesBuf, not Result. External payload preflight cannot make
  SmallVec/control allocation fallible. Full OOM recovery is not established.

## Established Comparators

These are independently selected for relevant maintained implementations/tests;
upstream benchmark results are not NFS performance evidence.

| Candidate and pin | Evidence and applicability |
| --- | --- |
| [http_headers, 6230675b0c2c8a759e0ef5e9db92d11958f5e55e](https://github.com/microsoft/oxidizer/tree/6230675b0c2c8a759e0ef5e9db92d11958f5e55e/crates/http_headers) | `src/field_value.rs`: borrowed raw view versus validated owner, 64-byte inline values/shared large storage. Source/sink separation and `tests/field_value_allocations.rs`, `tests/benchmark_ownership.rs` support ownership/allocation patterns. Unpublished; HTTP grammar/limits do not transfer. PERF notes are not rerun results. |
| [bytes 1.12.1, 76c0fbb54ed4336caf9d2311658a2f4a5627c21d](https://github.com/tokio-rs/bytes/tree/76c0fbb54ed4336caf9d2311658a2f4a5627c21d) | Established runtime-neutral ownership library. `src/bytes.rs`: Vec adoption/shared slicing can retain whole allocation. `BytesMut` freeze shares; reserve may shift/copy. `Buf::chunk` is a prefix, not whole contiguous storage. Chain/limit tests and clone/slice/growth benches provide comparator cases. |
| [tokio-util 0.7.20, 5ec9e6b8d5a7111fe9cd35a637bd8b4e4eb1e483](https://github.com/tokio-rs/tokio/tree/5ec9e6b8d5a7111fe9cd35a637bd8b4e4eb1e483/tokio-util) | Maintained async framing/error tests. LengthDelimitedCodec checks max before reserve but assembles contiguous single frames, not fragmented RPC. `tests/length_delimited.rs` covers pending/incomplete input. Tokio-only, not smol; lockfile compatibility needs checking. |
| [Serde 1.0.229, 7fc3b4c30c94f73a96ebd1553f2b090d928fc3a8](https://github.com/serde-rs/serde/tree/7fc3b4c30c94f73a96ebd1553f2b090d928fc3a8) | Established lifetime-contract comparator. Visitor borrowed/transient/owned distinctions and `test_suite/tests/test_borrow.rs` clarify ownership; no XDR engine or cross-span contiguous borrow. |
| [Rust 1.95 Arc](https://github.com/rust-lang/rust/blob/1.95.0/library/alloc/src/sync.rs) | Arc<Chunk { Vec, ticket }> moves Vec header, preserving payload with one control allocation. Arc<[u8]>::from(Vec) allocates/transfers instead. Arc::new is infallible; try_new unstable. Useful simple comparator, not selected winner. |

bytes/Serde/Tokio use MIT OR Apache-2.0; Oxidizer uses MIT. Preserve distribution
notices; source copying needs separate attribution review. Workspace Unlicense
does not relicense dependencies. No upstream implementation was copied.

## rallocator

Microsoft Oxidizer rallocator 0.4.0, same `6021c37...` full pin above, MIT/Rust 1.95;
not Redox ralloc. [Source/tests](https://github.com/microsoft/oxidizer/tree/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates/rallocator)
establish global allocator registration and allocation/free/size-changing realloc
events. This changes allocator behavior; use a measurement executable, not product
allocator. Experimental integration and overhead remain unverified.

- [Seismograph 0.5.0 Policy](https://docs.rs/seismograph/0.5.0/seismograph/recorder/struct.Policy.html):
  `all(false)` records unsampled without stacks; bounded buffers may lose events.
  Check capture loss counters; no exact counts with loss.
- Events contain requested size/alignment/address/thread, not usable capacity,
  unique lifetime, task identity, payload copies or zero-fill. Same-size realloc
  emits no events; resized allocation is free+alloc and can hide overlap.
- Address reuse/cross-thread ordering/initial live set must be validated before
  reconstruction. Native snapshots/unmatched events do not prove live origins.
  Thread-local suppression cannot label migrated async tasks.
- Event/allocator tests cover pairing, stacks, recursion, cross-thread frees and
  resize. Use explicit origin/copy hooks for exact protocol metrics.
- Platform requirements: Linux 4 KiB pages, Windows MSVC, macOS >=14.4,
  x86_64/aarch64; no WASM. Actual target/runtime support needs validation.

## Protocol

- [RFC 5531 section 11](https://www.rfc-editor.org/rfc/rfc5531#section-11): records
  contain fragments with four-byte markers, final bit and 31-bit length.
- [RFC 5531 section 8.2](https://www.rfc-editor.org/rfc/rfc5531#section-8.2): auth body <=400.
- [RFC 4506](https://www.rfc-editor.org/rfc/rfc4506): four-byte alignment, u32 opaque
  lengths, zero padding and bounds on untrusted input.
- [RFC 1813](https://www.rfc-editor.org/rfc/rfc1813): NFS counts/status/stable-write
  semantics; filename/path strings are wire-unbounded, handle <=64.