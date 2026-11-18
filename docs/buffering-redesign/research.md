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

### B0.1 recorder API handoff

Docs checked versioned online manifests/APIs and pinned upstream tests, not only
local registries. Pin `rallocator = "=0.4.0"` and `seismograph = "=0.5.0"` with
default features disabled; no monitor/test-util features are needed. Both and the
transitive seismograph_rallocator 0.5.0 declare MIT/Rust 1.95.

- [`rallocator::rallocator!()` and event tests](https://github.com/microsoft/oxidizer/blob/6021c37b6b8e5459d3e487d75d1fb2d5834fc02b/crates/rallocator/tests/events.rs)
  provide isolated global registration and direct `GlobalAlloc` fixture patterns.
- [`Configuration`](https://docs.rs/crate/seismograph/0.5.0/source/src/recorder.rs):
  set `allocations: Policy::all(false)` and a bounded
  `EventBufferCapacity::new(64)` (minimum power-of-two capacity, per thread).
  Stack-free policy does not eliminate fixed stack storage in each ring slot.
- [`SnapshotOptions`](https://docs.rs/crate/seismograph/0.5.0/source/src/snapshot.rs):
  `EventBufferDisposition::Stop` captures/disables/releases buffers; `Clear`
  captures/clears/continues; default `Retain` does not drain. Decode `as_bytes()`
  under synchronous thread-local `without_recording`.
- [`Allocation`](https://docs.rs/crate/seismograph/0.5.0/source/src/recorder/alloc.rs)
  exposes address, requested size/alignment, heap and thread fields.
  `allocation_id` is an address-correlation key, not a unique lifetime.
  Validate per-thread sequences and event-vector counts as well as total/lost
  counters: ring-slot lock timeouts can omit entries beyond overwritten-prefix
  loss accounting. No global cross-thread order or initial live set is guaranteed.

Use a single-thread explicit allocate/free fixture and stopped capture first.
These are source contracts, not executed results or exact process-peak evidence.

### RPC checkpoint constraint

For B0.4, repeated Retain snapshot lifecycle, nested suppression and exact recorder
slot/metadata bytes are not independently established. The bounded contract avoids
these dependencies: allocation markers identify checkpoints in one complete
single-thread trace, followed by one Stop snapshot; observer tracking/reporting
runs afterward. Marker/runtime/transcript/validation costs remain in the measured
event-visible cohort. Neither this sequence nor loss-free events establish the
process's initial heap or actual realloc overlap/count.

## Protocol

- [RFC 5531 section 11](https://www.rfc-editor.org/rfc/rfc5531#section-11): records
  contain fragments with four-byte markers, final bit and 31-bit length.
- [RFC 5531 section 8.2](https://www.rfc-editor.org/rfc/rfc5531#section-8.2): auth body <=400.
- [RFC 4506](https://www.rfc-editor.org/rfc/rfc4506): four-byte alignment, u32 opaque
  lengths, zero padding and bounds on untrusted input.
- [RFC 1813](https://www.rfc-editor.org/rfc/rfc1813): NFS counts/status/stable-write
  semantics; filename/path strings are wire-unbounded, handle <=64.

## Dependency Gate Policy

Docs checked official [license configuration](https://embarkstudios.github.io/cargo-deny/checks/licenses/cfg.html)
and [advisory configuration](https://embarkstudios.github.io/cargo-deny/checks/advisories/cfg.html),
plus cargo-deny source at
[`4b1f2002a11077f25437c2983f0a1a5439a886f4`](https://github.com/EmbarkStudios/cargo-deny/tree/4b1f2002a11077f25437c2983f0a1a5439a886f4/src).
Default license allowlist and advisory ignores are empty. SPDX OR needs an allowed
branch; AND needs every term. Crate-specific license exceptions and advisory
ignores are policy choices, not mechanical repairs or remediation.

The reported base-graph license failures cannot be cleared merely by removing
B0 dependencies. The owner has now approved the [exact policy/remediation scope](baseline.md#outstanding-gates);
no advisory ignores or exceptions. Direct advisory-only Rust MCP check on the clean
prerequisite worktree scanned successfully with the optional `disable_fetch`
argument omitted; all five records are current scanner findings, not guessed IDs.
Online advisory/release verification is recorded below.

## Approved Advisory Remediation Evidence

Docs verified the five IDs against official RustSec records and upstream releases.
These establish dependency advisories, not local exploitability.

| Dependency | Pinned advisory and minimal route |
| --- | --- |
| anyhow | [RUSTSEC-2026-0190](https://github.com/RustSec/advisory-db/blob/81f061183406286f0fe1707ce6f1905914c8e380/crates/anyhow/RUSTSEC-2026-0190.md), [1.0.103 release](https://github.com/dtolnay/anyhow/releases/tag/1.0.103). MIT OR Apache-2.0; MSRV 1.68. |
| chrono | [RUSTSEC-2026-0543](https://github.com/RustSec/advisory-db/blob/233155b4025b854c68a4b199c0e32d7fd53291a2/crates/chrono/RUSTSEC-2026-0543.md): all versions unmaintained, no patched version. Only client `ls` example uses it. |
| crossbeam-epoch | [RUSTSEC-2026-0204](https://github.com/RustSec/advisory-db/blob/463f03a9c0ce5519ecc99bf257204ccfa167a8ed/crates/crossbeam-epoch/RUSTSEC-2026-0204.md), [0.9.20 release](https://github.com/crossbeam-rs/crossbeam/releases/tag/crossbeam-epoch-0.9.20). Compatible with deque's ^0.9.17; MIT OR Apache-2.0, MSRV 1.61. |
| intaglio | [RUSTSEC-2026-0078](https://github.com/RustSec/advisory-db/blob/a9993361b06388325618aa11ec691faaac5f9a3e/crates/intaglio/RUSTSEC-2026-0078.md), [1.13.3 release](https://github.com/artichoke/intaglio/releases/tag/v1.13.3). MIT; MSRV 1.85. |
| tokio | [RUSTSEC-2026-0386](https://github.com/RustSec/advisory-db/blob/84d943a6deeb8cc6c6f632531aaf4f1c62275239/crates/tokio/RUSTSEC-2026-0386.md): fixed >=1.51.6,<1.52.0 OR >=1.53.3. Choose direct floor 1.53.3 rather than ^1.51.6, which also admits vulnerable 1.52.x. [Selected manifest](https://github.com/tokio-rs/tokio/blob/a85c3f5baa298a6b554fd3700daf0342289881af/tokio/Cargo.toml): MIT, MSRV 1.71, existing features retained; macros requires tokio-macros ~2.7.0. |

For the example-only chrono replacement, [Jiff 0.2.28 pinned source/manifest](https://github.com/BurntSushi/jiff/tree/66f6ec2d0b75112deadf7e067dbf2ef114d41c50)
supports `Timestamp::try_from(SystemTime)`, `default-features=false`,
`features=["std"]`, Unlicense OR MIT, MSRV 1.70.
Its default `Timestamp` display uses `Z`; the [approved example scope](baseline.md#outstanding-gates)
accepts that format directly. No custom formatter, timezone database or broader
time migration is needed. This is source evidence, not cross-target validation.