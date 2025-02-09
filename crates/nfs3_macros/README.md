# nfs3_macros

`nfs3_macros` is a Rust library that provides `XdrCodec` derive macro for the `nfs3_types` crate. `XdrCodec` macro automatically implements `Pack` and `Unpack` traits for structs and enums, simplifying serialization and deserialization of XDR encoded data.
## Features

- Automatically implements `Pack` and `Unpack` traits for structs and enums.
- Supports named, unnamed, and unit structs.
- Supports unit-only enums.
