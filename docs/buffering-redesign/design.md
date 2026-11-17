# NFS3 Buffering Redesign

Current proposal: [design-v3.md](design-v3.md), revision D3.3, documenting user-approved
B1-B4 correction directions after second review. Baseline-only instrumentation with
rallocator/Rust 1.95 is authorized in [baseline.md](baseline.md); no buffering
behavior or prototype implementation authorized. Prior findings in [review.md](review.md).
The rest of this file preserves
D2/C2 for review traceability; it is superseded, not an implementation contract.

Revision: D2 with user directions C2 (supersedes C1). Date: 2026-11-17. Status: REVISE
after D2-RV1 review; architecture revision pending. Authorized scope: DESIGN ONLY. No Rust changes,
dependency installation, baseline instrumentation, or behavior changes authorized.
Evidence: [research.md](research.md). Measurements below are acceptance proposals,
not results. Verify inspected HEAD `6b2d3aba001c78504f4029a85c199d6e3d920161`,
branch `bufs`, with empty tracked diff and no untracked source. Snapshot evidence
and D1-V1 findings are in [validation-plan.md](validation-plan.md).

## User Directions C2

Explicit user directions, 2026-11-17, take precedence over the historical D2
proposals below:

- Increasing MSRV is allowed.
- Pools are allowed. Evaluate reuse benefits and retained capacity; do not impose
  a no-pool or zero-idle-retention requirement.
- Unsafe initialization is allowed with a soundness argument and measurements
  proving it is worthwhile against a safe alternative. Permission is not proof
  of correctness or benefit; neither may be assumed.
- Preserving contiguous VFS writes is not a goal. Evaluate segmented VFS APIs
  without a mandatory contiguous bridge or API-compatibility constraint.
- `bytesbuf` is the user's recommended candidate. Investigate current applicable
  versions and direct reuse first; compare alternatives with evidence. There is
  no user preference for `bytes` and no approved dependency choice.
- Resource defaults should prioritize performance and low operational friction.
  Support conservative server configuration without making it the assumed default.
  Propose specific limits and tradeoffs for user approval before adopting them.
- Performance gates are not set. Withdraw D2's numerical gates and allocation/copy
  thresholds, rather than treating them as provisional requirements. Measure the
  opportunity across alternatives; a modest improvement is not a stopping target
  or a reason to exclude a potentially better approach.
- Ask the user before imposing any new restriction. Research findings, candidate
  settings and reviewer suggestions are proposals, not authorization. Distinguish
  protocol/correctness obligations from project policy and optimization choices.

The D2 resource table, reservation ledger, chunk/detachment rules, performance
threshold table and no-pool assumptions below are withdrawn historical proposals,
not an active contract. Their dependent assertions (including zero backing after
drop when a pool may retain it) require revision. Measure live payload, retained
views and reusable pool capacity separately. Existing research and review evidence
remains useful but does not approve any replacement architecture. Before behavior
changes, propose evidence-backed acceptance criteria, obtain user agreement and
Verify assessment, and submit the revised design for review.

## Constraint Provenance and Correction C1

The user explicitly permits increasing MSRV (2026-11-17). Rust 1.88 describes the
current manifest, not an immutable requirement. Select and justify a new minimum
against candidate dependencies and supported targets; no manifest change is made
in this design-only correction. Reassess newer bytesbuf and rallocator versions.
An isolated harness is an option, not a requirement imposed by MSRV preservation.

The lead introduced the following proposals without user approval: fixed resource
ceilings/timeouts, 4/64 KiB chunk sizes, the 4 KiB/4x detachment policy, full 8 MiB
record credit retention, preservation of contiguous VFS writes, 64 KiB outgoing
fragment default, dependency selection, exclusions of pooling/unsafe initialization,
and numerical performance/allocation gates. They must not be presented as user
constraints. Review R1-R11 rejects several of these choices; the D2 text below is
retained for traceability, not an accepted implementation contract. In particular,
retained-value charging, VFS segmentation, list storage, and send fragmentation
must be revised. No provisional target is locked by this correction.

Actual workflow requirements are design-only until explicit implementation
authorization, valid wire interoperability, source-backed alternatives, justified
resource bounds, safe ownership, independent verification/review, and repository
validation. API compatibility is explicitly not a goal. Safe ownership does not
justify a blanket prohibition on all unsafe library internals or audited alternatives.
D2-V2 acceptance covers the earlier measurement-plan feasibility only, not C1
dependency/toolchain changes or the rejected architecture.

## Goals and Current Path

Reduce payload copies and peak/live/retained backing capacity, without allocation
proportional to an untrusted advertised length before receiving its bytes. Preserve
valid RPC/XDR/NFS3 wire interoperability; public API compatibility is not required.
Use safe ownership, bounded work, and explicit copies rather than self-reference or
an unconditional zero-copy claim. Backend storage, kernel socket memory, and caller
application allocations are separate accounting domains, not silently excluded
from end-to-end measurements.

Source-inspected copy/allocation map (not measured):

| Path | Present ownership and costs |
| --- | --- |
| Server TCP receive | `tcp::process_socket` owns a 128 KiB scratch Vec, copies into a 256 KiB Tokio duplex buffer, then reads into a record Vec. |
| Server record assembly | `IncompleteRpcMessage::recv` resizes/zero-fills by each advertised fragment length; Vec growth may move earlier bytes. No local cumulative record/fragment-count bound. |
| Decode | `Unpack(&mut impl Read)` cannot return source-backed views. `Opaque<'static>` allocates/zero-fills and copies; `Vec<u32>` reserves advertised element count; `List<T>` grows until terminal boolean. Padding values are not checked. |
| Async server dispatch | Complete Vec moves into a spawned handler; opaque args own separate Vecs. `handle` keeps its cursor in scope across handler await. Source inspection does not establish exact allocator lifetime. Reply queue is unbounded; transaction limits do not bound pre-decode/reply bytes. |
| Server READ/send | VFS returns Vec, wrapped without payload copy in Opaque; `pack` copies it into one complete RPC Vec; `write_fragment` emits marker then body. |
| Client WRITE/send | Borrowed Opaque is copied into a whole marker/message Vec. |
| Client receive | Rejects non-final fragments, zero-fills advertised whole reply Vec, copies opaque output again. `NotFullyParsed` can retain the entire record. |
| Generated codec | Struct/enum derives in `nfs3_macros/src/lib.rs` use Read/Write traits and unchecked size/count additions. |
| Short I/O | Client default read/write-all loops do not reject zero progress; new paths must return UnexpectedEof/WriteZero, not spin. |

Controlling files: `crates/nfs3_server/src/{tcp.rs,rpcwire.rs,rpcwire/messages.rs}`,
`crates/nfs3_client/src/{rpc.rs,io.rs}`, `crates/nfs3_types/src/xdr_codec/`, and
`crates/nfs3_macros/src/lib.rs`. Local VFS advertises 1 MiB READ/WRITE preferences
and maxima; `read` returns Vec and `write` borrows a contiguous slice.

## Alternatives and Dependency Decisions

Pinned source/test/benchmark links, selection rationale, licenses, and source
limitations are in research D1-R1. No candidate benchmark is an NFS result.

| Candidate | Decision and tradeoff |
| --- | --- |
| Bounded contiguous Vec + owned decoder | Simplest fallback. Bounds risk and requires little metadata, but growth and decode/send copies remain; tiny views can pin full records. Use as the comparison baseline, not the target architecture. |
| Contiguous Bytes/BytesMut record | Comparison candidate with growth/retention tradeoffs, not a selected architecture or blanket exclusion. Evaluate against segmented alternatives under C2. |
| bytes | Alternative to compare, not the user's preference or an approved dependency. Compare ownership/control costs with bytesbuf and std ownership (R9); account backing capacity rather than view length. |
| bytesbuf | User-recommended candidate to investigate for direct reuse using current applicable versions. MSRV increases and pools are allowed. D1's 0.5.0-only evaluation is insufficient for selection; assess current API, runtime integration, ownership, retention and measured performance before recommending a choice. |
| tokio-util | Adapt checked framing/state-machine tests, not source. Reject direct codec dependency: contiguous eager reservation, single-frame semantics, Tokio-only client coverage, and 0.7.20 lockfile upgrade are unnecessary for this design. |
| http_headers | Adapt validated-owned versus borrowed-view distinction, small-value copy policy, owner-release/allocation tests, and source/sink separation. Reject direct dependency (unpublished) and HTTP grammar/limits. XDR crosses arbitrary binary chunks and async task ownership. |
| Serde | Adapt explicit transient/borrowed/owned contracts; no new dependency or XDR format adapter. Owned decoded handles avoid derive lifetime propagation through spawned tasks. |

Local abstractions must be justified against direct dependency reuse; their shape
is not fixed before bytesbuf evaluation. Pools are allowed. Unsafe initialization
requires soundness evidence and measured benefit against a safe alternative.
Do not introduce custom machinery or exclude alternatives without explaining the
tradeoff; ask before making any proposed restriction binding.
All researched reuse candidates are MIT or MIT OR Apache-2.0; normal
dependency notices and cargo-deny policy must be checked. No source copying is
planned; copying source later requires license/attribution review, not relicensing
under the workspace's Unlicense.

## Ownership and APIs

The following are proposed API concepts, not claims about existing methods.

- `Record`: bounded sequence of immutable chunk owners plus total logical length.
- `Backing`: Bytes plus original allocated capacity, allocation identity, and a
  shared budget ticket. Views clone the backing owner and range, never shed its
  ticket. Capacity is charged once until the last view drops, not per clone.
- `DecodeCursor`: owns/moves a Record, tracks chunk/offset/remaining bytes, and
  shares only selected payload backings into decoded values. Dropping the cursor
  releases all unselected chunks before awaiting the handler.
- Owned decoded `Opaque`: small contiguous owned value or a bounded sequence of
  backing ranges. No implicit `Deref<Target=[u8]>`, no public bare Bytes escape.
  `chunks()` yields slices, `as_contiguous()` returns Option, and explicit checked
  `to_contiguous(limit, budget)` copies only when necessary. `len()` is logical.
- Borrowed encode input (`OpaqueRef<'a>`) supports existing borrowed client input
  without a forced owning conversion. Owned Opaque can expose a borrowed view.
- Replace `Unpack(Read)` with a bounded `Decode(&mut DecodeCursor)` contract;
  results own their payload handles and have no input lifetime. `DecodeLimits`
  supplies field limits, collection expansion, depth and node budgets.
- Replace infallible `packed_size()` with checked encoded length. The encoder
  emits owned metadata chunks and borrowed/shared payload segments; explicit
  buffered-to-Write compatibility helpers may copy and are not hot-path APIs.
  Send plans have checked total length and exact segment cursors.

Small fields of at most 4 KiB (names, handles, auth) are copied into exact-sized
owned storage at decode, even when contiguous. This knowingly pays metadata-copy
cost to prevent retaining a 64 KiB transport block for a filename. Bulk READ/WRITE
opaque fields share ranges. A public subview operation applies this same rule
when its result is at most 4 KiB and would retain more than 4x its logical length;
  it must be explicitly budgeted with fallible data allocation. Empty views retain no backing. Larger views
retain only intersected blocks, never the entire Record's descriptor list.

Lifetime examples:

1. Server WRITE: record -> decoder -> owned args retaining payload storage;
  evaluate a segmented VFS write API that holds valid views across await without
  mandatory coalescing. A contiguous adapter may be compared where useful, not
  required for compatibility. Backend storage copies are counted separately.
2. Server READ: check/clamp request count to configured max; reserve reply budget
   before VFS call; adopt returned Vec without copying when capacity fits the
   backend contract; encode scalar metadata separately and share payload through
   queue and socket writes. Last writer/owner drop releases capacity.
3. Client READ: return owned segmented Opaque independent of the call future;
   retaining it retains only its intersected chunks and budget ticket. Client
   can be dropped safely while output remains. Small extracted views detach.
4. Client WRITE: send plan borrows caller bytes while `call(&args).await` runs;
   no spawned `'static` task receives this borrow. Server reply plans must be
   owned before channel send. Borrowing a local owned Opaque across an await is
   ordinary safe Rust; moving a reference alongside its owner is not required.

All spawned owners must be Send + 'static and shared backing Sync. Add compile
checks for Tokio and smol futures; never repair lifetime errors with unsafe.

Each admitted record gets one shared reservation owner. Every surviving backing,
detached value and send-plan allocation retains that owner, even after the client
or task drops. The whole 8 MiB reservation is conservatively retained until the
last associated value drops; no partial credit transfer/refund in D2. Thus a client
32 MiB domain holds at most four such record lifetimes. Before a new call, a
nonblocking reservation attempt can return ResourceLimit if the caller retained
earlier outputs. This is intentionally conservative and avoids unsafe credit
transfer or deadlock on the caller's own values. Independent client instances use
separate domains unless explicitly configured to share one; no implicit process
bound is claimed for arbitrary numbers of clients. Physical capacities and reserved
credits are separate reported metrics; keeping a tiny copied value can retain
credits, but must not retain its old transport allocation.

## Receive State Machine

Shared runtime-independent record assembler; Tokio server and both client IO
adapters supply reads. No server scratch/duplex forwarding stage.

`Header { bytes[4], filled } -> Admit -> Body { remaining, final_bit } -> Header`
for non-final fragments, or `Complete -> Header` for the next record.
Errors/timeouts/cancellation enter `Closed`, dropping partial storage and permits.

- Read header incrementally into four stack bytes. Mask final bit separately;
  checked-add fragment length to cumulative advertised record length; reject
  over-limit headers before reading/reserving their bodies. Count zero-length
  fragments too. Final zero-length fragment is valid; empty record fails RPC decode.
- On the first accepted marker, acquire per-connection and global record credits
  before body allocation. A blocked admission stops reading body, not writing
  replies. Later fragments use the same record reservation.
- Start with a 4 KiB initialized receive chunk; subsequent full chunks are
  64 KiB. Allocate the next chunk only after previous capacity has filled and
  another body byte is needed. Never allocate from the advertised full length.
  Use fallible Vec reservation for data, initialize only bounded capacity, and move/freeze
  without growth. A freeze must not copy; measure it. All allocator-returned
  capacity counts, including unused tail. C2 permits an alternative uninitialized
  receive path only with soundness evidence and measured benefit; this initialized
  path remains a comparison candidate, not an imposed requirement.
- Fragment boundaries do not seal chunks; sender's one-byte fragments must not
  create one chunk/descriptor per byte. Short TCP reads also do not seal chunks.
  Final record seals the last chunk. For B>0 body bytes the chunk count is
  N = 1 + ceil(max(0, B-4096)/65536); N=0 when no body has arrived, with at most
  one initial 4 KiB pending-read allocation allowed. At 2 MiB, 33 origins have
  nominal capacity 2 MiB+4 KiB. Actual returned capacity is charged separately.
- Receiver segmentation and sender fragmentation are independent. Scalars and
  XDR padding may cross either boundary. Byte-by-byte network delivery changes
  read count, not allocated chunk count.
- EOF with no partial header at record boundary closes cleanly; EOF in a marker,
  body, or unfinished record is truncation. Interrupted I/O retries; read zero
  during an exact operation is UnexpectedEof. No resynchronization after framing
  failure; close instead of draining unbounded attacker-controlled lengths.

## Decode and Wire Semantics

Decode only complete bounded records. This avoids partial object rollback across
await and keeps time-limited network assembly independent of CPU parsing.

Read u32/u64 in place if contiguous, else copy at most eight bytes to stack.
Before opaque view/copy, validate checked length plus padding against remaining
record and schema maximum. Check every padding byte is zero even across chunks.
Validate booleans/enums/discriminants; consumed record bytes must equal total.
Authenticate body <=400 before allocating/decoding it; bound AUTH_SYS name/group
fields per protocol. Apply NFS handle/name/path and procedure-specific bounds from
RFC 1813, not only the generic record cap. WRITE count must equal data length;
READ response count must equal data length and not exceed requested count.

Collections cannot reserve from wire count. Check minimum wire-size feasibility,
checked host `size_of` expansion and per-record node/depth budgets before bounded
growth. Decode linked lists iteratively. For zero-minimum-size generic types,
the node and memory budgets still apply. Descriptor arrays/collection growth
must be fallible and counted; old plus new capacity during growth counts toward
peak. Enforce checked arithmetic in derived enum/struct sizes and BoundedList.

After a fully consumed frame, RPC/application errors may leave the stream reusable.
On framing, truncation, timeout, malformed decode, or partial write failure, close
or poison. Client marks itself busy/poisoned before first I/O and clears only after
a full accepted response; dropping a call future leaves it poisoned. Distinguish
pre-I/O validation/resource errors, which leave the stream untouched. Error values
retain only bounded diagnostic prefixes (256 bytes), offsets and length, not
whole malformed records.

## Limits, Backpressure, and Cleanup

Status under C2: the numerical defaults and reservation scheme in this section
are withdrawn, not approved settings. Redesign for performance and low friction,
with separately configurable conservative policies; ask before adopting limits.

Defaults are deployment policy, not RFC maxima; make configurable with invariant
validation. Raising them requires measured budgets. FSINFO must advertise values
consistent with configured payload limits; do not silently advertise 1 MiB while
allowing backend overrides above the cap. Valid peers exceeding local policy get
bounded rejection/connection closure, not huge allocation.

| Resource | Default and rationale |
| --- | --- |
| READ/WRITE payload | 1 MiB, matching current FSINFO defaults and benchmark ceiling. |
| Fragment and record wire body | 2 MiB each; permits 1 MiB data plus metadata/auth; aggregate checked on each marker. |
| Fragments per record | 4,096 including empty; permits reasonable fragmentation, bounds marker CPU overhead. |
| Receive allocation | 4 KiB initial, then <=64 KiB, independent of advertised length. |
| Largest protocol allocation | <=1 MiB; only contiguous VFS WRITE scratch or adopted READ payload may exceed 64 KiB. Other individual requests <=64 KiB. Actual returned capacity must be accounted/checked. |
| Decode expansion | <=1 MiB owned metadata/collection capacity, <=16,384 nodes, depth <=64; failed decode releases all. |
| Record ownership budget | 8 MiB reservation including receive/send backing, decoded expansion, scratch, descriptor/refcount/control allocation and transient growth. Allocation sites charge actual capacity before publication. |
| Per connection | <=8 records across assembly, execution, queue and current write; <=64 MiB reservation. Reply channel <=8 entries; writer keeps the same permit until final byte sent/dropped. |
| Server listener domain | <=128 accepted connections; <=32 admitted records / 256 MiB record reservations, plus <=64 KiB charged fixed state per connection (<=8 MiB). Multiple listeners must share the admission domain for this to be a process-wide protocol bound. Runtime/allocator/kernel/backend overhead is reported separately. |
| Client | One call active; 32 MiB retained-buffer domain per client by default, shareable budget domain for many clients. Returned owners retain credits; insufficient budget before I/O returns ResourceLimit rather than waiting on the caller's own retained values. |
| Time | 30 s absolute record-assembly deadline after first marker byte (not reset by trickle), 30 s idle-progress timeout, 30 s write deadline, 120 s total active request deadline. Configurable for slow deployments. |
| Reuse | C2 allows pools. Evaluate retained capacity, reuse benefit and configurable reclamation; no zero-retention or no-pool default has been approved. |

Conservative simultaneous ledger (MiB): receive 2.0625 + send 2.0625 + decoded
expansion 1 + adopted backend READ output 1 + WRITE scratch 1 + descriptor/control/
transient-growth allowance 0.5 = 7.625. Remaining 0.375 is headroom, not additional
unlimited allocation. READ output and WRITE scratch usually do not coexist, and
shared payload in send/receive is charged once; retaining both allowances avoids
depending on those optimizations for admission safety. Enforce each category and
the aggregate 8 MiB; allocate scratch only after decode budget is checked, and
drop scratch/args before reply publication. Count old+new collection allocations
during growth inside the 0.5 MiB allowance; reject growth that cannot fit. Large
host collections must use bounded pages (<=64 KiB allocations) or reject; no
automatic oversized Vec growth. Stages must prove these lifetimes/invariants.

Distinguish three quantities: reserved credits, requested/returned Rust backing
capacity plus accounted control allocations, and allocator usable/resident memory.
The 8 MiB and 264 MiB domain ceilings bound the second quantity using the first,
not OS RSS or allocator page rounding. Charge actual Vec capacity; the allocator
may internally consume more and this must be measured/reported, not claimed capped.
Per-connection/control limits require measured size/allocation audits on each target.
Executor worker stacks, runtime-global scheduler state, socket kernel buffers and
allocator pools live outside these ceilings and cannot be inferred from them.

Explicit fallible sites: data/metadata Vec reserve, scratch allocation, descriptor
pages and collection pages via try_reserve before initialization. Validate requested
size, charge returned capacity before publication, and drop/reject an unexpectedly
oversized capacity. Reservation request itself is bounded; there may be transient
allocator rounding outside Rust capacity accounting. Bytes promotions, Arc/control
blocks, Tokio task/channel allocation may use infallible library allocation and
abort on actual OOM. No guarantee of recovery from process OOM is made. Their
sizes/counts must be audited and precharged conservatively from the control reserve
before use; otherwise that stage is blocked. Inject failures only at supported
fallible sites, verify cleanup there, and do not pretend to inject allocator aborts.

Acquire credits in a fixed order (connection then global), never while holding
the writer lock. Reader, handler task set and writer are independently driven;
one task owns each socket half. The reader cannot block writer progress by awaiting
body admission. A bounded join/task set ties connection lifetime to its children;
on closure cancel pending assembly/handlers, close queue, drop unsent replies and
await task cleanup when futures yield. Async cancellation cannot terminate a
blocking/non-cooperative backend poll; no hard memory-release deadline is promised
for such a backend. Document cooperative backend requirement, test yielding pending
futures, and keep permits charged while non-cooperative work survives. A slow writer retains permits and naturally stops intake.
TransactionTracker is duplicate suppression, not the memory admission mechanism.

An application backend may allocate unrelated storage or return an oversized Vec;
protocol cannot bound arbitrary backend internals. Document a VFS return-capacity
contract <= configured payload cap and enforce before adoption. Reject oversized
returns with bounded server failure; no late giant copy to hide their capacity.
Bound READ count before calling backend, and bound directory budgets before
constructing responses. Backend work cancellation/partial writes do not promise
transaction rollback; preserve NFS stable-write/commit semantics.

## Send State Machine

Validate checked encoded total and record limits before writing any marker. Build
small metadata chunks plus owned/shared or call-borrowed payload segments; do not
serialize bulk bytes into a whole-message Vec. Coalesce only small metadata under
its budget. Default outgoing fragment <=64 KiB; final bit only on last fragment.
Each fragment length reflects encoded bytes, independent of segment boundaries.
Empty RPC-invalid messages are rejected before send.

Single writer maintains marker offset, segment index/offset and fragment remaining
across short writes. Zero write -> WriteZero; error after any byte poisons/closes
connection. Do not interleave records from different handlers. Sequential slice
writes work on both client runtimes; vectored writes may be an optional measured
optimization, never required for correctness. Held views survive awaits and are
released after use. Kernel copies are not described as eliminated. Cancellation
closes a partially written stream, never resumes at the next record marker.

## Measurement Contract Proposed for Verify

Baseline before behavior changes is mandatory. Existing Criterion benchmarks cover
NULL/GETATTR/LOOKUP, 4/64/256/1024 KiB READ/WRITE and directory operations but do
not establish payload copies, retained capacity or allocation maxima. Authorize a
separate instrumentation-only stage if counters are absent. Do not change behavior
while capturing baseline. Save Git commit plus dirty diff/untracked source, compiler,
allocator, features, target, OS/CPU, runtime, release profile, seed and commands.

### rallocator Integration Proposal

Use the user's recommended rallocator for a diagnostic measurement lane, subject
to later instrumentation authorization. D1-R2 in research.md pins 0.4.0 source and
API. User permits an MSRV increase; its >=1.95 requirement is not a rejection
criterion. Reassess workspace test/benchmark integration versus an isolated
measurement executable on simplicity, allocator isolation and reproducibility.
Do not require a separate manifest/lockfile merely to preserve Rust 1.88. Pin the
selected version and toolchain after evaluation and update the MSRV gate consistently.
No harness, dependency, toolchain installation or manifest change is made here.

rallocator replaces the global allocator. Baseline/candidate must both use the same
pinned rallocator, compiler, event policy and harness for that lane. Keep a second
normal-allocator lane for production-like performance; never compare rallocator
baseline to system-allocator candidate. Record unsampled events without stacks by
default, check zero lost events, bound capture windows and record draining costs.
Stacks are optional diagnostic runs. OS-thread identity is not async task identity;
instrument protocol origin/request labels explicitly without holding thread-local
suppression across awaits. Preallocate/bound collector storage and audit recursion.

Events provide requested-size counts/maxima, not actual usable capacity, unique
origin retention or exact peak. Event addresses can be reused; size-changing realloc
appears as free/alloc and may conceal transient coexistence. Use monotonically
assigned origin/lifetime IDs and explicit old+new realloc accounting in protocol
hooks; never infer exact peak from event ordering alone. Native allocator snapshots
are diagnostic, not live-object counts. Exact retained capacity comes from backing
owner creation/range/drop counters, not rallocator pools or unmatched events.
Counters for copies and zero-fill remain mandatory. Required exact allocation
gates use a validated loss-free tracker and origin ledger, not rallocator alone.
If event loss, attribution ambiguity or harness compatibility prevents exactness,
mark that metric BLOCKED and fix instrumentation before behavior changes. Tool
availability and tracking overhead remain unverified; no measurements claimed.

Measure per operation and peak whole-workload: payload bytes copied categorized by
transport forwarding, growth, codec, coalescing, backend and test transport; total
allocation count/bytes, largest allocation, unique peak live backing, final retained
backing, metadata/refcount cost, initialized/zero-filled bytes, throughput and p50/
p95/p99 latency. RSS is supplementary only. Allocation tracking must include
deallocation and shared owner lifetime; copy sites require explicit counters, not
inference from allocations. Measure instrumentation overhead in paired runs.

Workloads: metadata RPCs; READ/WRITE at 4/64/256/1024 KiB; contiguous/many-fragment
record layouts; independent TCP delivery sizes 1/3/4096/65536 bytes; splits at every
marker/scalar/padding boundary; header-only huge lengths; valid partial bodies;
4,096/4,097 fragments including empty; slow readers/writers; concurrency 1/8/32
active records and 128 idle connections; retained 1/64/4096-byte views; repeated
large-then-small traffic; cancellation during every receive/send state.

Withdrawn D2 thresholds (historical comparison only; C2 sets no performance gates):

| Metric | Threshold |
| --- | --- |
| Payload copies, protocol layer | Server/client send and bulk decode: 0 payload bytes. Server receive forwarding/growth: 0. Contiguous VFS WRITE bridge: <=1x payload. Kernel/backend/test-duplex copies separately counted. |
| Malicious length | A rejected >2 MiB first marker causes 0 body allocation; rejecting a later marker causes no additional body allocation and frees partial origins. Accepted header-only input allocates at most one 4 KiB chunk. |
| Valid receive capacity | Nominal capacity 4 KiB+(N-1)*64 KiB for N>0, actual charged capacity reported and constrained by budget; <=33 origins at max record. Independent of sender fragmentation/TCP delivery. |
| Retention | Only views meeting the >4x detachment rule must lose original origins after other original owners drop; detached data capacity <=4 KiB each, control separate. Always-copied small decoded fields lose original origins. At exactly 4x sharing is allowed. Test 1/64/4096 bytes and cross-chunk views. After final owner drops: 0 protocol backing; allocator pools reported separately. |
| Allocation count | Provisional metadata <=baseline+8 per operation; large receive <=3N+8 backing/control allocations plus separately counted decoded metadata. N uses total wire body, not payload only. No growth with fragment/short-read count at fixed bytes. Calibrate before behavior changes. |
| Largest/peak | All absolute limits above required. Provisional 1 MiB READ/WRITE peak <=75% baseline, separately per client endpoint, server endpoint and whole workload using identical domains. Backend/kernel/test transport reported separately. |
| Throughput/latency | Provisional metadata/large throughput >=95% baseline, p99 <=110%. Improvement target >=10% large throughput is aspirational. Ratios apply only to comparable completed work. |
| Reproducibility | 20 serial paired runs alternating order, warmup and >=10,000 completed ops per latency sample. Bootstrap paired run-level ratios with 10,000 resamples. 95% CI lower throughput >=0.95 and upper p99 <=1.10; width <=5 percentage points, otherwise inconclusive. Save raw samples and timing boundaries. |

Measure instrumented/uninstrumented overhead in paired runs. Above 3% throughput
or 5% p99 overhead, use uninstrumented timing and separate counter runs without
mathematical correction. These numerical measurement-method choices also remain
proposals, not user-approved restrictions. After baseline evidence, propose new
acceptance criteria and obtain user agreement plus Verify assessment before behavior
changes. No D2 cap, copy target or performance ratio is binding or an optimization
ceiling. Review must assess the updated contract, not enforce withdrawn thresholds.

Pre-change fragmented-client/cancellation inputs may fail or hang: record bounded
timeouts as correctness baselines, not fabricated throughput. Compare perf only
where both implementations complete equivalent work. New features require absolute
bounds/correctness evidence instead of false improvement ratios.

## Wire and Repository Gates

Beyond symmetric round trips: hand-authored RFC marker/XDR fixtures with expected
bytes; independent packet builder/parser for fragment/corruption scenarios; exact
wire capture comparisons; integration with an independent NFS peer where available.
Cover zero padding, auth 400 boundary, count mismatches, overflow, unknown enum,
huge collections, truncation, all header splits, cross-chunk fields, multi-record
streams, zero-progress IO, cancellation/last-owner release, slow peer budgets and
WRITE stable/commit behavior. Independent peer unavailability is explicit, not pass.

Required future MCP gates: cargo-check all_targets/all_features before Rust edits;
clippy workspace/all_targets/all_features/warnings_as_errors; nightly cargo-fmt
(Verify check-only); cargo-test all_features; cargo-build all_targets/all_features;
cargo-machete; cargo-deny-check. Follow locked CI debug/release and the selected MSRV
matrix, Tokio/smol examples, generated codecs and all callers. Build test_mirror_fs
through MCP before running with CI-equivalent arguments. Windows x86_64, Linux
x86_64/aarch64, macOS aarch64 remain required CI evidence; WASM support is not
established by current CI and cannot be claimed. Missing tools/targets are blockers
for their gates, not successes. No Rust gates executed in this design-only stage.

## Migration and Bounded Stages

All stages require explicit user authorization of the accepted revision/scope.
Each has independent Verify evidence plus Review verdict before the next stage.

0. Baseline-only instrumentation in existing tests/perf helpers and benchmark file,
  with narrowly feature-gated hooks at production copy/allocation sites and the
  rallocator integration selected after C1 reassessment; record unchanged behavior, isolate
  counters, capture independent baseline and lock acceptance thresholds. No
  buffering behavior change. MSRV increases are permitted; exact dependencies and
  toolchain must be recorded in the stage scope. Instrumentation needs explicit
  instrumentation-stage authorization; no tool installation assumed.
1. Bounded shared-record/DecodeCursor primitives and owned/borrowed opaque APIs in
   nfs3_types; tests for capacities, wire bytes, lifetime/drop and cross-chunk access.
   No production transport switch yet. Temporary compatibility paths explicitly
   copy and are removed after migration; no claim of end-to-end improvement here.
2. Migrate primitive/list codecs, RPC/auth/NFS/mount/portmap types, checked encoding,
   all derive branches in nfs3_macros, and direct codec callers. Bound schema fields,
   collections, padding and encoded length. Keep workspace compiling per stage.
3. Migrate server messages/rpcwire/tcp to direct bounded assembly, task admission,
   owned handler args, VFS bridge, reply send plans and shutdown. Migrate handlers,
   file handles, backend read-capacity contract, FSINFO and directory limits;
  evaluate segmented VFS writes without a contiguous-API constraint while
  preserving protocol stable-write semantics.
4. Migrate client RPC and io adapters, poisoning/error retention, fragmented replies,
   borrowed send plans and retained output budgets. Update tokio/smol and nfs/mount/
   portmapper callers; remove old whole-message APIs when no callers remain.
5. Complete tests, examples (client and server), cargo_nfs3_server/mirror/memfs,
   test VFS implementations, public README/API documentation and changelogs as
   needed; remove temporary compatibility paths. Full baseline comparison, gates,
   independent interoperability and final review.

Stage scope may be split for atomic compilation but architecture changes return to
design review first. Do not claim completion with unresolved validation or review.

## Open Decisions and Evidence

- C2 withdraws numerical gates and imposed resource defaults. Investigate bytesbuf,
  pools, segmented VFS and worthwhile unsafe initialization; propose evidence-backed
  settings and acceptance criteria, asking the user before imposing restrictions.
  Baseline and actual dependency builds await implementation-stage authorization.
- Source revision is recorded in validation-plan.md; specialist MCP execution and
  independent peer availability are unverified. Rallocator harness compatibility,
  allocator overhead and per-target control-allocation audit remain future gates.
  No model substitution is permitted.
- D2-RV1 review verdict: REVISE, blockers R1-R11 in review.md. C1 records user
  permission to increase MSRV and corrects constraint provenance; it does not
  resolve those blockers or approve revised dependencies. No implementation authorized.