# B0 Baseline-Only Contract

User authorized B1-B4 document corrections and baseline-only instrumentation with
rallocator/Rust 1.95 on 2026-11-17. This remains valid. No buffering behavior,
prototype, resource policy or performance threshold is authorized. Architecture
review blockers do not prevent this separate unchanged-behavior stage.

## Allowed Changes

- Implement is sole Rust/test/benchmark writer; Verify independently captures and
  Review inspects actual patch/evidence. No concurrent writers or benchmarks.
- Raise workspace MSRV to 1.95 and directly affected docs/checks. Existing CI 1.97.0
  and nightly formatter need not change. No unapproved toolchain installation.
- Pin rallocator 0.4.0 and compatible recording dependencies at workspace level,
  used only by an opt-in nfs3_tests measurement executable/bench. Verify APIs/licenses.
  No product allocator replacement, bytesbuf prototype or unrelated dependency updates.
- Feature-gated hooks at existing receive/growth/codec/send/backend/test-copy sites,
  absent by default. No scheduling/allocation/ownership/VFS/IO/retry behavior changes.
- Reuse existing perf/tests; add a dedicated executable only for allocator isolation
  or structured output. Test tracker consistency and hook-on/off golden wire bytes.

## Measurement

Follow [validation-plan](validation-plan.md). Report process allocation events and
explicit protocol-origin/copy/zero-fill hooks with honest attribution. Address is
not lifetime identity; capacity change alone does not prove growth-copy bytes.
Unknown movement stays bounded/unknown, not zero. Retained output lasts to actual
drop. Baseline has no new pool/read-into features: label them absent/unsupported.

Use identical compiler/allocator/harness per comparison lane; keep diagnostic
rallocator and normal-allocator timing separate. Record reproducibility-critical
inputs, raw samples, certainty and overhead. Harness workload/time limits are not
product policy. No deliberately OOM-inducing advertised-length probes or hangs.

## First Slice and Acceptance

Start with deterministic opaque encode/decode or existing RPC: known payload,
copy/initialization counters and golden bytes must agree with unchanged code;
feature-disabled behavior must match. Run immediately after first substantive edit.
Then cover metadata RPCs, 4/64/256/1024 KiB READ/WRITE, retained outputs/handles,
directories and safely bounded fragmentation/short-read/slow-peer/concurrency cases.
Unsupported old-client/cancellation workloads are correctness results, not speedups.

Acceptance: unchanged default behavior/wire bytes, opt-in instrumentation/allocator,
validated counters, independent reproducible capture, relevant Review acceptance
and repository gates. Missing required metrics/gates block completion. No speedup
is expected or claimed. If exact instrumentation requires redesign, stop at the gap.

Use [Rust rules](../../.github/agents/buffering/rust-validation.md); shell Cargo
and further tool installation are not granted. The user authorized committing
the current baseline work, pushing this branch and opening a PR for CI. This
does not authorize product behavior changes or merging. The user also authorized
the current session rebase onto origin/main without renaming. Running an MCP-built
measurement executable for authorized capture is allowed; record command/settings.

## B0.2 Scoped Opaque Retention

This bounded continuation of authorized baseline-only work changes only the
measurement executable and focused harness tests. It does not change product
allocation, ownership, scheduling, wire handling or dependencies. Keep the existing
one-byte B0.1 lane separately runnable and comparable: select the lane before
recording, run each in its own process and preserve default B0.1 output.

- Prepare deterministic RFC opaque inputs of 4/64/256/1024 KiB before recording;
  reset logical counters before the measured decode phase. Retain all four decoded
  owned outputs, then drop largest to smallest. Use nonzero payload bytes checked
  in place; pass outputs/observations through black_box against allocation elision.
  Inputs/recorder/runtime allocations are outside the claimed output scope.
  Store outputs/observations in fixed arrays, do not print or allocate metadata in
  the window, and keep prepared inputs alive until after the stopped snapshot.
  Validate/print under without_recording; do not assume recorder restart support.
- Observe each output's address and Vec capacity without copying or changing its
  ownership. Match the complete single-thread trace to four allocations and four
  frees, using requested sizes/alignment and the independently expected schedule.
  Exactly eight output events must fit the existing 64-slot recorder bound.
  Reject extra, missing, lost, unordered, wrong-sized or ambiguous events.
- Assign monotonic generations at allocation events; an address is a lookup key,
  never a lifetime ID. Require an empty initial fixture-output live set, match
  each final drop exactly once and finish with zero live outputs. Validate sequence
  boundaries, checked sums and counter loss before publishing exact results.
  Reject simultaneous live origins with the same address.
- Report scoped requested live/peak bytes and retained output bytes at named
  checkpoints. The all-four checkpoint is 1,380,352 bytes only if observed allocation
  sizes/capacities match the expected payloads. Validate this against an independent
  ledger and hand-authored wire lengths/payloads, not just a self-consistent tracker.
  Codec counts remain logical IO and explicit zero-fill, not physical copy claims.
  Fixed live checkpoints after decode are 4,096 / 69,632 / 331,776 / 1,380,352;
  after drops they are 331,776 / 69,632 / 4,096 / 0. Expected decode/zero-fill ledger
  is four completions and 1,380,352 bytes each, with all other sites zero.
  These aligned sizes do not exercise padding; the B0.1 lane still covers it.
- Keep process-wide memory, usable allocator bytes, realloc overlap, cross-thread
  attribution and instrumentation overhead explicitly unknown. This lane has no
  pool, shared slices, clone accounting or timing claim.

Focused falsification covers generation reuse, duplicate/missing/out-of-order events,
unmatched/premature/double frees, wrong size/alignment, event/counter loss and checked
arithmetic failure, using synthetic traces against the same pure validator invoked
by the executable, rather than invalid real allocations. Verify process isolation
between lanes; do not depend on real allocator address reuse in tests.
Independent capture must record exact build/run inputs and current source delta.
Broader RPC/workload/overhead and external platform gates remain separate.

## B0.3 RPC Copy and Growth Ledger

Continue baseline-only instrumentation; preserve existing IO calls, allocations,
ownership, queues, error behavior and task spawning. New feature-gated hooks are
absent by default; no new dependencies or allocator replacement in this lane.
Share a bounded allocation-free ledger through types, reusing opaque Count
semantics, and explicitly forward the feature through client/server/test manifests.

- Instrument existing client record encode/send and reply marker/body read/zero-fill;
  server selected socket-read results, socket-to-duplex forwarding, fragment marker/body reads, resize zero-fill,
  reply encode/send; and the test ReadFs explicit to_vec copy and WriteFs consumed
  bytes. Do not equate transport IO with physical copies. Attempt/completion bytes
  remain distinct on error or cancellation.
  Exception: process_socket races read against replies in select!. Its socket-read
  site records only selected results, after selection, never all initiated futures.
  Label selected success/error/EOF separately; cancelled losing branches and read
  attempt counts are uninstrumented. Do not wrap/pin those futures or alter select!.
  This site uses a separate selected-outcome struct, not Count: successful calls
  and bytes, EOF, WouldBlock, other errors and overflow loss. Observe &result at
  branch entry. Exact success requires one EOF, zero errors/loss, selected bytes
  equal captured client bytes and completed forwarding bytes, and forwarding
  attempts equal completions. This assumes normal cancel-safe Tokio duplex/TCP
  AsyncRead behavior, not arbitrary custom IO. Scratch zero-fill stays separate.
- Observe old length/capacity and new capacity around existing record growth.
  Report capacity-growth observations and bounded possible logical-payload
  relocation, not inferred realloc counts, address lifetime IDs or actual copying.
  A successful resize's new initialized bytes are separately measurable.
- Use a fresh isolated process/current-thread runtime and connection for each
  measured case, with one in-flight request. Reset before spawning the connection;
  observe server send completion as well as client reply completion before closing.
  Prove socket/reader/handler termination before the final ledger; feature-gated
  lifecycle observations may count starts/ends without retaining or joining tasks.
  Account the final EOF read separately; an unmatched attempt alone does not
  distinguish failure from cancellation. Ledger attribution is all work on that
  OS thread, not async task identity. Never reset beneath a pending marker read.
- Add a normal-allocator workload executable and focused tests, runnable with
  hooks disabled or enabled. Reuse Server, ReadFs and WriteFs; perform NULL,
  GETATTR, LOOKUP and READ/WRITE at 4/64/256/1024 KiB with deterministic nonzero
  data. Verify returned bytes/count/EOF/stability and completed operations.
  Bound workload and shutdown time; retain measured outputs until explicit
  checkpoints, but do not call capacity observations exact lifetime accounting.
  WRITE's sink must validate deterministic consumed bytes with allocation-free
  diagnostic observations, without changing its success/error behavior.
- Assert conservation against independent captured wire lengths and explicit
  copy sites. Any transcript capture copies are separately attributed to test
  instrumentation, not hidden inside protocol-copy totals. Capture storage is
  bounded and excluded from product-memory claims.
- Hand-authored RPC/XDR expectations must run in both feature configurations,
  exceeding symmetric round trips. Validate any variable XID/handle/verifier
  fields against their originating fixture/request state; do not normalize away
  payload, framing or semantics. Test counter overflow, partial/error boundaries
  and growth without allocation versus growth with an unknown relocation amount.
  The opaque ledger includes handles, names and empty authentication, not just
  bulk payloads. Validate all applicable sites including zero-length calls.

Independent expected complete wire lengths (record marker included), for empty
authentication, 16-byte handles, name `file` and successful attributes:

| Case | Request bytes | Reply bytes |
| --- | --- | --- |
| NULL | 44 | 28 |
| GETATTR | 64 | 116 |
| LOOKUP | 72 | 228 |
| READ of N aligned bytes | 76 | N + 132 |
| WRITE of N aligned bytes | N + 84 | 164 |

Conservation is about successful total bytes, not scheduler-dependent chunk counts.
Prepare hand-authored handles from the fixture's known generation/ID before reset,
or use equivalent test-only setup; no unmeasured bootstrap request may overlap capture.

Reviewed implementation conditions:

- Count the server's 128 KiB socket scratch zero-fill. Count 4-byte marker scratch
  initialization or label it uninstrumented, never zero by omission.
- Reader/handler lifecycle records start, normal exit and guard drop synchronously.
  Require equality before snapshot inside block_on; runtime-drop cancellation
  invalidates an exact interval. Do not retain product JoinHandles, add notifications,
  wrap futures or change spawn/join behavior. Harness-owned server joins/yields are
  permitted and bounded.
- Count socket EOF as a zero-byte completion. Classify the final marker
  read_exact error by reference before preserving the same result/`?`; one unmatched
  four-byte UnexpectedEof attempt is valid only when independent conservation proves
  no partial marker/record. Any other unmatched operation at an attempt-instrumented
  site invalidates a successful case. Selected socket-result counts are not attempt
  instrumentation and do not claim cancellation coverage.
- Record harness outcome separately; error/timeout runs make no exact claims.
  Use an external per-process watchdog: the existing client's zero-progress loops
  may prevent cooperative timeouts from firing. Do not repair those product loops.
- New rpc diagnostics feature forwards types/client/server hooks without enabling
  allocator-diagnostics. Hooks may not panic through unchecked arithmetic or change
  ignored errors. Keep B0.1/B0.2 captures unchanged.
- Expected file handles use root generation bytes and fixture ID 2 in native byte
  order; WRITE verifier equals generation bytes. Validate reply XID against captured
  request XID without adding a random dependency. Transcript overflow is failure.
- Report handle construction/conversion, verifier copies, internal/harness duplex
  copies and header-field codecs as uninstrumented. Do not present the ledger as
  an exhaustive physical-copy total. Include a focused multi-fragment receive test
  to exercise growth with nonempty old logical content.

This slice establishes logical IO, explicit copy/zero-fill and growth observations,
not process/whole-RPC peak retention, allocator usable bytes, syscall counts,
throughput or latency. Existing B0.2 scoped retention evidence remains separate.
Subsequent stages add bounded framing/directory/slow-peer/concurrency cases and
normal-allocator timing/overhead. Exact general backing lifetimes require a
separate feasible instrumentation contract, not guessed drop points.

## B0.4 Event-Visible RPC Allocation Cohort

Measure the same eleven RPC cases in a separate rallocator executable with both
allocator-diagnostics and rpc-diagnostics enabled. Do not describe this as hooks-off.
No product changes or new dependencies. Only harness observer/refactoring, a bounded
pure tracker, executable registration and focused tests may change.

Use one stopped snapshot, not repeated Retain snapshots or recorder restart.
Repeated-snapshot/nested-suppression guarantees are unverified and unnecessary here.
Start recording before runtime/filesystem/transcript/duplex/input creation. A
synchronous harness-only observer allocates one nonzero fixed-size marker at each
setup, output-retained, output-dropped and post-runtime-teardown boundary. Keep
all four marker owners in fixed storage through Stop; black_box their addresses.
Match their observed allocation events by address/layout and generation, not
size alone, and require correct ordering. Marker storage is explicit harness cost.
No formatting, output or tracker allocations occur before Stop. Decode, validate
and report only after stopping; existing golden/payload/quiescence validation may
allocate during capture and must be labeled harness/validation work.
Use safe nonzero Box markers; no new unsafe code. Checkpoints include their own
marker allocation. Setup is after fixture construction and logical-ledger resets
but before server spawn; retained is after quiescence and golden/ledger/output
validation; dropped immediately follows drop(output); teardown immediately follows
drop(runtime). Keep command-line owners created before recording alive past Stop.

At output-retained observe the owned LOOKUP handle or READ data address/length/
capacity without copying; other cases have no owned output payload in this scope.
Borrowed or zero-capacity LOOKUP/READ outputs are failure, not zero retained memory.
Require its recorded generation live at retention, its exact matching free after
retention and before output-dropped, and absence thereafter. Observe actual explicit
drop; do not infer drop from a response callback. Preserve default B0.3 output.

Bound the capture to 4,096 event slots on one thread and at most 4,096 live tracked
origins. Reject extra threads, loss, gaps, count mismatch, duplicate live addresses,
unmatched frees, malformed layout, checkpoint mistakes and checked-arithmetic/table
overflow. Treat an insufficient bound or unobserved-origin free as a failed capture,
not truncated success. This bound is a harness choice, not product policy.
Require first sequence 1, last sequence equal to total events, vector/count/thread
totals equal, and all four marker generations increasing and still live at Stop.

Reconstruct monotonic allocation generations and requested-byte live totals from
the initially empty event-visible cohort. Report allocation/free event counts,
requested event bytes, largest observed request, event-order requested-byte peak
and checkpoint live totals, with actual event/ring headroom and marker cost.
Include runtime, fixtures, transcripts and validation in these cohort totals.
Do not label them product-only or exact process memory. Report configured slots,
not invented recorder byte overhead. Realloc event pairs are not distinct observed
realloc calls; same-size realloc and overlap remain unknown, as do usable bytes,
unobserved initial process allocations, hidden recorder costs and instrumentation
overhead. An event-order peak does not include hidden old/new overlap.

Falsification covers loss/order/layout/overflow, fresh generations on address reuse,
marker order/identity, output premature/missing/double free and unobserved-origin
free. A realloc-shaped pair must not produce an exact realloc/overlap claim.
Independent capture must retain source/build configuration and raw measurements;
golden wire and logical ledger validation remain mandatory in every case.
Use the external process watchdog from B0.3.
Register the executable with both features required and gate its subprocess tests
on both. Compare all eleven B0.3 cases before/after in default/hook configurations;
wire and semantic output must remain unchanged. Any scheduler-dependent chunk/call
count differences must still obey the existing conservation checks, not be silently
normalized as protocol changes.

## B0.5 Bounded Wire and Scheduling Workloads

Harness-only continuation, normal allocator, hook-off/on configurations, no new
dependencies or product/CI changes. Reuse Server/process_socket, ReadFs/WriteFs/
ListFs, independent B0.3 wire builders and diagnostic semantics. New workload
code may have its own small executable for externally watchdog-bounded captures.
Do not modify B0.2/B0.4 fixtures or broaden their metric claims.

- Independently vary sender record fragments and receiver read segmentation.
  Exercise all two-fragment split positions of a NULL call, empty fragments,
  READ/WRITE at 4 and 1024 KiB with multiple explicit request fragments, one-byte
  receive chunks for bounded small cases and odd-sized chunks for 1 MiB cases.
  Reassemble captured requests independently and compare with known RPC bytes;
  response wire stays a single fragment, as in the current server.
- Keep any chunking adapter in test code, safe and allocation-free per poll.
  Do not add product reads/writes or infer TCP segmentation from sender write calls.
  Label raw hand-built request lanes as server-path coverage, not client encode
  measurements. Account extra record markers separately from logical RPC bytes.
- Exercise READDIR and READDIRPLUS at 0/3/100 entries with bounded pagination.
  Check independent entry names/cookies/attributes/handles, exact terminal EOF,
  no omission/duplication, and retained owned results after connection shutdown.
  No exact directory backing-memory claim without origin tracking.
  Use an additive/local directory fixture with nonzero mtime, leaving existing
  ListFs defaults unchanged: zero mtime yields a zero verifier that the current
  server wrongly rejects on resumed READDIR. Record a separate zero-verifier
  BAD_COOKIE case as a known product defect, never a successful valid request.
  Echo the returned verifier on every resumed request.
- Prove two-connection progress while one peer stalls mid-request or holds a
  large response unread. Bound channel sizes and delays; assert the healthy peer
  completes before releasing the slow peer, then both finish correctly. Use
  current-thread runtime and aggregate attribution only, never per-task TLS identity.
- Add real ephemeral loopback TCP READ/WRITE lanes with Nagle enabled/disabled
  explicitly on both ends. Exercise the same process_socket path; no privileged
  mount, external server, fixed-port cleanup or published-peer claim.
- Bound malformed-input cases to a partial marker and a small advertised body
  (at most 8 KiB) followed by EOF. Verify cleanup without hanging or calling
  malformed work successful. Check the current client's fragmented-reply rejection
  as an unsupported correctness result, not a speedup or released-peer proof.

All processes have external watchdogs; cooperative timeouts alone are insufficient.
Capture buffers, pages, connections, bytes and delayed steps have explicit finite
harness bounds. Validate output/wire independently in both feature configurations;
require normal quiescence for successful ledger claims, classify error/cancellation
outcomes separately, and do not reset counters beneath pending IO. Conserve
successful selected/forwarded/record-body bytes with fragment markers accounted.
Do not assert scheduler-dependent call counts, fairness thresholds or product limits.
Huge advertised-length/OOM, numeric overload policy, multi-thread attribution and
independent released-peer interoperability remain untested, not passing.

Reviewed implementation conditions:

- Patterned READ sources and WRITE sinks must use position-dependent nonzero data,
  validated in both feature configurations; a uniform payload cannot expose
  reordering. Keep B0.3 fixture defaults unchanged and do not call its 0xAA-specific
  consumed hook for patterned data.
- Safe server-side chunking uses initialized sub-ReadBuf storage; no unsafe,
  per-poll allocation/copy or injected WouldBlock. Advance scripted chunk state
  only after Ready(Ok(nonzero)). It controls outer socket reads only; internal
  duplex/read_exact coalescing remains uninstrumented.
- NULL splits include both empty-edge fragments (0 through body length inclusive).
  An all-empty record belongs to expected-error cases. Keep successful peers'
  write sides open until every expected reply is read.
- Partial marker/body plus EOF may return Ok from process_socket; independently
  classify incomplete record/no reply as expected truncation, not protocol success.
  Fragmented-reply client rejection must equal RpcError::FragmentedReply; discard
  its unread connection afterward.
- For slow readers, use a small explicit duplex capacity C and read only the reply
  marker before letting the healthy peer finish. Body length greater than C plus
  bytes read proves the slow reply cannot finish. Before release, hook-on send-body
  completed bytes must be exactly the healthy reply's body. Drive peers independently;
  no sleeping or awaiting the stalled writer on the healthy path. Do not extend
  this capacity proof to TCP kernel buffering.
- Loopback lanes accept exactly one ephemeral connection and set/read back nodelay
  on both ends before Server/TokioIo creation. Use the real NFS client.
- Directory page concatenation must match a fixture-derived independent model,
  including fileids, names, cookies, attributes and generation-bound handles.
  Require nonempty nonterminal pages, bounded page count, only final EOF, at least
  two pages for 100 entries, and encoded resok within count/maxcount. Do not copy
  the server's page-boundary formula. Validate retained results after runtime drop.

## B0.6 Normal-Allocator Timing

Harness-only, no dependencies/product changes. Reuse the eleven B0.3 cases and
its validated request/output path, with default no-op synchronous observer callbacks
immediately before client operation and immediately after successful decoded output.
The timing observer suppresses printing; preserve existing observer boundaries and
B0.3/B0.4 semantics. Both lanes use the system allocator, identical source/compiler/
profile and harness; only rpc-diagnostics differs. Never enable allocator-diagnostics.

- Measure client encode through decoded response on one current-thread duplex
  connection with one request per fresh runtime/fixture. Include transcript taps,
  backend work, scheduling and ordinary RPC allocations in the interval. Exclude
  runtime/fixture construction, server spawn, post-response validation/golden/ledger
  reporting, output drop and teardown. Label this cold-connection, tapped-harness
  latency, not steady-state service time, syscall latency or network throughput.
- Preallocate bounded sample storage before iterations. Use 32 discarded warmups
  and 512 measured operations per process; permit only smaller bounded test counts
  if useful, never unbounded CLI values. Validate every warmup and measured result
  through the existing byte-golden/payload/conservation path outside timing. Abort
  on any error or missing/duplicate timing boundary; no successful partial report.
  Do not reset counters beneath pending IO; each complete harness invocation retains
  its existing reset/quiescence protocol.
- Retain integer nanosecond samples in operation order. Report nearest-rank
  p50/p95/p99, sample count, sum and operation-window ops/s; bulk cases also report
  payload bytes per summed operation-window second. These rates exclude setup and
  validation gaps and are not sustained wall-clock throughput. Separately report
  total loop wall time/rate including those gaps. No timing subtraction.
- Capture five serial paired release runs per case with alternating off/on order.
  External watchdog 60 seconds per process; no concurrent builds/benchmarks.
  Store raw samples, exact commands/build fingerprints/source and binary hashes,
  environment and pairing order in one current artifact. Independently reconstruct
  quantiles/rates and per-pair hook-on/off latency ratios; report spread and noise,
  not statistical significance or an acceptance threshold. Near-zero/noisy overhead
  is inconclusive, never a speedup. Ratios characterize the entire feature lane,
  not isolated hook CPU cost; recorder overhead remains unmeasured.
- Focused tests falsify quantile/rate calculation, finite CLI bounds, timing
  boundary validation and successful small subprocess runs with both feature modes.
  Compare the eleven existing B0.3 outputs in both configurations and retain B0.4
  semantics. Apply repository gates; independent Verify captures and Review inspects.

Reviewed implementation requirements:

- Separate target directories for off/on binaries, package nfs3_tests only, no
  all-features/workspace timing build. Fingerprints must exclude allocator features;
  verify different hashes and recheck before each pair to prevent lane overwrite.
- Stage observer timestamps; commit a sample only after run_observed returns Ok
  and exactly one start/end pair is validated. Assert sample capacity unchanged.
  Convert Duration nanoseconds with checked u128-to-u64 conversion; use checked
  u128 sums/rate intermediates and integer floor rates. Reject zero durations.
- CLI warmups 0..=32, measured samples 1..=512. Loop wall time covers measured
  iterations only, before first run through last committed sample; warmups excluded.
  Payload bytes mean READ/WRITE data length, not wire bytes. Nearest-rank positions
  for 512 samples: 256th, 487th and 507th.
- Timed work also includes server first poll/startup, transcript first-touch faults,
  duplex growth/backpressure, argument construction and success mapping. Only the
  spawn call is excluded, not execution of its task. Post-send work included varies
  with scheduling; this is a full-lane comparison, not isolated hook cost.
- Use monotonic std::time::Instant; identical tracing configuration, preferably no
  subscriber. Preserve B0.3 not-measured labels; document B0.6's separate observer
  use. Steady-state/TCP timing and allocator-recorder overhead remain unknown.

## B0.1 Active Verification Evidence

Current source is HEAD `d1f731b07c7e7b664b3a6108231d3ce3d10a92a3` in
`/home/vaiz/repos/my/copilot-worktrees/nfs3/vaiz-redesigned-dollop`, rebased onto
`00ea3aa2f2d4027bb9ead8da2ee71d647e88f40e`. Prerequisite fixes and safe harness
are in the base. The committed nine-file baseline source delta is supplemented
by the B0.2 executable modification (+478/-2) and the 123-line untracked test
`crates/nfs3_tests/tests/opaque_allocator_baseline.rs`. Lead docs are also dirty.

After harness adoption, Implement passed host gates and default-feature debug/release
mirror runs: each passed 34 readonly and 14 readwrite cases. Explicit MCP builds
selected matching server/harness profiles; NFS3_MIRROR_SERVER selected the prebuilt
server. Each run was bounded to 180 seconds; neither timed out. All four observed
server children were reaped and all temporary fixture directories removed.

After B0.2, Implement reran required host gates; Verify independently captured both
default and retention lanes in debug/release and externally asserted their event
schedules and ledgers. Default B0.1 remains a six-event, zero-loss fixture with the
same one-byte/padding semantics. Full B0 acceptance remains blocked.

### Reproduction

Implement Rust MCP calls used the current worktree's Cargo.toml, toolchain `1.97.0`,
and `locking_mode: "locked"` (metadata used `"frozen"`):

| Tool | Additional arguments | Result |
| --- | --- | --- |
| cargo-metadata | `no_deps: true` | Pass |
| cargo-check | `workspace: true, all_targets: true, all_features: true` | Pass before edits |
| cargo-test | `package: ["nfs3_types"], test: "opaque_baseline", test_args: ["--nocapture"]` | 3 passed |
| cargo-test | Same, `features: ["opaque-diagnostics"]` | 5 passed |
| cargo-test | `package: ["nfs3_types"], lib: true, features: ["opaque-diagnostics"], testname: "opaque_diagnostics", test_args: ["--nocapture"]` | 2 passed |
| cargo-test | `package: ["nfs3_tests"], bin: "opaque_allocator_baseline", test: "opaque_allocator_baseline", features: ["allocator-diagnostics"], test_args: ["--nocapture"]` | 3 unit + 2 subprocess passed |
| cargo-build | `workspace: true, all_targets: true, all_features: true` | Pass |
| cargo-test | `workspace: true, all_features: true` | Pass including doctests |
| cargo-clippy | `workspace: true, all_targets: true, all_features: true, warnings_as_errors: true` | Pass |
| cargo-fmt | `toolchain: "nightly-2026-07-25", all: true, check: false` | Pass; scoped formatting |
| cargo-machete | Current workspace path, `fix: false` | Pass |
| cargo-deny-check | `workspace: true, all_features: true, which: ["all"]`; no toolchain parameter | Pass; existing duplicate-syn warning |

`git diff --check` passed. Broad host checks were not independently repeated.

Current captures were built through Rust MCP `cargo-build` with the current
manifest, `toolchain: "1.97.0"`, `locking_mode: "locked"`,
`package: ["nfs3_tests"]`, `bin: "opaque_allocator_baseline"`,
`features: ["allocator-diagnostics"]`, `profile: "dev"` and separately `"release"`,
`target: "x86_64-unknown-linux-gnu"`, and absolute
`target_dir: "<current-worktree>/target"`.

Verify independently ran `./target/x86_64-unknown-linux-gnu/{debug,release}/opaque_allocator_baseline`,
once per profile without arguments and once with `--retention`, in separate
serial processes from the worktree root. No environment overrides; each had a
30-second bound and exited 0 with empty stderr.
Linux x86_64, kernel `7.3.4-1-cachyos`, Intel i7-13700K. Build metadata confirms
Rust 1.97.0 `2d8144b78` (2026-07-07); binary fingerprint confirms `rustflags: []`
and features `["allocator-diagnostics"]`.
Single-thread, rallocator 0.4.0/seismograph 0.5.0; no seed/timing.

Active SHA-256 evidence:

| Input/artifact | SHA-256 |
| --- | --- |
| Executable source | `dceba74b48c6149a80c39b93a6f131ae3722b1b6703de172e3b15c3702620e3d` |
| Untracked integration test | `8752a6f3b18dbc90b7d0a0743d3386179a67385f53b39cdd7d41a6dc163a9aa0` |
| Captured debug executable (subsequently rebuilt) | `fa82a820fee6344d96d85c61fd26eb4c468ec698f1f24a68cd78da404be0d100` |
| Captured release executable (subsequently rebuilt) | `4bd989678029933941f7b5c791ae8d9c934301def4401a1bf1892d3554d89344` |

### B0.2 Independent Capture

Verify externally asserted every event, address pair, sequence, generation,
size/capacity, live checkpoint, ledger field and unknown label in both profiles.
Release raw output excerpt (header omitted):

```text
events=8 retained=8 lost=0 threads=1; sequence_start=1; sequence_end=8
checkpoint=decode_4KiB thread=1 sequence=1 kind=Allocation generation=1 address=140108178292736 requested=4096 capacity=4096 alignment=1 scoped_requested_live=4096 retained_output=4096
checkpoint=decode_64KiB thread=1 sequence=2 kind=Allocation generation=2 address=139833288687616 requested=65536 capacity=65536 alignment=1 scoped_requested_live=69632 retained_output=69632
checkpoint=decode_256KiB thread=1 sequence=3 kind=Allocation generation=3 address=139833288949760 requested=262144 capacity=262144 alignment=1 scoped_requested_live=331776 retained_output=331776
checkpoint=decode_1024KiB thread=1 sequence=4 kind=Allocation generation=4 address=139833290784768 requested=1048576 capacity=1048576 alignment=1 scoped_requested_live=1380352 retained_output=1380352
checkpoint=drop_1024KiB thread=1 sequence=5 kind=Deallocation generation=4 address=139833290784768 requested=1048576 capacity=1048576 alignment=1 scoped_requested_live=331776 retained_output=331776
checkpoint=drop_256KiB thread=1 sequence=6 kind=Deallocation generation=3 address=139833288949760 requested=262144 capacity=262144 alignment=1 scoped_requested_live=69632 retained_output=69632
checkpoint=drop_64KiB thread=1 sequence=7 kind=Deallocation generation=2 address=139833288687616 requested=65536 capacity=65536 alignment=1 scoped_requested_live=4096 retained_output=4096
checkpoint=drop_4KiB thread=1 sequence=8 kind=Deallocation generation=1 address=140108178292736 requested=4096 capacity=4096 alignment=1 scoped_requested_live=0 retained_output=0
opaque=Ledger { encode_payload: Count { attempts: 0, attempted_bytes: 0, completions: 0, completed_bytes: 0 }, encode_padding: Count { attempts: 0, attempted_bytes: 0, completions: 0, completed_bytes: 0 }, decode_payload: Count { attempts: 4, attempted_bytes: 1380352, completions: 4, completed_bytes: 1380352 }, decode_padding: Count { attempts: 0, attempted_bytes: 0, completions: 0, completed_bytes: 0 }, payload_zero_fill: Count { attempts: 4, attempted_bytes: 1380352, completions: 4, completed_bytes: 1380352 }, padding_zero_fill: Count { attempts: 0, attempted_bytes: 0, completions: 0, completed_bytes: 0 }, lost_updates: 0 }
fixture=passed; scoped_initial_live_set=empty; scoped_requested_live=0; scoped_requested_peak=1380352; scoped_retained_output=0; final_drops=4
process_initial_live_set=unknown; process_live=unknown; process_peak=unknown; process_retention=unknown; usable_allocator_bytes=unknown; realloc_overlap=unknown; physical_copy_bytes=unknown; cross_thread_attribution=unknown; instrumentation_overhead=unknown; timing=not_measured
codec_counts=logical_io_and_explicit_zero_fill; pool=absent; shared_slices=absent; clone_accounting=absent
```

These are scoped output requested-byte metrics, not exact global memory, physical
copy counts, performance or overhead measurements.

### B0.3 Independent RPC Capture

On the current HEAD plus B0.2/B0.3 worktree delta, Verify passed all 22 serial
default/hook release cases under a 15-second parent-process watchdog. External
assertions checked all reported counters, frame lengths, byte conservation,
selected outcomes, growth and lifecycle. Full byte-golden comparison and payload
validation ran inside the executable, not an external parser of printed output.

Rust MCP build inputs: current manifest, Rust 1.97.0, locked, package nfs3_tests,
bin rpc_baseline, release, with separate target directories `target/b03-default`
(no features) and `target/b03-hooks` (`rpc-diagnostics`). Verify used host Linux
x86_64 builds and `target/b03-{default,hooks}/release/rpc_baseline CASE`, cases
`null`, `getattr`, `lookup`, `read-{4,64,256,1024}` and `write-{4,64,256,1024}`.
System allocator, current-thread runtime, one connection/request, deterministic
0xAA payload; fingerprints show features []/[rpc-diagnostics] and no rustflags.
The rpc feature does not activate allocator-diagnostics.

Independently checked representative values (not an exhaustive physical-copy ledger):

| Metric | READ 1024 KiB | WRITE 1024 KiB |
| --- | ---: | ---: |
| Request wire bytes | 76 | 1,048,660 |
| Reply wire bytes | 1,048,708 | 164 |
| Selected socket bytes / completed forwarding | 76 | 1,048,660 |
| Explicit test-backend READ copy bytes | 1,048,576 | 0 |
| Test-backend WRITE consumed bytes | 0 | 1,048,576 |
| Diagnostic transcript copy bytes | 1,048,784 | 1,048,824 |
| Server scratch zero-fill bytes | 131,072 | 131,072 |

Both showed one selected EOF, zero selected errors/counter loss, one growth
observation and normal socket/reader/handler starts/ends/drops of one each before
snapshot inside block_on. Scheduler-dependent selected call counts are not invariants.
Verify also passed the previously missing Rust MCP workspace all-feature release
tests with target x86_64-unknown-linux-gnu, Rust 1.97.0 and locked resolution.

B0.3 capture source/artifact SHA-256 (later observer changes are recorded below):

| Path | SHA-256 |
| --- | --- |
| types/src/rpc_diagnostics.rs | `4282463a0a99bdd8516e717f09059d7e756db410adf8f94825e20b1dbd3310ae` |
| tests/src/rpc_baseline.rs | `f6c645275fdfcae63e85a8c320696fc015b5d62ca34dfa853dc80661a54d5d84` |
| tests/src/bin/rpc_baseline.rs | `de5b49f29887705df51c312339a2939dad05d60e9187eda45bc8a971894aef77` |
| tests/tests/rpc_baseline.rs | `4e9f9e139075dd293009c8a9f30bcbd36da1e91afe0e489602939c977bf1ca83` |
| Default release executable | `af0bbc85af356167566b2229f262919bf8dc3246a72970e061bd5180d18d0e1c` |
| Hook release executable | `79b51178d3738c7078d5e6ba8422e4c4c4badb51d1c54d3bca46d1f7a07a69af` |

The four source paths abbreviate crates/nfs3_types and crates/nfs3_tests; they
are untracked and must be retained with the 15 tracked B0.3 changes (+180/-3).
B0.2 executable/test source hashes remain unchanged. No dependency, lockfile or
CI changes. Exact process/backing lifetimes, actual relocation, syscall counts,
overhead and performance remain unmeasured.

### B0.4 Independent Allocation Capture

Verify independently reconstructed all eleven raw traces, including original
per-thread recorder totals/loss, global/vector counts, sequences 1 through N,
generation/layout pairing, marker boundaries and output final frees. All captures
passed under separate 15-second parent watchdogs. All 22 default/hook B0.3 old/new
stdout comparisons were byte-exact. No broad gate was inferred from those comparisons.

Rust MCP build: Rust 1.97.0, locked, package nfs3_tests, bin rpc_allocator_baseline,
features allocator-diagnostics,rpc-diagnostics, release, host Linux x86_64,
target directory target/b04-allocator. Run
`./target/b04-allocator/release/rpc_allocator_baseline CASE` for the eleven B0.3
cases, one request/current-thread runtime per process. No rustflags.

The current raw stdout, commands, hashes and independent reconstruction are in
`/home/vaiz/.copilot/session-state/797fe6bb-7a9b-4c02-a5e5-fc222498425e/files/b04-allocator-capture.json`,
SHA-256 `879f12d75c5e386c1b3b75eb524d303b546023215535b8da9919e716297e8204`.
This is the single active raw capture, not a series of historical samples.

All byte values below are event-visible requested bytes, including runtime,
fixtures, transcript/validation work and the checkpoint's own marker.

| Case | Events | Allocation/free events | Event-order peak | Setup | Retained | Dropped | Teardown |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| NULL | 127 | 66/61 | 2,264,500 | 2,127,182 | 2,122,930 | 2,123,187 | 1,068 |
| GETATTR | 133 | 69/64 | 2,264,648 | 2,127,182 | 2,122,914 | 2,123,171 | 1,068 |
| LOOKUP | 147 | 76/71 | 2,264,800 | 2,127,182 | 2,122,930 | 2,123,171 | 1,068 |
| READ 4 KiB | 143 | 74/69 | 2,274,360 | 2,127,182 | 2,127,010 | 2,123,171 | 1,068 |
| READ 64 KiB | 143 | 74/69 | 2,581,560 | 2,250,062 | 2,249,890 | 2,184,611 | 1,068 |
| READ 256 KiB | 143 | 74/69 | 3,564,600 | 2,643,278 | 2,643,106 | 2,381,219 | 1,068 |
| READ 1024 KiB | 143 | 74/69 | 7,496,756 | 4,216,142 | 4,215,970 | 3,167,651 | 1,068 |
| WRITE 4 KiB | 139 | 72/67 | 2,276,893 | 2,123,070 | 2,122,914 | 2,123,171 | 1,068 |
| WRITE 64 KiB | 139 | 72/67 | 2,584,093 | 2,184,510 | 2,184,354 | 2,184,611 | 1,068 |
| WRITE 256 KiB | 141 | 73/68 | 3,567,049 | 2,381,118 | 2,380,962 | 2,381,219 | 1,068 |
| WRITE 1024 KiB | 141 | 73/68 | 6,712,693 | 3,167,550 | 3,167,394 | 3,167,651 | 1,068 |

Largest observed request is 1,048,832 bytes in every case (harness transcript bound).
The 4,096-slot ring has at least 3,949 slots headroom; one thread and zero loss.
Four 257-byte markers account for 1,028 live bytes at Stop; the remaining 40 bytes
belong to one unclassified observed allocation and are not subtracted or called
a leak. LOOKUP output generation 53 frees at sequence 128, between retained/drop
markers 127/129; READ output generation 52 frees at sequence 124 between its markers.

Current allocator source SHA-256:
`cc4b466f33bd8f2ecab74a59ff268dfb00dbd6b4c257af32bb159f5642f44774`;
integration test `a8b658acffa8425904736cfd8f7686db1036e514d792b9e34ace317386001d79`;
binary `b7d5981e2dfb1d4916aef22f69e9acc3ec72f6b9a6285b3d5b13f314f53c04f6`.
Pure tracker SHA-256:
`02a9e6a234cf6198153fcc1fe09376712e8714c87846dd2f19851a1f449d9e2a`.
The B0.4 capture's observer-refactored rpc_baseline.rs hash is
`4f1a077bbcec589b544b8740b87c955f2ace6e870c4ed33a753fa059a8766d41`;
its B0.3 output compatibility was independently checked.
Exact B0.4-only textual diff of this already-untracked file is unavailable;
Review must inspect the full current source, not pretend a reconstructed diff.
Real realloc/overlap, collector byte overhead, usable/global initial memory and
performance remain unknown.

### B0.5 Independent Wire Capture

Independent Verify and actual-source Review accepted the bounded workload stage.
Each of the default/rpc-diagnostics release configurations produced 110 valid,
3 expected-malformed, 1 unsupported and 1 known-product-defect outcomes. All 230
processes finished within their external 20-second watchdog. Classification is
not interchangeable with successful valid-protocol execution.

Current raw output, commands, build/source/binary hashes and external assertions:
`/home/vaiz/.copilot/session-state/797fe6bb-7a9b-4c02-a5e5-fc222498425e/files/b05-wire-capture.json`,
SHA-256 `44b66b91a035c1478db6f8993e540972d595bf479e6bf0ac3d6d227101eba8b0`.
External parsing checked reported conservation, growth, lifecycle, pagination and
progress. Exact wire bytes, patterned payloads and retained directory contents
were checked inside the harness, not independently recovered from stdout.

100-entry READDIR/PLUS used 4/17 pages. Slow-reader proof was 4,224 > 1,024+4;
both hook-on slow cases completed only the healthy 24-byte reply body before
release. Both TCP endpoints read back requested NODELAY settings. The five
untracked workload files plus lib registration were reviewed in full; no exact
whole-tree stage-only diff is claimed. B0.2 source and B0.3/B0.4 artifact hashes
matched; older B0.2 binary capture hashes do not identify subsequently rebuilt files.
No timing, exact backing-memory, TCP segmentation or independent-peer claim follows.

### B0.6 Independent Timing Capture

Independent Verify and actual-source Review accepted this bounded timing lane.
All 110 release processes completed under 60-second external watchdogs: five
alternating off/on pairs per eleven cases, 32 discarded warmups and 512 validated
samples per process. External reconstruction matched counts, sums, nearest-rank
quantiles and floor rates; samples were positive and measured loop wall time
exceeded summed operation time. Binary hashes were checked before every pair.

Current raw samples, pair order, environment, source contents/hashes, fingerprints,
exact execution commands and independently derived statistics are in
`/home/vaiz/.copilot/session-state/797fe6bb-7a9b-4c02-a5e5-fc222498425e/files/b06-timing-capture.json`,
SHA-256 `0fd3b177de0372ecd1f145b5b8669e5d1ddfc1f5c016fbfe79c47795777e0ae1`.
Final JSON parsing, source identity checks and 110-run/55-pair counts passed.

New explicit Rust MCP reproduction builds matched both captured binary hashes:
package nfs3_tests, bins=true, release=true, locked, Rust 1.97.0, current manifest,
separate target/b06-default and target/b06-hooks, features []/[rpc-diagnostics].
Exact new requests/results are in independently_reproduced_builds; original
invocations were not independently recorded. Reproduction establishes binary
identity without repeating timing samples. No allocator-diagnostics feature.
Execute `target/b06-{default,hooks}/release/rpc_timing CASE --warmup 32 --samples 512`.

These are cold-connection, tapped-harness operation-window latencies, not TCP,
steady-state throughput or isolated hook cost. Table p50 columns are medians of
five per-process p50s; ratios are independently paired, not ratios of those medians.

| Case | Off p50 (us) | On p50 (us) | Paired p50 on/off median [min, max] |
| --- | ---: | ---: | --- |
| NULL | 34.94 | 35.34 | 1.026 [0.962, 1.063] |
| GETATTR | 5.00 | 6.53 | 1.297 [1.256, 1.374] |
| LOOKUP | 5.28 | 6.70 | 1.275 [1.227, 1.993] |
| READ 4 KiB | 36.83 | 39.15 | 1.063 [0.995, 1.105] |
| READ 64 KiB | 15.73 | 17.30 | 1.118 [1.066, 1.174] |
| READ 256 KiB | 298.41 | 296.38 | 1.003 [0.986, 1.009] |
| READ 1024 KiB | 1206.93 | 1218.18 | 0.999 [0.978, 1.015] |
| WRITE 4 KiB | 5.35 | 8.73 | 1.686 [1.360, 1.796] |
| WRITE 64 KiB | 17.56 | 44.48 | 2.593 [2.486, 2.714] |
| WRITE 256 KiB | 51.80 | 156.96 | 3.060 [2.873, 3.182] |
| WRITE 1024 KiB | 735.32 | 1134.19 | 1.557 [1.500, 1.582] |

Raw p95/p99, summed-operation-time ratios and rates, loop-wall rates and all
pair spreads remain in the artifact. No significance, acceptance threshold or
speedup follows. Near-parity/spanning-parity cases are inconclusive; ratios below
one, including lower tail quantiles in some hook-on runs, are not improvements.
WRITE ratios include a diagnostic-only full-payload 0xAA validation scan in the
test backend, plus transcript and codec counters; they are not product-only hook
overhead. NULL being slower than GETATTR, and READ 4 KiB slower than READ 64 KiB,
remain unexplained. The i7-13700K's 24 allowed logical CPUs were unpinned;
heterogeneous cores, scheduling and allocator/process effects may contribute but
have not been established as causes. Recorder overhead and steady-state/TCP timing
are still unknown.

Focused off/on tests, 22 byte-exact B0.3 compatibility comparisons, the existing
B0.4 test, strict Clippy, nightly formatting, debug/release all-feature workspace
tests, all-target/all-feature build, machete and deny passed on final source.
Deny retains the existing duplicate-syn warning. A transient release TCP timeout
passed retry with the same executable/features/20-second bound, followed by the
focused and full release suite. No product/assertion/watchdog changes were retained;
firewall involvement is unproven.

### Host MSRV Validation

User-installed toolchain alias `1.95` reports rustc 1.95.0
`59807616e` (2026-04-14). Independent Verify ran Rust MCP with current manifest,
locked resolution, workspace=true and separate `target/msrv-1.95` output.
Frozen no-dependencies metadata passed. No source changes or timing recapture;
the accepted Rust 1.97 measurement binaries/artifacts were preserved.

| Rust MCP configuration | Result |
| --- | --- |
| cargo-build, all_targets=true, default features | Pass |
| cargo-build, all_targets=true, all_features=true | Pass |
| cargo-test, default features | Pass after authorized README feature guards |
| cargo-test, all_features=true | Pass, including diagnostics |
| cargo-test, all_features=true, release=true | Pass, including diagnostics |

Default executable tests and doctests passed, including hooks-off timing and
bounded wire workloads. The user authorized fixing the pre-existing optional-runtime
README doctests: hidden feature guards and fallback mains now permit feature-off
compilation without ignoring examples or enabling dependencies by default.
Client doctests passed separately with default, Tokio-only and Smol-only features
on both Rust 1.95 and 1.97.0 (two tests, zero ignored in each of six runs).
The enabled examples still compile their real runtime code. Repair validation used
frozen resolution; Rust 1.95 default workspace tests passed in target/msrv-1.95.

Before/after source identity for the initial read-only MSRV run matched: tracked patch hash
`ce4ece3294c62ac145a905e3b4b39e7a11d987cc` and all 16 untracked content hashes.
The subsequent authorized repair changed only 12 hidden README lines; existing
rpc-diagnostics documentation was preserved. Rust 1.97 all-target/all-feature
precheck and diff whitespace check passed. Existing dirty source remains unstaged.
Host MSRV allocator compilation/execution and default doctests are established;
external gates remain open.

### Outstanding Gates

Known product correctness finding: a zero directory mtime yields a zero verifier;
the server rejects its own verifier on resumed READDIR with BAD_COOKIE despite
valid continuation. B0.5 records this separately; fixing product behavior is outside
baseline-only authorization.

**Owner-approved policy/scope (implemented in merged
[PR #173](https://github.com/Vaiz/nfs3/pull/173), adopted and passing on combined
B0 inputs):** allow exactly `MIT`,
`Apache-2.0`, `BSD-3-Clause`, `Unicode-3.0`, and `Unlicense`, and prepare a separate
prerequisite PR remediating the five reported existing dependency advisories.
No advisory ignores, blanket exceptions or policy weakening are authorized.
Verify actual advisory sources/patched versions before selecting dependency changes.
For the example-only chrono replacement, the user accepts Jiff's default timestamp
display. No custom compatibility formatter or formatting tests are wanted.
See [Docs semantics](research.md#dependency-gate-policy).

Host Rust 1.95 coverage, including repaired default doctests, is recorded above.
Windows x86_64/Linux aarch64/macOS aarch64 baseline
coverage on 1.97 is not established. Baseline CI and
independent peer interoperability remain open. Safe mirror execution is no longer
blocked: merged PR #174 and current host evidence are recorded above.

Current CI runs tests with all features, not a separate feature-off lane; its MSRV
build uses default features and does not cover the new allocator dependencies.
Dedicated baseline coverage remains pending; the merged harness PR did not add it.
Uncovered B0 workloads, exact full-memory metrics, allocator-recorder overhead and
numeric criteria remain outstanding. Scoped output generation/final-drop accounting,
bounded RPC logical IO/explicit copy/growth observations and normal-allocator
feature-lane timing are now measured, within the explicit limits above.
Process initial-live-set/cross-thread ordering, exhaustive physical-copy and exact
general backing-lifetime accounting remain missing. Whether exact end-to-end retention
requires product redesign remains unresolved; do not infer exactness or authorize
behavior changes from this fixture.

Refresh actual source/status before reusing this evidence; changed inputs
invalidate affected checks. Current next action is in [lead state](state/lead.md).