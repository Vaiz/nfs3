# Public API migration inventory

Comparison: original main/PR merge base
`00ea3aa2f2d4027bb9ead8da2ee71d647e88f40e` to production HEAD
`b96be44a1a3a65a127b4716249a3f412c48d0b93` plus the authorized uncommitted
C1/C2 changes in this worktree. This describes the resulting complete surface,
not only C2b. Source inspection and focused checks are evidence, not independent
API acceptance or a machine-proven compatibility claim. No new dependency or
feature/default is introduced by consolidation.

## 1. Canonical codecs and derives

`nfs3_types::xdr_codec` exports exactly one checked pair:

```rust
trait Encode {
    fn encode(&self, encoder: &mut Encoder) -> EncodeResult<()>;
}
trait Decode: Sized {
    fn decode(decoder: &mut Decoder) -> DecodeResult<Self>;
}
```

The streaming `Pack`, `Unpack`, unchecked `packed_size`, streaming `Error`/`Result`
and their helpers/implementations are removed, not aliases or private bridges.
Encode using `Encoder::new(limits)`, `encode(&value)`, consuming `finish()` to
obtain a `SendPlan`. Decode a complete `Payload` with `Decoder::new(record,
limits)`, `decode::<T>()`, then consuming `finish()` for exact consumption.
Custom implementations must nest through the state methods; independent
allocations/direct trait calls are outside aggregate guarantees.

Both states preserve the first value-only error, including swallowed nested
errors. Partial aggregate progress/output stays private; failure releases input/
output on finish/drop. Neither errors nor summaries retain bulk owners.

`nfs3_macros` and its `xdr_codec` reexports:

- `XdrCodec` now generates checked Encode **and** Decode with independent
  directional bounds. `XdrEncode`/`XdrDecode` generate only one direction.
  One schema/parser/traversal is shared by all entry points.
- Named, tuple and unit structs; unit-only enums with u32 literal/implicit
  successive discriminants; tagged unions with one `#[xdr(u32_literal)]` per
  variant (unit or one unnamed field) are supported.
- Missing/duplicate/malformed/overflow tags, negative/expression discriminants,
  unsupported arm shapes and Rust unions produce compile errors. No `xdr`
  attributes on containers/fields/unit-only variants.
- Lifetimes, const parameters and where clauses remain. Type parameters receive
  only the direction's bound, not both; field-type bounds are avoided for
  recursive lists. Borrowed `&Payload` encodes without Decode, and decode-only
  types can instantiate combined-derived generic types.
- Generated paths use `nfs3_types::xdr_codec` directly. Manual implementations
  handle protocol field bounds/default union arms. Unknown tags are sticky
  `DecodeError::InvalidTag`.

Implementations cover u32, u64, strict bool, padded fixed `[u8; N]`, Opaque,
Payload, Void, counted `Vec<T>`, iterative linked `List<T>` and `Encode for &T`
with only the corresponding directional constraint. Collections grow
incrementally, never reserve from an advertised count, and charge full geometric
host-allocation targets. Metadata Decode always owns bytes for every lifetime.

### Policies, errors and extension substrate

All policy fields below are public, required, `usize` unless noted. Policy and
limit-kind enums derive Debug/Copy/Clone/PartialEq/Eq; no numeric defaults.

- `DecodeLimits { max_metadata_bytes, max_collection_elements, max_depth,
  max_reservation_bytes, payload: PayloadLimits }`.
- `DecodeLimit::{MetadataBytes, CollectionElements, Depth, ReservationBytes,
  FieldElements}`.
- `DecodeError::{Wire(PayloadError), Limit { kind: DecodeLimit, limit, actual },
  InvalidTag(u32), Overflow, Allocation}`; `DecodeResult<T>`.
- `EncodeLimits { max_wire_bytes, max_metadata_bytes, max_chunks,
  max_retained_capacity, max_depth, max_reservation_bytes }`.
- `EncodeLimit::{WireBytes, MetadataBytes, Chunks, RetainedCapacity, Depth,
  ReservationBytes, FieldBytes, FieldElements}`.
- `EncodeError::{UnrepresentableLength(usize), UnrepresentableCount(usize),
  InvalidRepresentation, Limit { kind: EncodeLimit, limit, actual },
  Overflow, Allocation}`; `EncodeResult<T>`.

Errors implement Display/Error and payload conversion; Decode forwards its wire
cause. Encode maps non-allocation/non-overflow payload failures to
InvalidRepresentation. Reservation work is monotonic requested layouts, not live
memory or allocator usable size.

Public Decoder methods besides construction/decode/finish:
`fail<T>(DecodeError)`, `position()/remaining() -> DecodeResult<usize>`,
`opaque(max) -> DecodeResult<Opaque<'static>>`,
`collection<T: Decode>(max) -> DecodeResult<Vec<T>>`.
Public Encoder methods:
`metadata_reservation_bound(bytes) -> Option<usize>`,
`fail<T>(EncodeError)`, `metadata(&[u8])`, `opaque(&[u8], max)`,
`collection<T: Encode>(&[T], max)`, `payload(&Payload)`.
Encoding methods return `EncodeResult<()>`. All arithmetic is checked and lengths/
counts must fit u32. Metadata includes scalars/tags/padding as well as opaque data.

`SendPlan` has summary Debug, `len`, `is_empty`, and borrowed nonallocating
`chunks() -> impl ExactSizeIterator<Item=&[u8]> + FusedIterator + Clone`.
It owns copied coalesced metadata and shared bulk and is Send/Sync/'static.
No sizing pass/whole-record assembly is required. Bulk retention is summed per
descriptor/use, even for repeated origins. Record markers are external.
Cursor, builder, implementation modules and concrete iterator names are private;
cross-crate framing/macro extension substrate stays public.

## 2. Storage and accounting

New `xdr_codec` exports:

- `PayloadLimits { max_logical_bytes, max_backing_capacity, max_segments }`;
  `PayloadLimit::{LogicalBytes, BackingCapacity, Segments}`.
- `PayloadError::{LimitExceeded { kind, limit, actual },
  InvalidRange { start, end, len }, Overflow, Allocation,
  Truncated { offset, needed, remaining }, InvalidBool { offset, value: u32 },
  NonzeroPadding { offset, value: u8 }, BufferTooSmall { needed, available },
  Trailing { offset, remaining }}` (other numeric fields usize).
  Debug/Copy/Clone/PartialEq/Eq/Display/Error.
- `BackingBudget`: Clone shared domain, Debug, `new(limit)`, `used()`,
  `try_reserve(bytes) -> Result<BackingReservation, PayloadError>`.
  Reservation: Debug, `remaining()`, no public clone/split.
- `Payload`: Debug, deliberately no Clone/Deref/AsRef/implicit materialization.
  `empty`, `descriptor_reservation_cost(segments) -> Option<usize>`,
  `try_from_vec(Vec<u8>, limits)`, `try_from_chunks(Vec<Vec<u8>>, limits)`,
  `len`, `is_empty`, `segment_count`, `retained_capacity`,
  `chunks() -> impl ExactSizeIterator<Item=&[u8]> + DoubleEndedIterator`,
  `as_contiguous() -> Option<&[u8]>`, `try_clone`, `try_slice(Range<usize>)`,
  `try_to_vec`. Fallible operations return PayloadError.
- `ReceiveBuffer`: Debug, `try_new(len, max_capacity)`,
  `try_new_charged(len, &mut BackingReservation)`, `as_mut_slice`, `capacity`,
  consuming `freeze(prefix, PayloadLimits) -> Result<Payload, PayloadError>`.

Adoption does not shrink/copy Vec backing. Empty views own no backing; shared
views keep only intersected origins/charges, but a tiny view can retain a large
origin. Capacity sums per descriptor, not unique bytes. Explicit `try_to_vec`
copies. Receive storage is initialized before IO; only checked prefixes publish.
Charged backing releases on final owner drop, not request-future cancellation.
Unused reservation releases independently; outputs can outlive clients/listeners.
Actual accepted backing and descriptor capacities are checked. Allocator
transients/usable size, Arc control allocations, runtime/task allocations and
independent application/backend memory are not a total-process bound.

The former additive `BoundedList` is not public; directory page sizing/storage
is private, allocation-free checked arithmetic followed by incremental bounded
growth. `List<T>` remains the wire representation.

## 3. Protocol types

All NFS/MOUNT/portmap/RPC wire types and manual result/unions now use only the
checked pair. `rpc::fragment_header` retains raw marker construction/access/
byte conversion, but no codec impl (framing is not a nested XDR value).
Procedure `TryFrom<u32>` errors use `DecodeError::InvalidTag`, not streaming Error.

Canonical READ/WRITE declarations replace the original lifetime-specific structs:

```rust
struct READ3resok<D> {
    file_attributes: post_op_attr, count: count3, eof: bool, data: D,
}
type READ3res<D> = Nfs3Result<READ3resok<D>, READ3resfail>;
struct WRITE3args<D> {
    file: nfs_fh3, offset: offset3, count: count3, stable: stable_how, data: D,
}
```

Fields remain public, no declaration bounds/default data type. Use Payload for
shared bulk, `&Payload` to encode borrowed bulk, Opaque for copied metadata.
No redundant leaf aliases or alternate nominal READ/WRITE families remain.
`Nfs3Option<T>` loses declaration-level streaming bounds; methods need no codec
bound. `Nfs3Result<T,E>` uses independent directional constraints. All other
procedure args/success/failure aliases retain their layouts and lifetime
parameters; supporting NFS attributes/handles/names/options/lists, MOUNT handles/
paths/names/results/exports, portmap mappings/lists/call fields, and RPC auth/
headers/replies/unions all acquire the same checked contract.

Strict validity: zero padding, boolean/enum tags, auth body <=400, AUTH_SYS
machine <=255 and groups <=16, NFS handle <=64 and MOUNT constants.
Wire-unbounded names/paths use caller policy, not invented protocol maxima.
`mknoddata3` accepts valid non-device ftype tags into `default`, rejects unknown
ftypes; encoding `default` rejects ambiguous representation. Encoding
`Nfs3Result::Err((NFS3_OK,_))`/`mountres3::Err(MNT3_OK)` also rejects.
The existing closed auth flavor enum is not broadened to all extensible RPC
flavors. READ/WRITE count consistency remains operation policy, not normalization.
`opaque_auth::auth_unix(&auth, EncodeLimits) -> EncodeResult<Self>` is fallible:
bounded checked encoding plus an explicit <=400-byte owned metadata copy.

## 4. Runtime-neutral IO and canonical clients

Traits move from client IO to `nfs3_types::async_io`, reexported at
`nfs3_client::io`. Identity changes crates; both remain Send with required
`async_read`/`async_write` returning Send futures of `io::Result<usize>`.
Exact/all defaults retry Interrupted, reject overreported progress and return
UnexpectedEof/WriteZero on zero progress instead of looping. Added default
`async_write_vectored(&[IoSlice])` uses the first nonempty slice.
TokioIo/SmolIo override it with real runtime vectored IO; connector/wrapper
feature names remain `tokio`/`smol`, with no core runtime dependency.

Public framing:
`RecordLimits { block_bytes, max_record_bytes, max_fragments }` (Debug/Copy/Clone),
`validate() -> bool`, `builder_reservation_bound() -> Option<usize>`;
`record_reservation_bound(blocks) -> Option<usize>`;
`FramingError::{Io(io::Error), Limit, Storage(PayloadError)}` with Display/causes;
async `first_marker(&mut IO) -> io::Result<Option<u32>>`,
`receive(&mut IO, RecordLimits, &mut BackingReservation)`,
`receive_with_marker(&mut IO, marker, RecordLimits, &mut BackingReservation)`
returning Result<Payload, FramingError>; `send(&mut IO, &SendPlan) -> io::Result<()>`.
Incremental blocks are independent of fragments; no read-ahead or global credit
acquisition. Boundary EOF is distinguished; partial EOF fails. Send copies at
most 1020 leading metadata bytes to a marker stack buffer, never bulk. Deadlines/
cancellation/close policy belong to callers.

### RPC

`nfs3_client::rpc` is the canonical namespace. No bounded implementation family,
shadow streaming traits or second RPC error hierarchy remains.

- `RpcLimits { record: RecordLimits, backing_bytes: usize,
  decode: DecodeLimits, encode: EncodeLimits }` (Debug/Copy/Clone);
  `validate(self) -> Result<Self, RpcErrorKind>`.
- `RpcErrorKind::{Configuration, Unusable, Argument, Admission, Framing,
  UnexpectedReply, Semantic, Io(io::Error), Storage(PayloadError),
  Encode(EncodeError), Decode(DecodeError), Rejected(rejected_reply),
  NotAccepted(accept_stat_data)}`.
- `RpcError { kind: RpcErrorKind, io_started: bool, connection_usable: bool }`;
  `is_connection_reusable() -> bool` returns the state flag.
  Both errors have Debug/Display/Error and source chaining; root reexports remain.
- `RpcClient<IO>`: summary Debug, explicit
  `new(io, RpcLimits, credential, verifier) -> Result<Self, RpcErrorKind>`,
  `is_usable()`, `backing_used()`.
- With IO: AsyncRead+AsyncWrite,
  `async call<C: Encode + Sync, R: Decode>(program, version, procedure, &C)
  -> Result<R, RpcError>`.

Constructors perform no IO; no defaults/new_with_auth alternate constructor.
Single-flight calls reserve the maximum reply backing before IO, check encode
before poisoning, correlate XID and exactly parse success/rejections. Complete
valid RPC rejections preserve reuse. Pre-IO admission/argument/encode failure
preserves usable state; started failures/cancellation poison it. Unpolled drop
does nothing. IO custom errors may own arbitrary inner data; other diagnostics
are bounded values. No automatic retry/reconnect/deadline is added.

### NFS, mount, portmap and connection builder

Root `Nfs3Client<IO>` (Debug):
`new(RpcClient<IO>, max_bulk_bytes: u32) -> Result<Self, RpcErrorKind>`;
`rpc() -> &RpcClient<IO>`. All named methods remain: null/getattr/setattr/lookup/
access/readlink/read/write/create/mkdir/symlink/mknod/remove/rmdir/rename/link/
readdir/readdirplus/fsstat/fsinfo/pathconf/commit, plus public generic structural
`call<C: Encode + Sync, R: Decode>(NFS_PROGRAM, &C) -> Result<R,RpcError>`.
Except these changes, named procedure args/result types retain their meanings:

- `read(&READ3args) -> Result<READ3res<Payload>, RpcError>`.
- `write(nfs_fh3, offset: u64, stable_how, &Payload) -> Result<WRITE3res,RpcError>`.

Construction checks maximum READ/WRITE reply feasibility, nesting/auth metadata,
descriptor work and unaligned bulk retention. READ rejects excess requests before
IO, tightens decode to requested count, validates count/data equality; WRITE
derives transmitted count, borrows input and validates response count <= submitted.
Semantic failure poisons; a started unusable WRITE is **uncertain**, never an
implicit retry recommendation. Send plans own references to input backing.

`MountClient::new(io, RpcLimits)` and
`new_with_auth(io, RpcLimits, credential, verifier)` now return
Result<Self,RpcErrorKind>; null/mnt/dump/umnt/umntall/export use checked core.
`PortmapperClient::new(io,RpcLimits)` likewise; null/getport/dump remain.
MountError/PortmapError forward canonical RpcError and its reuse flag.

`Nfs3ConnectionBuilder::new(connector, host, mount_path, RpcLimits,
max_bulk_bytes) -> Result<Self,RpcErrorKind>` validates before IO.
Host/path, port overrides, auth setters and privileged-source-port option remain;
default privileged connection behavior is unchanged. `mount` resolves ports/
mounts/connects through the one core. `ConnectError` adds `Policy(RpcErrorKind)`,
with Display/source/explicit conversion, not success-shaped fallback.
`Nfs3Connection` retains fields, Deref/DerefMut, root handle, unmount and conversion
to Nfs3Client. Examples supply finite documented policy and downloads stream chunks.

## 5. Server and VFS

Canonical `tcp::{NFSTcpListener, ServerLimits, DecodePolicy, EncodePolicy,
DirectoryLimits}`. The bounded implementation module is private.

- ServerLimits: public `record: RecordLimits`, `decode: DecodePolicy`,
  `encode: EncodePolicy`, `max_read_bytes/max_write_bytes: u32`,
  `directory: DirectoryLimits`, `max_connections/max_operations: u32`,
  `backing_bytes: usize`, and idle/progress/receive/send `Duration` timeouts;
  `operation_envelope() -> Option<usize>`, `validate() -> io::Result<Self>`.
- DecodePolicy: metadata bytes, collection elements, depth, reservation bytes.
  EncodePolicy: wire bytes, metadata bytes, chunks, depth, reservation bytes.
  Derived bulk envelopes use admitted record/READ storage, not semantic WRITE
  size. Derived builder/control allowances are no longer caller policy.
- DirectoryLimits: `max_wire_bytes: u32`, `max_entries/max_name_bytes/
  max_metadata_bytes: usize`; `reply_metadata_bound() -> Option<usize>`.
  Policies are Debug/Copy/Clone with explicit fields and no defaults.
- `NFSTcpListener<T: NfsFileSystem>`: async
  `bind(&str, T, export: String, ServerLimits) -> io::Result<Self>`,
  `local_addr`, `backing_used`, `active_operations`,
  `accept() -> io::Result<Option<JoinHandle<()>>>`, `run() -> io::Result<()>`,
  `drain()`. `bind_ro` constructs ReadOnlyAdapter<T> for read-only backends.

The old NFSTcp trait, TCP/rpcwire engine, transaction tracker and unbounded reply
channel are deleted. Removed bind_with_generation, with_export_name,
set_mount_listener, get_listen_ip/port, handle_forever, generate_host_ip and
automatic address search; use explicit export/policy, local_addr, run/drain.
Exports normalize before length checks; accepted sockets retain TCP_NODELAY.

One operation/reply per connection; independent connections run concurrently.
Admission reserves the whole envelope before body reads. Connection exhaustion
closes; operation exhaustion reads only the marker. Deadlines cover idle/progress/
whole receive/send, not backend execution. Known args/auth decode exactly before
backend await; unknown procedures/version errors may discard uninterpreted args.
Repeated XIDs may execute again: no duplicate suppression/exactly-once guarantee.
Post-mutation encode failure closes rather than fabricating a failure response.
Nonmutating encode failure may use a reserved SYSTEM_ERR.

READ clamps to configured storage. WRITE checks ROFS before semantic count/
transfer INVAL, then handles/backend; well-formed oversized WRITE is not malformed
XDR. Directory page counts/names/host expansion/wire sizes remain independently
bounded with exact EOF/cookie behavior. FSINFO/PATHCONF advertise configured
support. MKNOD/LINK are not added as server capabilities.

Two canonical traits replace Vec/slice bulk methods, without copying adapters:

```rust
// NfsReadFileSystem: Send + Sync; existing Handle/metadata methods remain.
fn read(&self, OwnedRead<Self::Handle>)
    -> impl Future<Output=Result<OwnedReadResult,nfsstat3>> + Send;
// NfsFileSystem: NfsReadFileSystem; existing mutation/COMMIT methods remain.
fn write(&self, OwnedWrite<Self::Handle>)
    -> impl Future<Output=Result<(fattr3,stable_how),nfsstat3>> + Send;
```

Direct `vfs` reexports (implementation module private):
OwnedRead<H> has public handle/offset, mutable `buffer()` and consuming
`complete(count,eof) -> Result<OwnedReadResult,nfsstat3>`.
OwnedReadResult exposes `payload() -> &Payload`, `eof()`.
OwnedWrite<H> has public handle/offset/stable and private payload/lease,
`data() -> &Payload`. All have useful Debug, no downstream constructors.
NfsOwnedRead/NfsOwnedIo duplicate traits are removed.

Whole requests/private work leases must enter actual async/blocking jobs, through
completion and errors. Outer cancellation cannot release actual-work credits.
WRITE success requires complete bytes and requested stability; returned stability
is achieved stability. ReadOnlyAdapter forwards READ/rejects WRITE as ROFS.
MemFs fills supplied READ storage, copies WRITE chunks to persistent storage and
reports FILE_SYNC. Backend persistent allocations/caches are outside transport
budgets. Drain stops admission/readers and waits for work; an uncooperative backend
can block indefinitely. Drop signals shutdown, not cancellation of admitted jobs.

`fs_util` exports `read_owned_file<H: Send+'static>(OwnedRead<H>,PathBuf)`,
`write_owned_file<H: Send+'static>(OwnedWrite<H>,PathBuf)` and `commit_file(PathBuf)`.
Async results are OwnedReadResult, (Metadata,stable_how), and (), each with
nfsstat3 error. Actual blocking jobs retain whole requests. Short/Interrupted IO
is handled; segmented WRITE avoids coalescing; DATA_SYNC uses sync_data,
FILE_SYNC and whole-file COMMIT use sync_all. Join failures map SERVERFAULT.
Mirror CLI/example COMMIT changes from no-op success to real sync.

## 6. Executables, hidden support and validation boundaries

CLI keeps its existing functional options and requires explicit independent
record/codec/transfer/directory/admission/backing/deadline flags. Invalid policy
fails before bind/backend work. Ctrl-C stops admissions and drains real work,
retaining logging guards. The [CLI README](../crates/cargo_nfs3_server/README.md)
owns the flag list and complete invocation. No automatic numeric profile.

Mirror lookahead/resume cookies retain unsent entries and consume a single cached
iterator state, avoiding lost/duplicate names; backend caches remain separately
allocated. Server examples, client examples and performance source migrate to
canonical APIs with explicit fixture policy. Frozen ignored measurement harnesses
and prior artifacts are not rewritten or interpreted as new measurements.

Hidden `__test_reexports` exposes only TestConnection<T> with
`new(vfs,export,limits)`, `root_dir`, consuming async `run(stream)`, using the one
production loop and awaiting work on disconnect. Old RPCContext/tracker/socket
test reexports are removed. Private handlers, lease/work variants, directory
sizing and macro parser functions are not public API.

Unpublished nfs3_tests keeps performance backends and adds explicit client fixture
policy/payload construction and checked metadata fixture encoding. No runtime or
client dependency is added to the shared types/server IO layer.

Rust API compatibility is intentionally broken; valid RPC/XDR/NFS wire layouts
are not. Error enums are exhaustive. Auto traits for generic requests/clients/
listeners depend on their members; there are no new unsafe guarantees. Focused
tests preserve raw wire fixtures, splits/fragments/padding/short IO, both runtimes,
generic directional bounds, cancellation, ownership, retention/admission,
directory/stability and CLI behavior. Exact stage/integrated diffs, source
manifests, test mapping and focused results are under target/consolidation/c2b.
Independent integrated review and final repository gates remain required; this
inventory is not redesign acceptance or new performance evidence.
