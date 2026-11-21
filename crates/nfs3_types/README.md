# `nfs3_types`

`nfs3_types` provides type definitions and utilities for working with `NFSv3`. Its main purpose is to be used with the `nfs3_server` crate, but it can also be used independently.

## Features

- Type definitions for `NFSv3` protocol, including Mount and Port Mapper
- XDR encoding and decoding
- Utilities for handling `NFSv3` operations

## Runtime-neutral record IO

`async_io` provides the shared `AsyncRead`/`AsyncWrite` contracts and bounded
RPC record receive/send core, without a Tokio/smol dependency. `first_marker`
distinguishes clean boundary EOF from a partial marker; `receive_with_marker`
continues after caller admission using the same marker and reservation.
`RecordLimits` is explicit. Runtime adapters own deadlines and connection-close
policy; framing does not read ahead or acquire global credit.
The [client](../nfs3_client/README.md) keeps IO compatibility reexports and the
[server](../nfs3_server/README.md#canonical-bounded-listener) shares this core.

## Owned segmented XDR input

`xdr_codec::{Payload, PayloadLimits, ReceiveBuffer, PayloadError, PayloadLimit}`
provide owned storage. Limits are explicit caller policy, with no defaults.
The checked codec pair replaces the removed streaming `Pack`/`Unpack` API.
READ/WRITE structs are generic over their data representation. Relocated IO
defaults handle zero/Interrupted/overreported progress rather than looping forever.

`Payload::try_from_vec` and `try_from_chunks` adopt Vec backing without copying or
shrinking. `try_clone`/`try_slice` share only selected chunks; `chunks` and
`as_contiguous` borrow bytes. Only explicit `try_to_vec` materializes a copy.
`retained_capacity` sums Vec capacity per nonempty descriptor, not globally unique
bytes, allocator usable size, or descriptor/Arc overhead. Empty views own no chunk.

`ReceiveBuffer::try_new` fallibly reserves, checks actual capacity against the
caller bound, and zero-fills initialized writable storage. `freeze` consumes it,
checks a prefix and independent payload limits, and preserves nonempty backing.
Invalid freeze drops the buffer. These bounds cannot undo earlier allocations or
bound transient/allocator overhead; Arc control allocation remains infallible.
This is not a full OOM-recovery or process-memory guarantee.

`BackingBudget::new(bytes).try_reserve(max_reply_bytes)` atomically admits a whole
finite entitlement or fails without waiting. `ReceiveBuffer::try_new_charged`
spends that entitlement before allocating. Actual accepted capacity must equal
the requested initialized block length; excess capacity is rejected before
publication. The private record builder moves received prefixes, descriptors and private
charges incrementally, without rescanning old blocks or copying payload bytes.
Its derived reservation-work bound includes geometric descriptor allocations
and known Chunk layouts; `async_io::record_reservation_bound(blocks)` supplies
cross-crate sizing without exposing construction internals. Actual descriptor
capacities above requested targets are rejected. `freeze(0, ...)` and error/drop release backing.
Unused reservations plus live mutable/frozen tickets equal domain `used()`.
Fields drop backing before releasing its ticket, with lock-free infallible
release and no user callbacks. Slices/clones/send plans retain only selected
origins and charges; uncharged constructors remain available. Arc headers,
allocator usable size/transients and unlimited independent application-retained
metadata are not accounted. See the [client](../nfs3_client/README.md)
for live use and cancellation/uncertain-WRITE policy.

The private cursor reads scalars across chunks, validates lengths/padding and
shares selected bulk while copying bounded metadata. Use the public `Decoder`
below, and `finish` it to require exact consumption and release unselected chunks.

## Owner-aware protocol decoding

`Decode`, `Decoder`, `DecodeLimits`, `DecodeError` and `XdrDecode` provide checked
record decoding. `XdrCodec` generates both directions. Construct a `Decoder`
with a complete `Payload` record and explicit limits, call `decode::<T>()`, then
`finish()` to require exact consumption. No default policy is supplied.

Limits cover aggregate copied metadata bytes, aggregate collection elements,
active nesting depth, monotonically charged requested reservation work, and
per-field shared payload limits. Metadata charges its length before reservation;
collections grow incrementally with targets 1, 2, 4, ... and charge the **full**
target times host element size on every growth. Advertised counts never determine
reservations. Payload slicing charges exactly the selected nonempty descriptors
after per-view checks, before allocation. Empty values charge zero.
This is codec reservation work, **not** live/peak memory or allocator usable size.
It excludes arbitrary allocations in user implementations; nesting must go through
`Decoder::decode`, not direct calls to the trait method.

`Opaque<'a>` decodes to independent owned bytes for **every** lifetime. `Payload`
decodes shared bulk without passing through `Read`.
`nfs3::{READ3resok<D>, WRITE3args<D>}` are the canonical structs;
`READ3res<D>` is the corresponding result alias. Use `Payload` for shared bulk,
`&Payload` for borrowed encoding, and `Opaque<'a>` for explicitly copied metadata.
There are no declaration bounds, redundant bulk aliases or infallible payload clones.

READ/WRITE decoding is structural: mismatched count/data lengths are preserved,
not truncated or rejected. Exact operation validation (including authoritative
WRITE wording and contextual READ counts) belongs to operation policy; the
client validates responses, and the server applies its documented WRITE policy
before backend work.

The first value-only error is sticky, even if custom code swallows a nested error;
all subsequent operations and `finish` return it. Primitive cursor reads remain
failure-atomic; an aggregate may have consumed prior fields. The decoder retains
its input until `finish`/drop at latest, and errors retain no backing.

Decoding is strict on invalid inputs: strict zero padding,
strict boolean/enum tags, validated `mknoddata3` default tags, caller aggregate
limits and protocol bounds (auth body 400, `AUTH_SYS` machine name 255/groups 16,
NFS handle 64 and mount constants). Wire-unbounded names/paths use caller policy.
The existing closed `rpc::auth_flavor` enum does not represent every flavor
permitted by extensible ONC RPC.

## Checked segmented output

`Encode`, `Encoder`, `EncodeLimits`, `EncodeError`, `SendPlan` and `XdrEncode`
provide checked encoding, also generated by `XdrCodec`.
Call `Encoder::encode(&value)`, then consume the builder with `finish()`.
Only successful finish exposes output. The first value-only error is sticky,
including swallowed nested errors. Custom implementations must nest through
`Encoder::encode`; their independent allocations are outside the guarantee.

`SendPlan` owns flat metadata plus ordered index ranges and shared `Payload`
parts. It is `Send + Sync + 'static` and survives dropping the encoded input.
Adjacent metadata appends coalesce, but never across bulk. `chunks()` borrows
ordered nonempty slices without allocating; its iterator caches part/chunk
position, so a later writer can retain it and a byte offset across short writes.
`len()` is computed with checked additions during encoding, with no sizing pass
or whole-record Vec. RPC record markers and fragmentation remain external.

`Opaque` is **copied metadata**, even when borrowed. Select `READ3resok<Payload>`,
`READ3res<Payload>` or `WRITE3args<Payload>` for bulk: only descriptors/Arc owners are
cloned, without byte copies or retaining unselected origins. READ/WRITE counts
and data lengths are preserved independently; operation validation remains
outside the structural codec.

Every `EncodeLimits` field is required. Bounds cover total wire bytes, **all**
metadata bytes (scalars/tags/lengths/padding/bodies), emitted chunks (metadata
parts plus bulk descriptors), conservative retained bulk capacity, active nesting
and monotonic requested reservation work. Bulk capacity sums per descriptor/use:
shared origins can be counted more than once. Empty bulk retains nothing.
Before allocation, metadata/part vector growth charges its **full target** times
host element size: first `max(1, required)`, then `max(2 * previous, required)`,
using checked arithmetic and tracked logical targets. Payload clones charge exact
descriptor layouts; Arc increments allocate nothing. Reservation work is not
live/peak memory or allocator usable size, and excludes allocator overhead.
Actual allocator-selected vector capacities need not equal requested targets.
Failed builders retain partial storage until finish/drop; errors retain none.

XDR lengths/counts must fit u32; padding is zero. Known field bounds match
`Decode`. Impossible manual representations (`mknoddata3::default`,
`Nfs3Result::Err((NFS3_OK, _))`, `mountres3::Err(MNT3_OK)`) are rejected rather
than assigned invented tags. No numeric defaults or performance claim is implied.

```rust
use nfs3_types::nfs3::WRITE3args;
use nfs3_types::xdr_codec::{EncodeLimits, EncodeResult, Encoder, Payload, SendPlan};

fn prepare_write(value: &WRITE3args<&Payload>, policy: EncodeLimits) -> EncodeResult<SendPlan> {
    let mut encoder = Encoder::new(policy);
    encoder.encode(value)?;
    encoder.finish()
}
```
