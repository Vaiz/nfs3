# Validation Plan

The [B0.1 fixture capture and gate evidence](baseline.md#b01-active-verification-evidence)
cover only opaque diagnostics, not a complete NFS baseline or performance claim.
Changed inputs require fresh assessment. B0 authorization and outstanding gates
are in [baseline](baseline.md); acceptance thresholds remain unset.

## Measurements

Per operation/endpoint and whole workload, report:

- Payload bytes copied by transport forwarding, growth, codec, materialization,
  padding batching, backend and test transport; zero-fill separately.
- Allocation/reallocation count/bytes, largest allocation, metadata/refcount cost,
  unique peak/live/retained/free-pool backing and old+new growth overlap.
- Queue depth, read/write syscall counts, throughput, p50/p95/p99 latency.
  Credits, Rust capacity, allocator usable bytes and RSS are distinct; RSS is supplementary.

Validate instrumentation first with deterministic allocate/clone/slice/realloc/drop/
trim sequences and an independent expected ledger. Use generation IDs, exact final
drop counts, known initial live set and validated cross-thread ordering/address
reuse. Check loss counters at every capture boundary; loss-free events alone do
not prove exact peaks. Collector must be bounded/nonrecursive and explicitly costed.
No thread-local task identity across awaits. Missing exactness blocks that metric.

Keep compiler/allocator/features/harness identical per baseline/candidate lane.
Measure rallocator diagnostic and normal-allocator timing separately. Serial paired
release runs alternate order; retain raw samples/timing boundaries, uncertainty and
instrumented/uninstrumented overhead, not mathematically corrected timing.
Only equivalent completed work supports speedup ratios. Noisy results are inconclusive.

Record revision plus relevant dirty patch/untracked source, compiler/target/OS/CPU,
profile/features/allocator, runtime/backend, workload/seed/concurrency, run counts
and exact commands. Preserve one active baseline and comparison evidence, not a
sequence of status snapshots. Agree numeric acceptance criteria before behavior changes.

## Coverage

| Area | Required cases |
| --- | --- |
| Normal work | NULL/GETATTR/LOOKUP; large READ/WRITE; directory and retained-handle traversal. |
| Framing | RPC fragments independently varied from TCP segmentation; every marker/scalar/padding split; multiple records/read; empty/tiny fragments; truncation and overflow. |
| Resource behavior | Header-only huge lengths with bounded input; incremental body delivery; slow readers/writers alongside healthy traffic; concurrency/overload/fairness; N records larger than receive budget/N. |
| Ownership | Tiny views at block edges, cross-span retention, final-drop/shutdown/reentrant callbacks, cold/warm pools and trim where implemented. |
| Cancellation/retry | Every partial IO state, before/after side effects, owned/blocking backend work, conflicting XID and outcome exhaustion. |
| Transport | Real TCP Nagle on/off, short/zero-progress IO, released single-fragment-only clients. |
| Codec/API | Auth/padding/count/discriminant errors, nested collection expansion, all derive forms/generics, owned metadata, Send/Sync, Tokio and smol. |

Use bounded inputs/accounting hooks, never deliberate OOM. Candidate-only features
are unsupported baseline cases, not fabricated measurements. Unsafe adapters need
soundness review, Miri/interleaving/drop tests and comparison with initialized storage.

Wire proof must exceed symmetric round trips: hand-authored RFC golden bytes,
independent packet builder/parser, captured output and an independent NFS peer.
Unavailable peers are explicit gaps. Preserve NFS stable/COMMIT/exclusive CREATE.

## Repository Gates

Use [repository instructions](../../.github/copilot-instructions.md) and
[Rust execution rules](../../.github/agents/buffering/rust-validation.md): pre-edit
check; focused post-edit checks; strict Clippy; nightly fmt (Verify check-only);
all-feature tests; all-target/all-feature build; machete; deny.

Require locked CI debug/release and selected MSRV coverage for Linux x86_64/aarch64,
Windows x86_64 and macOS aarch64; Tokio/smol/examples/generated codecs/callers.
Build test_mirror_fs and the matching server through MCP before CI-equivalent
invocation; select the debug/release server explicitly with NFS3_MIRROR_SERVER.
WASM support is unestablished. Missing tools, targets,
policy or measurements block their gates, never count as passes.

After each stage Verify supplies status, baseline-relative patch and untracked
source; Review inspects actual files and evidence. No approval carries to changed inputs.