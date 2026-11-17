# NFS3 Buffering Redesign D3

2026-11-17. Revision D3.3: user-approved correction directions and baseline stage.
User approved B1-B4 design corrections and baseline-only instrumentation with
rallocator and Rust 1.95. [baseline.md](baseline.md) defines that authorized scope.
Buffering behavior/prototypes remain unauthorized. D3.3 documents corrections,
not an independent architecture acceptance or measured proof. Entry point:
[design.md](design.md). Prior D3.2 review findings remain in review.md for tracing.

Evidence: [research.md](research.md) D1-R1/R2 and D3-R4/R6;
[review.md](review.md) D2-RV1 R1-R11;
[validation-plan.md](validation-plan.md). Verify's source snapshot:
`6b2d3aba001c78504f4029a85c199d6e3d920161`, branch bufs, no source changes at
inspection. Document edits are not a fresh Git status inspection.

## User Directions and Decision Status

- MSRV may increase. Current bytesbuf is the user's recommendation, not bytes.
  Recommend Rust 1.95 for bytesbuf 0.14.0, subject to dependency/CI verification.
- Pools allowed. Unsafe initialization requires soundness evidence and demonstrated
  benefit. No blanket unsafe ban; safe ownership is still required.
- Contiguous VFS compatibility is not a goal. API migration is expected.
- Favor performance and low friction; allow conservative server configuration.
  Ask before imposing restrictions. Numeric resource defaults are not approved.
- Performance gates are unset. D2 ceilings, copy/allocation/performance thresholds,
  reservation sizes and deadlines are withdrawn. A modest win is not a reason to
  stop investigating a better approach.
- Baseline-only Rust instrumentation and Rust 1.95/rallocator integration are now
  explicitly authorized. All buffering behavior/prototypes remain separately gated.

Architecture recommendations below are proposals. Protocol validity and memory
safety are correctness obligations; deployment caps and regression tolerances are
policy choices requiring agreement. Feasibility assessment does not approve policy.

## Current Copy and Ownership Map

Source observations, not measured allocation/copy counts:

| Path | Present costs and lifetime |
| --- | --- |
| Server TCP | 128 KiB scratch -> 256 KiB duplex -> record Vec adds forwarding copies. |
| Assembly | IncompleteRpcMessage::recv resizes/zero-fills from advertised fragment length before reading body. Vec growth may move earlier bytes. |
| Decode | Unpack(Read) cannot retain source ranges. Opaque<'static> allocates/zero-fills/copies. Vec<u32> reserves wire count; List grows Vec. Padding values ignored. |
| Server async | Complete Vec moves into spawned task; decoded payload owns a second Vec. Unbounded reply queue; transaction limit does not bound receive/queued bytes. |
| READ send | VFS Vec moves into Opaque, then is copied into whole encoded RPC Vec. |
| Client WRITE | Borrowed Opaque copied into whole message Vec. |
| Client receive | Single-fragment-only, eager advertised Vec, separate opaque copy; NotFullyParsed retains record. |
| IO/derive | Client all-loops fail to handle zero progress. Generated sizes and collection growth use unchecked additions. |
| Retry | TransactionLock::drop marks Completed on cancellation; completed XIDs suppress retries without replay. |

Owning paths: server tcp/rpcwire/messages/nfs_handlers/transaction_tracker; client
rpc/io/tokio/smol; types xdr_codec and nfs3/rpc; macros lib.rs. VFS advertises
1 MiB today, not an approved new ceiling. READDIR host size may exceed wire size.

## Storage Recommendation and Alternatives

Recommend direct bytesbuf 0.14.0 for owned segmented storage and cross-span access,
with a small NFS-specific accounted-memory adapter; prototype before committing.
Do not copy upstream implementations or create another segmented engine.

| Candidate | Proposed decision and tradeoff |
| --- | --- |
| bytesbuf 0.14.0 | Primary candidate: BytesView range/slices/scalars, BytesBuf append, custom blocks/metadata. MIT, Rust 1.95. Payload can remain segmented through decode/VFS/send. |
| bytesbuf GlobalPool | Measure reuse, but public API cannot enforce configurable retained capacity/trim or exact origin accounting. Not recommended as the sole server policy; this is an API limitation, not a prohibition on pools. |
| bytesbuf_io 0.14.0 | Runtime-neutral owned IO, but no Tokio/smol adapters found. Convenience reads reserve requested length; cancellation not guaranteed. Recommend local poll-based transport retaining framing state; adopt bytesbuf_io only if prototype simplifies it without hidden growth/copies. |
| bytes / BytesMut | Comparison candidate; contiguous slicing useful, but growth/coalescing and hidden backing capacity remain. Outer Arc<Bytes> adds ownership cost. Not the user's preference. |
| std Arc<Chunk { Vec, ticket }> | Simple comparator with explicit capacity, one owner allocation and moved Vec header. Arc<[u8]>::from(Vec) instead allocates/transfers. A fallback if integration costs exceed bytesbuf benefits, not selected in advance. |
| Contiguous Vec | Comparison baseline: simplest metadata, but growth/copy/retention tradeoffs. Measure rather than dismiss by label. |
| http_headers | Adapt owned/borrowed distinction and allocation/owner tests; reject HTTP grammar assumptions. Unpublished, not a dependency. |
| tokio-util / Serde | Adapt framing/error tests and borrow/ownership contracts, not implementations. Neither is an RPC/XDR drop-in or full runtime solution. |

Research supplies pinned source, symbols, tests and benchmark scenarios; their
results are not NFS results. bytesbuf multi-span to_bytes copies; Vec conversion
above BlockSize::MAX uses split_off and may copy. Multiple blocks avoid requiring
one giant allocation. BlockSize u32 is a library constraint, not record policy.
Instrument conversions, metadata, initialization, growth and coalescing explicitly.
Licenses are MIT or MIT OR Apache-2.0; preserve applicable notices/cargo-deny checks.
Workspace Unlicense does not relicense dependency source. No source copying planned.
MSRV/lockfile/CI/documentation move together if approved.

## Pool and Accounting Contract

`Payload` owns a bytesbuf BytesView; `PayloadRef<'a>` borrows a slice or a Payload's
segments. Contiguous metadata retains an Opaque-like borrowed/owned representation.
No implicit contiguous dereference for bulk payload: chunks, logical len, optional
contiguous access and explicitly named materialization are separate operations.

One origin is one physical allocation, not one view or RPC. Custom BlockMeta carries
origin ID, actual capacity and domain. Final block-owner release returns backing to
pool or allocator. Views retain intersected origins only. Metadata copies have their
own capacity charge; a retained filename never holds an 8 MiB reservation. Clones
charge their additional metadata/control, not shared bytes twice.

Provider state: free -> checked_out -> shared -> free/released. Live is the union
of checked_out/shared origins; retained-by-returned-views is a subset of live,
never an additional summand. Free and live are disjoint. A domain lock or equivalent
linearizable operation transfers the origin between states, preserving its physical
charge; counters may not temporarily undercount. Re-rent moves free to live without
allocating. Final release atomically chooses free or released. Trim detaches only
free origins under the same synchronization and never races them into a new rent.
Monotonic origin/lifetime IDs disambiguate address reuse. Cross-domain references
retain the original physical charge; receiving domains count logical work only.
Count control/span-list capacity separately; multi-view reports deduplicate origins.
Never drop a view/block whose final callback acquires the domain lock while holding
that lock: detach free blocks/accounting decisions under lock and release outside.
Outstanding origins retain the domain lifetime after listener/client shutdown;
shutdown disables new rents, and their final returns free storage rather than
recreating a closed pool. Test reentrant final-drop and cross-thread shutdown.

Use public Block/BlockRef/BlockMeta and BytesBuf::from_blocks. Provider retains the
original backing Vec when adopting/recycling; ordinary From<Vec> exposes no reclaim
API. Standard thread-safe ownership implements clone/drop balance, not a novel
refcount. Unsafe extension constructors require proof: stable pointer/capacity,
exclusive writable access, no mutation after sharing, metadata lifetime, clone/drop
balance and cross-thread reclamation. Pool return only after every view/writer has
released the origin. Miri/modelled interleavings/drop tests precede acceptance;
performance value remains unmeasured. Initialized storage is a valid comparator.

Prepare payload backing before bytesbuf construction; supply explicit blocks,
never ambient GlobalPool growth. A hard managed-allocation budget requires a known
requested Layout charged BEFORE allocation. Prototype fixed-length boxed backing
or an exact-layout allocator wrapper; do not rely on Vec reserve capacity equalling
its request. Record actual owned capacity/layout. Adopted external Vec is charged
at its already-known capacity before domain transfer; if it cannot fit, ownership
stays external and is dropped/rejected, not published as uncharged protocol storage.
Post-allocation capacity checks are detection, not a proof of an allocation cap.

bytesbuf Memory::reserve, SmallVec and runtime/control allocations are not universally
fallible. Exact managed-backing limits exclude unproven library allocator overhead;
total protocol capacity claims require a measured and audited control allocation
envelope per target before enabling such a claim. Precharge known envelope before
operations; reject before any operation whose growth cannot fit. Otherwise report
that total-bound gate BLOCKED, not covered by the payload bound. Recoverable errors
exist only at supported preflight/allocation sites; actual OOM may abort. Allocator
usable/resident bytes may exceed requested layouts and remain separately reported.

Pool proposal: reuse whole owned blocks, release surplus/oversized free blocks
under configurable retention policy and expose trim. This is the custom provider's
policy extension, not a fork of bytesbuf. Block classes, cache size, sharding and
trim hysteresis remain experiment variables. Measure lock contention/cross-thread
return; free pooled capacity remains visible after RPCs finish. Do not recover a
Vec from a borrowed view; pool owns the original backing.

Small metadata is independently owned so file handles survive many calls without
pinning transport blocks. Bulk subviews share, with explicit compact/retain choice.
Automatic copy ratio and inline size remain measurement questions, not the old
4 KiB/4x rule. Expose retained capacity; compare selective tiny-view copies with
reusable backing retention. Last-view drop may return bytes to pool, not OS.

## Types, VFS and Async Lifetimes

Keep filename3/nfspath3/auth/handles contiguous, with owned decoded and borrowed
encode forms. Replace only bulk READ/WRITE opaque fields with generic payload:

- WRITE3args<Data = Payload>, READ3resok<Data = Payload> and enclosing result use
  Data: EncodePayload for encoding. Borrowed clients instantiate PayloadRef<'a>;
  decode instantiates Payload. No self-reference to a record owner.
- Other lifetime-bearing structs retain metadata lifetimes; generated Decode impls
  target owned/'static forms. Derives gain explicit payload field bounds and
  owned-decode handling, not an impl promising arbitrary borrowed Decode.
  Cover named/tuple/unit structs, simple/complex enums and generics in compile tests.
- Encode has checked length and a send plan whose borrowed segments cannot outlive
  caller future. Server queued plans own all values. Explicit-copy Write helpers
  may ease migration but are not the payload hot path.
- DecodeCursor owns complete record view, checks remaining bytes and shares payload
  ranges. Cross-span scalars use stack scratch. Drop cursor/unselected origins
  before handler await. Spawned owners must be Send+'static; backing Sync.

WRITE (B2 correction): server VFS takes owned Payload plus the operation work token,
not PayloadRef as its primary interface. Backend can move both into blocking work
or an owned IO operation without a lifetime-forced payload copy. A borrowed helper
is optional for synchronous consumers. Physical origin charge stays with its
original domain until final drop; backend logical ownership does not transfer or
refund it. Iterate slices/vectored IO while preserving one logical write and stable
semantics. A genuinely contiguous backend explicitly materializes and reports it.
Migrate memfs, mirror, adapters, examples and test backends with this contract.

READ (B3 correction): primary VFS API takes an owned accounted writable buffer
prepared from the operation's E entitlement and returns it with initialized count
and EOF. The backend can move this buffer into owned/blocking IO. Only the actually
initialized prefix becomes immutable Payload; unfilled storage never enters a
reply. Buffer allocation happens before backend invocation through the provider,
allowing READ backing reuse. Short READ is legal: reduce requested count to an
affordable positive amount, preserving true EOF rather than inferring it from
clamping. If no data byte can fit for a nonempty read, use bounded RPC SYSTEM_ERR
before backend invocation; do not loop zero-byte successful non-EOF replies.
Directory replies similarly shorten or return TOOSMALL. External Vec adoption is
an explicitly measured fallback with backend allocation outside the pre-allocation
guarantee, not evidence that READ allocation is bounded. Backend internals/caches
remain separate domains. Preserve count/eof/stable/commit semantics.

Client results may outlive client through owning metadata/domain. New constrained
calls must not wait on credits only the caller's retained outputs can release;
return pre-IO ResourceLimit with retained-capacity diagnostics. No credit reassignment
that temporarily undercounts live memory or invalidates outputs on client drop.
For replies without a finite protocol/request bound (e.g. READLINK), an agreed
client policy bound is needed for a pre-IO guarantee. Otherwise resource exhaustion
can occur after send and must poison/close; do not promise every resource failure
is pre-IO. Numeric client bounds remain unset.

## Receive and Send State Machines

Connection-owned parser: marker bytes/offset -> fragment remaining -> next marker
or complete record. Separate absolute record bytes, fragment count, marker offset
and receive segmentation. Short reads/fragment boundaries do not each allocate a
block. Record view excludes markers; checked arithmetic precedes reserve/copy/view.

Direct socket receive into provider blocks with configurable read-ahead replaces
scratch->duplex->record. A read may contain header, bodies and later records; keep
unconsumed suffix charged to read-ahead. Read-ahead never sizes from advertised
record length. Receive only an available quantum, grow as bytes need receiving.
Merge adjacent ranges from the same origin; tiny fragments do not each force a
backing allocation. Fragment CPU/descriptor costs still need separate accounting.

Tokio candidate: ReadBuf::uninit on a writable MaybeUninit span, then commit exactly
initialized filled prefix. Proof covers Pending/error/short read, no holes/stale
bytes, exact advance, exclusive ownership and cancellation after consuming bytes.
Compare initialized in-place receive on identical loads before adopting unsafe
initialization. Smol candidate initializes its writable span in place for slice IO,
avoiding a second payload copy. Never cast uninitialized bytes to &mut [u8].

Header-only records use bounded parser/read-ahead state, not worst-case full request
credits. Reserve incremental capacity before read. Validate cumulative length and
fragment policy after each marker before body growth. Empty fragments count;
final empty fragment valid, empty RPC record invalid. EOF at untouched boundary
is clean; partial marker/body is truncation. Zero exact-read progress -> EOF;
zero write -> WriteZero; retry Interrupted. Overflow/framing failure closes rather
than drains an attacker-chosen length. Incomplete CPU work is also bounded by
configured counts/deadlines, not just allocated bytes.

Recommend one outgoing fragment when a record fits the protocol 31-bit fragment
length, streaming segments inside it. Avoid breaking released single-fragment-only
clients without materializing records. Larger logical records need multiple wire
fragments and peer compatibility handling; no smaller local ceiling is introduced.
Forced fragmentation remains an independent interoperability test/option.

Batch marker+leading metadata, use writev for metadata/payload/trailing padding
where supported. Track iovec index/offset across short writes and serialize whole
records. Non-vectored fallback compares a small tail+padding copy against sequential
writes; count copies rather than asserting zero. Test real TCP with Nagle on/off;
do not make nodelay an unstated prerequisite. Record syscall counts and read-ahead
cost, including shared blocks containing more than one RPC.

Reader, bounded handler task set and single writer progress independently; admission
never holds writer lock. State survives Pending. Client sets busy before IO; a
cancelled call poisons unless its connection-owned state machine resumes that same
operation. Partial sends close/poison, never guessed resynchronization. Server
closure cancels unadmitted/read-only yielding work and drops unsent replies. Admitted
mutations run to actual completion in server-owned work tasks with payload, token
and credits; disconnect changes reply destination, not execution ownership. Backend
blocking/detached work must return a completion signal before the owner can release
those resources. Non-cooperative work keeps its charges; no rollback promise.

## Resource Policy: Not Approved Defaults

Expose independent controls: physical allocation, fragment/record bytes, fragments,
decoded host bytes/nodes/depth, read-ahead, executing requests, queued reply bytes/
entries, per-connection live bytes, listener/global physical capacity, pool free
retention, connection count and assembly/progress/write/handler deadlines. Multiple
listeners may share a domain; arbitrary independent clients have no implicit
process bound. Allocator/kernel/runtime/backend memory is separately reported.

For configured domain physical budget G, sum of unique live + free-pool backing,
metadata/control and old+new growth overlap must stay within its accounted domain.
Per-connection C counts receive/read-ahead/decoded/reply work. Shared backing counts
once physically; each consumer still uses logical work quotas. No fixed 8 MiB
reservation follows a tiny response. Reject unsatisfiable configurations up front.

Admission proposal: separate receive-byte share from execution/drain headroom.
Complete records atomically acquire execution slots plus known reply/host expansion
entitlement, or wait without a writer lock. Do not start response allocation until
admitted. If no runnable request fits, fail/drop a bounded candidate rather than
wait forever while holding partial messages. Fair queue rotates connections;
per-peer controls configurable because NAT clients should not silently share an
arbitrary small cap. Assembly slots do not exhaust execution slots. Progress expiry
and reclaimable free pool mitigate slow peers. Exact shares/fairness settings need
approval and a concrete liveness test before implementation acceptance.

Concrete reservation model for that proof: configure disjoint receive R, execution
E, drain/control D and free-cache F shares within G. Transfer idle-cache credit
only after free blocks are detached/released, never while live views remain.
Receive uses R; each connection has at most one partially assembled record and a
bounded completed-record queue. All such records and read-ahead remain in R until
released. A complete record's operation-specific maximum additional host/output
entitlement is computed from its validated args and selected policy, including
growth overlap; acquire that entire entitlement atomically from E with an execution
slot before calling VFS. No handler acquires another entitlement while holding one.
Returned backend storage is checked against the contract before adoption; arbitrary
backend internals remain outside this guarantee.

D reserves enough accounted state for active writers and bounded failure replies;
it is never lent to body receive. Queued replies keep their E entitlement until
write/drop, so a slow writer applies backpressure. E waiting records hold only R;
E owners can finish without obtaining more R/E. If a single operation cannot fit
E, send a bounded error through D without invoking backend. Fair round-robin admission
selects fitting complete records; configured deadlines reclaim abandoned R and
slow writers. This avoids circular waits under yielding backends and progressing
or timed-out peers. All arithmetic and share consistency are checked at setup.
Non-cooperative work keeps its slot/entitlement until it actually ends, never
pretend-releases capacity. If deadlines/quotas are disabled, finite reclamation or
overload fairness for those resources is NOT promised; report that configuration
capability explicitly. User chooses whether such configurations are acceptable.
Share sizes and timeouts are still approval questions, not hardcoded defaults.

B1 correction: incremental R allocation alone is unsafe even when E has progress.
Use a completion-safety admission rule before every R grant, including read-ahead.
Each admitted record declares a maximum attainable charge M_i from configured
record/fragment policy, including origin rounding and descriptor cost; H_i is its
current held R charge. Unknown future fragments use the configured record maximum,
not merely the current marker length. Reject a marker whose cumulative demand can
never fit M_i/R; no payload allocation follows such rejection.

One concrete candidate is a Banker's-style safe-state check on resource credits:
start with free R, find a record whose remaining maximum M_i-H_i fits, simulate
its completion AND release after execution/reply drain by adding H_i, and repeat
until all admitted records have an order. Grant/admit only if such an ordering
exists. Completion claims reserve credit, not eager storage or zero-fill. Release
only actual held charges, never hypothetical unallocated bytes. Account completed
records awaiting E and shared reply backings until final release. E/D progress
and mutation-completion assumptions above are prerequisites; a stalled actual
backend does not license a fictitious release. Serialize/recheck grants under the
domain admission synchronization. A no-bound configuration cannot claim this
finite-resource guarantee. Test the review's N records larger than R/N case and
fragmented unknown totals: at least one progressing peer can finish without waiting
on another partial record. Compare cheaper grant scheduling in later prototypes;
no algorithm tuning or numeric default is silently approved by this correction.

Performance-oriented candidate: pooled reuse, adaptive receive/read-ahead and higher
concurrency within a chosen memory envelope. Conservative option: lower retention,
concurrency and tighter progress limits. Neither has numeric defaults approved.
Tune metadata latency, large IO throughput, retained-view cost and overload fairness;
neither maximum memory nor minimum copy count alone defines best performance.

Experiment proposal only: sweep receive quanta 4/16/64/256 KiB, payloads 4 KiB
through 8 MiB, concurrency 1/8/32/128 and no-pool/warm-pool variants. These are sample
points, not supported-size ceilings or gates. Extend the sweep where trends show
unrealized gains. Wire tests use arbitrary tiny splits. User must approve restrictive
defaults/timeouts; the implementation gate stays closed until policy is agreed.

## Collections, Protocol and Error Semantics

Paged List<T> with iteration/len/last/checked push replaces `.0` Vec access. Page
size is a tuning variable, not a 64 KiB restriction. Check wire feasibility and
host expansion before each growth; include nested owned fields, not just size_of.
Node/depth budgets cover zero-wire-size generics; linked lists decode iteratively.
Count old+new descriptor capacity at growth. No reserve from wire count alone.

Directory replies: count/maxcount and dircount bound wire output; host capacity and
operation entitlement bound accumulated entries. Inspect one entry at a time;
stop before either bound, reserve terminator/status overhead, eof=false and last
included cookie. If no entry fits return TOOSMALL. Memory exhaustion is not eof.
Client owns paged output independently. FSINFO/PATHCONF reflect approved policy,
not unrelated historical constants. Backend entries themselves remain external
until adopted and charged; reject excessive capacity without a giant hidden copy.

Validate scalar/enum/bool, auth body <=400, AUTH_SYS schema limits, NFS handle <=64,
opaque length, zero padding across spans and exact record consumption. WRITE
count equals data; READ response count equals length and is <=requested count.
RFC 1813 filename/path strings are wire-unbounded; local maxima are policy, not
invented RFC limits. Checked size accounting covers all derived fields and lists.

| Failure | Disposition |
| --- | --- |
| Incomplete/framing/overflow/policy rejection before full record | Close with bounded diagnostic; no unbounded drain. |
| Bounded malformed args/XDR/padding | GARBAGE_ARGS when valid RPC context permits; release record then continue framing. |
| Malformed credential | Appropriate RPC auth error, e.g. AUTH_BADCRED; distinguish bad verifier. |
| WRITE count mismatch / overlong supported name | NFS3ERR_INVAL / NFS3ERR_NAMETOOLONG. |
| Unsupported procedure/version | Existing RPC/NFS status semantics. |
| Complete RPC/NFS error reply | Reusable client after full consumption and valid XID. |
| Unexpected XID/CALL, malformed client reply, partial IO/cancellation | Poison/close; no guessing next reply. |

Errors retain offset/length and configurable bounded prefix, not record backing.
Primary references: RFC 5531, RFC 4506 and RFC 1813. Wire fixtures/independent
parsing required beyond symmetric round trips.

## Transaction and Retry Lifecycle

Proposed fix for R11: admitted -> executing -> reply queued -> bytes written.
Writing all bytes is not proof of client delivery. Token follows work/reply; Drop
does not imply success. Before execution, cancellation can remove entry. Once VFS
is polled, cancellation requests must keep an active entry/entitlement until that
future has actually terminated/dropped; non-cooperative work remains active.
Dropping a wrapper future is not enough if detached tasks or blocking work can
still mutate state. Such backends must transfer the token to the actual work and
signal its termination; until then suppression and associated charges remain.

Key initially preserves IP:port/XID scope (current peer_addr().to_string()), with procedure/version and a
request identity checked against the active record to reject XID reuse with different
args. Active comparison may retain input bytes and must be charged. No stronger
cross-reconnect exactly-once guarantee is claimed. Conservative replay-safe class
contains only read-only operations; mutation replay safety requires an explicit
protocol/backend proof, not an assumption that all WRITEs are harmless to repeat.
Outcome entries also retain request identity, not merely XID. Propose a collision-
resistant canonical request digest including RPC program/version/procedure and
arguments, plus length; algorithm/dependency and threat-model suitability require
review. Active comparison can check the original bytes. A digest collision is a
residual risk, not an exact-byte equality guarantee; do not claim otherwise.

If mutation may have occurred but response is unsent/partially sent or delivery is
unknown, transition to OutcomeUnknown/BytesWritten metadata rather than deleting
suppression and re-running mutation. Proposed duplicate disposition: replay cached
reply if one exists, otherwise explicitly return RPC SYSTEM_ERR for uncertain
mutations; read-only operations may execute again. No silent discard. Reserve
small outcome-entry capacity before admitting work. The wire SYSTEM_ERR is generic
and does not tell clients that mutation outcome is uncertain; document this
limitation and emit bounded server-side diagnostics. Outcome retention/eviction
and optional bounded reply cache are user policy decisions still requiring approval;
eviction permits later re-execution and must be documented. If no outcome slot can
be reserved, reject before VFS instead of losing tracking after a side effect.
Preserve stable/COMMIT/exclusive CREATE semantics. This is not transaction rollback
or a crash-persistent exactly-once protocol. Test every transition with a backend
whose side effect and future termination can be controlled independently.
B4 correction: admitted mutations keep a server-owned completion task, owned
Payload and transaction token even after socket closure; do not abort that task
merely because its response destination vanished. Detached backend work must
participate in actual completion signalling before token release. IP:port scope
does not protect a retry using a new source port; IP-only scope risks NAT/XID
collisions, so no reconnect deduplication guarantee is added here. Proposed
dispositions pending policy review: conflicting-argument XID reuse -> RPC SYSTEM_ERR
without another VFS call; exhausted outcome slots -> RPC SYSTEM_ERR before mutation;
duplicate already-written mutation without cached reply -> RPC SYSTEM_ERR, not
silent drop or blind re-execution. Identity digest/cache/retention choices remain
open; compare full hashing cost without silently substituting a weaker checksum.
Baseline observes existing tracker behavior and must not implement these changes.

## Measurement and Proof Plan

No gates set. Gather comparable evidence, identify best candidates and propose a
user-agreed contract; D2 thresholds do not apply. Capture baseline before behavior
changes. Separately authorize instrumentation-only hooks where missing; candidate
prototypes are not baseline-only and need distinct authorization after capture.

Reuse existing perf/tests with feature-gated origin/copy/allocation hooks. Use
rallocator 0.4.0 in a measurement executable/bench (workspace placement now possible),
not automatically as product allocator. Baseline/candidate use identical compiler,
allocator/features/harness per lane. Also measure normal allocator. Linux 4 KiB
pages, Windows MSVC, macOS >=14.4 and architecture requirements must be verified;
unsupported combinations are gaps. Events give requested bytes, not retained
origins; lost events invalidate exact counts. Pair origin hooks with realloc old+
new peak accounting, unique capacity and span/control cost. No thread-local task
identity across awaits or recursively allocating collector. Record collector cost.

Report per endpoint/op and whole workload: payload copies by site (including tail
batch/backend/test transport), allocations/reallocations/largest allocation,
initialized bytes, live/retained/free-pool/high-water capacity, queue depth, syscall
counts, throughput and p50/p95/p99. RSS is supplementary. Credit reservation differs
from physical memory. After drop, live origins return to free/released exactly
once; permitted pool retention is not a leak.

Workloads: metadata RPCs; READ/WRITE sizes; directories/retained-handle traversal;
independent RPC fragments and TCP segmentation; header-only/slow peers plus healthy
traffic; concurrency beyond proposed limits; tiny views at block edges; cold/warm
pools/trim; all partial marker/scalar/padding/payload states; cancellation/transaction
retry before/after side effects; real TCP Nagle on/off; released-client mixed version.

Compare bytesbuf/custom pool, GlobalPool diagnostic, std Arc and bytes where feasible
with identical inputs. Initialized/uninitialized paths only after safety checks.
Serial paired release runs, alternate order, publish raw samples/uncertainty and
instrumented/uninstrumented overhead. Exact run counts/tolerances are recommendations
for Verify, not approved restrictions. Noisy results are inconclusive; throughput
does not establish memory/copy goals. Incompatible baseline inputs get correctness
results, not fabricated speedups. Survey remaining opportunities before proposing
gates; do not stop automatically at the first successful alternative.

Tracker validation precedes performance interpretation: deterministic allocate,
clone/slice/realloc/drop/trim sequences with an independent expected ledger; origin
generation IDs distinct from addresses; exact final-drop count; loss counters at
start/end of every rallocator capture and known initial live set. Exact allocator
reconstruction also requires validated cross-thread ordering and address-generation
reuse; loss-free events alone are insufficient. Realloc reports
old+new overlap conservatively from site hooks even if allocator event order hides
it. Copy hooks name transport forwarding, growth, codec, materialization, padding
batch, backend and test transport; zero-fill is a separate counter. Warm/cold pool
captures record starting/ending free capacity. Run instrumented and uninstrumented
paired lanes and report overhead without mathematical subtraction. If hooks lose
events or cannot attribute origins, mark affected exact metrics BLOCKED. Compile-only
proof fixtures for generic derives, VFS borrows, Send and both runtime adapters
belong to separately authorized prototype work, not an assertion of feasibility.
Focused adversarial tests explicitly include huge advertised lengths delivered
incrementally without huge allocation, tiny/empty fragment storms, all length/
count/offset overflow boundaries and concurrent cancellation amid slow readers
and writers. Use bounded inputs and accounting hooks, never deliberate OOM.

## Migration and Required Gates

Stage B0 baseline-only is authorized as detailed in baseline.md. All later stages
await separate explicit authorization of accepted design/contracts/scope.

0. Baseline-only hooks/harness and allocator/toolchain lane validation, independent
   capture. Then separately authorized prototype comparison for provider/IO/layout,
   proof/bench evidence before unsafe initialization/default/threshold selection.
1. Approved bytesbuf dependency/MSRV change, accounted provider and payload APIs,
   proof/capacity tests; no production transport switch initially.
2. Cursor/paged lists/checked encode/owned metadata/generic payload types, macros
   all branches and every direct codec caller/fixture.
3. VFS bulk API/backends; server direct receive/admission/task and queue lifetimes,
   sender/transaction lifecycle; FSINFO/PATHCONF/directories under approved policy.
4. Client fragmented receive/send plans/poisoning, Tokio/smol, retained outputs,
   nfs/mount/portmap/error/reexport/example migrations.
5. Remove obsolete compatibility paths; server examples, cargo_nfs3_server mirror/
   memfs, client download_folder, tests/perf helpers, public README/docs/changelogs.
   Independent comparison, wire interop, full validation and review.

Verify returns Git status/diff/untracked source; Review inspects actual changed
files. Architecture changes return to review; no progression on stale approval.

Future MCP gates: check all targets/features before edits; clippy workspace/all
targets/all features/warnings-as-errors; nightly fmt (Verify check-only); tests all
features; build all targets/features; machete; deny. Locked debug/release CI for
Linux x86_64/aarch64, Windows x86_64, macOS aarch64 and new MSRV; Tokio/smol/examples.
Build test_mirror_fs through MCP before CI-equivalent run (CI uses debug even in
release job). WASM unestablished. Required tool execution must be demonstrated,
not inferred from advertising. No Rust gates executed in design-only phase.

Wire proof: RFC golden bytes, independent packet builder/parser, captured output
and independent NFS peer where available. Peer/target unavailability is a gap.
Add supported fallible-site failures, unsafe-adapter tests, retained-origin checks
and overload cleanup. Missing measurements are never a passing result.

## Review Map and User Gate

Proposed resolutions: R1 actual origin charges; R2 phased/fair admission/read-ahead;
R3 segmented VFS; R4 ordinary single-fragment streamed send; R5 paged dual-budget
lists; R6/R7 batching/Nagle/syscalls; R8 errors; R9 std comparison; R10 generic bulk
types/owned metadata derives; R11 token/retry lifecycle. Not yet reviewer acceptance.

User decisions: adopt accounted bytesbuf pool/segmented VFS direction? Resource
envelope and deployment priorities for numeric defaults? Later, measured acceptance
criteria. No restrictive values silently activated. D3 cannot authorize behavior
implementation while policy, provider proof, baseline feasibility or review blockers
remain unresolved.