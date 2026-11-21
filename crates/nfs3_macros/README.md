# `nfs3_macros`

`XdrCodec` derives the checked `nfs3_types::xdr_codec::{Encode, Decode}` pair.
`XdrEncode` and `XdrDecode` derive only their respective direction. All three use
the same validated schema and generator, with independent directional bounds.

Supported forms:

- Named, tuple and unit structs, in field declaration order.
- Unit-only enums with non-negative u32 literal discriminants or implicit
  successive tags. Negative/expression discriminants are unsupported.
- Tagged unions with one `#[xdr(u32_literal)]` per arm. Arms are unit or
  single-field tuple variants; Rust discriminants are not accepted here.

Missing, duplicate, malformed and overflowing tags, Rust unions and unsupported
arm shapes produce compile errors. `xdr` is rejected on containers/fields and on
unit-only enum variants. Protocol field bounds and default union arms use manual
implementations, not an additional attribute language.

Lifetimes, const parameters and where clauses are preserved. Each type parameter
receives only the direction's `Encode` or `Decode` bound. Field-type bounds are
avoided so recursive `List` definitions do not create cyclic trait obligations.
Thus `#[derive(XdrCodec)] struct Value<T>(T)` can encode a borrowed `&Payload`
without requiring it to decode, or decode a decode-only type.

Nested fields/tags use `Encoder::encode` and `Decoder::decode` for aggregate depth
and sticky errors. Unknown tags produce `DecodeError::InvalidTag`. Generated
paths name `nfs3_types` directly. See that crate for explicit resource policies.

Migration: `XdrCodec` no longer generates the removed streaming `Pack`/`Unpack`
traits or unchecked size arithmetic. Encode into a checked `SendPlan`; decode a
complete owned record and require `Decoder::finish()`.
