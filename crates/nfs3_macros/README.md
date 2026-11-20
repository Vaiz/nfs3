# `nfs3_macros`

`nfs3_macros` is a Rust library that provides `XdrCodec` derive macro for the `nfs3_types` crate. `XdrCodec` macro automatically implements `Pack` and `Unpack` traits for structs and enums, simplifying serialization and deserialization of XDR encoded data.
## Features

- Automatically implements `Pack` and `Unpack` traits for structs and enums.
- Supports named, unnamed, and unit structs.
- Supports unit-only enums.

## Owner-aware `XdrDecode`

`XdrDecode` independently implements `nfs3_types::xdr_codec::Decode`; it does not
alter `XdrCodec` or require legacy streaming traits. Named, tuple and unit structs,
unit enums with non-negative u32 literal discriminants (or implicit successive tags), and tagged
unions with `#[xdr(u32_tag)]` on every arm are supported. Union arms are unit or
single-field tuple variants. Invalid/missing/duplicate tags, unsupported shapes
and Rust unions produce compile errors.
All-unit enums reject `#[xdr]` attributes: legacy `Pack` uses their Rust
discriminants, not attribute tags. Negative and expression discriminants are unsupported.

Lifetimes, const parameters and where clauses are preserved. Only type parameters
receive an added `Decode` bound; field-type bounds are deliberately avoided so
recursive `List` definitions do not create cyclic trait obligations. Every nested
field uses `Decoder::decode` for aggregate depth and sticky-error enforcement.
Protocol field bounds/default union arms use manual implementations, not an
additional attribute language. See `nfs3_types` for explicit resource policy.

## Checked `XdrEncode`

`XdrEncode` independently implements `nfs3_types::xdr_codec::Encode`, with the
same supported forms and shared validated tag interpretation as `XdrDecode`.
It preserves generic/lifetime/const parameters and where clauses, adds `Encode`
bounds to type parameters, and nests every field/tag through `Encoder::encode`.
It can be derived alongside `XdrDecode` without requiring `Pack`/`Unpack`.
Manual implementations handle protocol bounds and default union arms.

All-unit discriminants must be non-negative u32 literals (or implicit successive
values); `#[xdr]` tags on all-unit enums, negative/expression discriminants,
missing/duplicate/overflowing tags and unsupported union shapes produce compile
errors. Neither new derive changes legacy `XdrCodec` parsing or behavior.
See `nfs3_types` for explicit encoding limits and owned segmented `SendPlan` output.
