# NFS3 Buffering Design

Revision D3.3, condensed without new architecture approval. Only B0 baseline
instrumentation and the documented B1-B4 correction directions are authorized;
prototypes and buffering behavior changes are not. See [baseline](baseline.md),
[open review findings](review.md), and [current status](progress.md).

## Constraints

- Reduce payload copies and peak/live/retained memory; never allocate a whole
  advertised record before receiving it. Preserve RPC/XDR/NFS3 interoperability,
  not public API compatibility.
- User recommends `bytesbuf`; no dependency winner is approved. MSRV increases,
  pools, and segmented VFS APIs are allowed. Unsafe initialization requires a
  soundness argument and measured benefit over a safe alternative.
- Favor performance and low friction, with conservative configuration available.
  Numeric limits, deadlines, pool policy and performance gates remain unset.
  Ask before imposing restrictions; withdrawn limits are not requirements.

## Current Costs

Source observations, not measurements:

| Path | Cost or risk |
| --- | --- |
| Server TCP | 128 KiB scratch -> 256 KiB duplex -> record Vec adds forwarding copies. |
| Record assembly | `IncompleteRpcMessage::recv` resizes/zero-fills from fragment lengths; growth may move previous data. |
| Decode | `Unpack(Read)` copies owned opaque data; collection growth trusts counts; padding values are ignored. |
| Server dispatch | Spawned task owns record and separately decoded payload; reply queue is unbounded. |
| Server READ/client WRITE | Payload copied into whole encoded message Vec. |
| Client receive | Rejects fragmented replies; eager record allocation, opaque copy, whole-record error retention. |
| IO/macros | Zero-progress loops and unchecked generated size/count arithmetic. |
| Retry | Transaction drop marks completion; completed XIDs suppress retries without replay. |

Implementation anchors: server `tcp`, `rpcwire/messages`, handlers and transaction
tracker; client `rpc`/`io`; types `xdr_codec`; macros `lib.rs`. Existing VFS 1 MiB
preferences are observations, not new policy.

For the [B0.3 RPC lane](baseline.md#b03-rpc-copy-and-growth-ledger), the local trace is:
client packs marker/RPC/args into one Vec; server forwards 128 KiB socket scratch
through its 256 KiB duplex, then resizes a record Vec per RPC fragment. Dispatch
moves that record into a spawned handler; opaque WRITE decoding separately allocates
payload retained across the borrowed VFS write. READ adopts the backend Vec into
Opaque, packs it into a reply Vec, queues and sends it; client zero-fills a record
Vec then decodes a separate owned output. Test ReadFs explicitly copies with to_vec;
WriteFs consumes a borrow without storage copy. Record Vec capacity changes bound
possible logical-payload relocation but do not reveal actual allocator copying.
Existing generic handlers do not expose exact final backing-allocation release.

## Storage Choice

[Pinned research](research.md) supports these proposals, not measured NFS results.

| Candidate | Decision |
| --- | --- |
| bytesbuf 0.14.0 | Prototype direct segmented storage with a small accounted provider; avoid another segmented engine. Rust 1.95, MIT. |
| GlobalPool | Compare reuse; no public trim/retained-capacity control or exact origin accounting. Insufficient alone for configurable server policy. |
| bytesbuf_io | Owned runtime-neutral IO; no ready Tokio/smol adapters found. Convenience reads reserve requested length; cancellation is not guaranteed. Compare with local poll-based framing. |
| bytes / BytesMut | Contiguous comparator; growth/coalescing and hidden capacity can cost. Outer Arc adds control overhead. |
| Arc<Chunk { Vec, ticket }> | Simple comparator with explicit ownership/capacity; Arc slice conversion does not adopt Vec storage. |
| Contiguous Vec | Baseline comparator, not dismissed without measurements. |
| http_headers | Adapt owned/borrowed contracts, selective small copies and owner/allocation tests. Unpublished; reject HTTP grammar assumptions. |
| tokio-util / Serde | Adapt framing tests and lifetime contracts; neither is a drop-in RPC/XDR solution. |

Dependencies are MIT or MIT OR Apache-2.0; retain notices and check cargo-deny.
No source copying planned. Dependency/MSRV/lockfile/CI/docs must move together.

## Ownership and Accounting

`Payload` owns a bytesbuf `BytesView`; `PayloadRef<'a>` borrows slices/segments.
Expose chunks, logical length, optional contiguous access and explicit materialization,
not implicit contiguous dereference. Metadata (names, paths, auth, handles) remains
contiguous and independently owned after decode. Tiny-view copy/retain policy is
measured, not fixed; empty views retain no backing.

Each physical allocation has a monotonic origin ID, actual capacity and domain.
Views retain intersected origins; clones charge additional control/span metadata,
not shared bytes twice. Live = checked-out/shared origins; retained views are a
subset of live. Free-pool capacity is disjoint. Account growth overlap and control
allocations separately. Credits, Rust backing capacity and allocator/RSS are distinct.

Provider transitions `free -> checked_out -> shared -> free/released` are linearizable.
Re-rent does not allocate; trim only detaches free blocks. Never invoke final-drop
callbacks under a lock they reacquire: detach under lock, release outside. Origins
keep the domain alive after shutdown; closed pools reject rents and release returns.
Cross-domain references retain their original physical charge.

Public Block/BlockRef/BlockMeta and `BytesBuf::from_blocks` permit custom storage.
Keep original backing for recycling; ordinary Vec adoption has no reclaim API.
Use standard thread-safe ownership, not a new refcount. Unsafe adapters must prove
stable pointer/capacity, exclusive writes, initialized-prefix commits, immutable
sharing, metadata lifetime, clone/drop balance and cross-thread reclamation.
Require Miri/interleaving/drop tests and benefit measurements before acceptance.

Charge a known requested Layout BEFORE allocation; post-allocation Vec capacity
checks do not prove a hard bound. External Vec adoption charges known capacity
before transfer, otherwise rejects without publishing uncharged storage. Prevent
ambient GlobalPool growth. Infallible library metadata/runtime allocations require
an audited per-target control envelope before claiming total protocol bounds.
Unsupported exact bounds remain BLOCKED; actual OOM may abort. Pool classes,
retention/trim/sharding and initialization strategy remain experiment variables.

## APIs and Async Lifetimes

- Generic bulk fields: `WRITE3args<Data = Payload>` and `READ3resok<Data = Payload>`;
  encoding accepts `Data: EncodePayload`, including borrowed client inputs.
  Decode targets owned payload/metadata. Derives cover generics and all struct/enum
  forms without promising arbitrary borrowed decode lifetimes.
- Cursor owns a complete record, shares payload ranges, uses stack scratch for
  cross-span scalars, and releases unselected origins before handler await.
  Spawned owners are Send + 'static; shared backing is Sync. No self-reference.
- VFS WRITE takes owned Payload and work token, movable into blocking/owned IO.
  Token, payload and accounting survive actual completion. Contiguous backends
  explicitly materialize and report copies; preserve stable-write semantics.
- VFS READ takes an owned accounted writable buffer from execution entitlement,
  returns initialized count/EOF, and exposes only initialized bytes. Clamp to an
  affordable positive count; preserve real EOF. If no byte fits, return bounded
  RPC SYSTEM_ERR before backend work, not repeated empty non-EOF success.
  External Vec adoption is a measured fallback outside pre-allocation guarantees.
- Encode checks total length and builds metadata plus borrowed/shared segments.
  Client borrows end with call future; queued server plans own all values.
- Client outputs can outlive client/domain shutdown. Constrained calls must not
  wait on credits only caller-held outputs can release: fail pre-IO when reply
  maximum is known. Unbounded replies such as READLINK need agreed policy for
  that guarantee; late exhaustion otherwise poisons/closes the connection.

## Receive, Send and Cleanup

Connection-owned parser: partial four-byte marker -> fragment body -> next marker
or complete record. Track marker offset, cumulative bytes and fragment count
separately from receiver segmentation. Read directly into available provider
quanta, not advertised-length allocations. Charged read-ahead may contain several
records; merge adjacent ranges. Short reads and sender fragments do not each force
a new backing block. Tiny-fragment CPU/descriptor costs still need bounds.

Tokio may fill `ReadBuf::uninit`; commit only proven initialized prefixes across
Pending/error/short-read/cancellation. Smol initializes spans in place unless a
proven alternative exists; never cast uninitialized storage to `&mut [u8]`.

Validate checked cumulative length/count before growth. Count empty fragments;
final empty fragment is valid, empty RPC record is not. Boundary EOF is clean;
partial marker/body EOF is truncation. Zero exact-read/write progress becomes
UnexpectedEof/WriteZero; retry Interrupted. Framing failure closes without draining.

Recommend one outgoing fragment within the protocol's 31-bit length, streaming
segments inside it to support released clients. Larger records need fragmentation
and peer compatibility handling. Batch marker/metadata; use writev where supported,
tracking offsets through short writes. Compare bounded tail/padding copies against
sequential fallback; test Nagle on/off. Never interleave records.

Reader, bounded handlers and single writer progress independently; admission never
holds writer lock. Partial sends close/poison. Cancelled client calls poison unless
connection-owned state resumes the same operation. Disconnect drops unsent replies
and cancels unadmitted/read-only yielding work; admitted mutations finish in
server-owned tasks. Blocking/detached work must signal actual completion before
resources are released. No rollback or non-cooperative cancellation guarantee.

## Admission and Limits

Expose configurable allocation/record/fragment/count/decode/read-ahead/connection/
execution/queue/domain/pool limits and progress deadlines; no numeric defaults
are approved. Backend/kernel/runtime memory is separately reported. Shared listener
domains can enforce shared bounds; independent clients have no implicit process cap.

Proposed physical budget G contains disjoint receive R, execution E, drain/control D
and free-cache F shares, including metadata and growth overlap. Release free cache
before transferring credits. R holds partial/completed records and read-ahead.
Validated operations atomically acquire full additional host/output entitlement
and a slot from E before VFS; no nested entitlement acquisition. Replies retain E
until sent/dropped. D is reserved for writers and bounded failure replies, never
lent to body receive. Reject impossible configurations/operations before backend.

B1 correction: before EVERY R grant, including read-ahead, preserve a completion
order. Each record declares maximum attainable charge M_i including future fragments,
origin rounding and descriptors, with H_i already held. Candidate safe-state check:
start with free R, find M_i-H_i that fits, simulate completion/release by adding H_i,
repeat for all records. Grant only if such an ordering exists; serialize grants.
Credits are not eager allocations. Keep completed/shared backing charged until
actual release; E/D progress and actual backend completion are prerequisites.
Test N records larger than R/N, unknown fragmented totals and a progressing peer.
This algorithm is a proposal requiring proof/cost comparison, not tuned policy.

Fair rotating admission, progress expiry and slow-writer backpressure must be tested.
Disabled quotas/deadlines cannot promise finite reclamation/fairness. Non-cooperative
work retains its entitlement. Numeric shares and overload behavior need user agreement.

## Decode, Collections and Errors

Check lengths/padding/remaining bytes before access; validate zero padding across
spans, scalar/enum/bool, auth <=400, AUTH_SYS fields and NFS handle <=64. WRITE
count equals payload; READ count equals payload and does not exceed request.
Filename/path strings have no RFC maximum: local bounds are policy. Consume records
exactly; generated sizes use checked arithmetic.

Paged lists replace Vec field access with iteration/len/last/checked push. Bound
wire feasibility, nested host expansion, nodes/depth and old+new growth before
allocation; no wire-count reservation. Decode linked lists iteratively. Directory
responses stop before wire or host limits, reserving terminal overhead; return last
included cookie/eof=false, or TOOSMALL if no entry fits. Exhaustion is not EOF.
FSINFO/PATHCONF must reflect agreed policy.

Framing/truncation/overflow closes. Complete malformed args/padding map to
GARBAGE_ARGS when RPC context permits; malformed credentials/verifiers get auth
errors; WRITE count mismatch -> INVAL, overlong supported name -> NAMETOOLONG.
Valid complete RPC/NFS errors permit reuse; unexpected XID/CALL, malformed client
reply or partial IO poison. Diagnostics retain bounded prefixes/offsets, not records.

## Transactions

Track admitted -> executing -> reply queued -> bytes written; bytes written does
not prove delivery, and Drop is not success. Before execution cancellation removes
entry; after mutation starts keep payload/token/credits until actual work ends,
including after disconnect. Baseline does not change existing tracker behavior.

Identity uses current IP:port/XID scope plus program/version/procedure/arguments.
No cross-reconnect exactly-once claim. Charge active request comparison; outcome
identity digest algorithm/collision risk and full-payload hashing cost need review.
Read-only operations may replay; mutations need explicit replay-safety proof.

Reserve outcome capacity before VFS. Proposed uncertain-mutation handling: replay
cached reply, otherwise SYSTEM_ERR rather than re-execution/silent discard.
Conflicting-argument XID reuse and exhausted slots also return SYSTEM_ERR before
mutation. It does not communicate outcome uncertainty to clients; document that.
Cache/retention/eviction policy remains open; eviction permits later re-execution.
Preserve stable/COMMIT/exclusive CREATE semantics; no crash-persistent guarantee.

## Stages and Gates

1. Authorized B0: validate hooks and capture independent unchanged-behavior baseline;
   next bounded lane is [scoped opaque retention](baseline.md#b02-scoped-opaque-retention).
2. Separately authorize provider/IO/type prototypes; prove unsafe contracts and
   compare alternatives before choosing policy and measured acceptance thresholds.
3. Approved provider/payload APIs, then cursor/lists/checked codecs and all macro
   branches/callers; initially no production transport switch.
4. VFS/backends, server receive/admission/send/transaction lifecycle, directory limits.
5. Client fragmentation/send/poisoning/retention and Tokio/smol migration.
6. Examples, cargo server mirror/memfs, tests/perf/docs; remove temporary compatibility
   paths; independent wire/performance verification and review.

[Validation plan](validation-plan.md) owns measurement and test requirements.
Each authorized stage needs independent Verify and Review on exact changed inputs;
architecture changes return to review. No stage is complete with required gates
unresolved. No baseline, implementation pass or performance improvement is claimed.
