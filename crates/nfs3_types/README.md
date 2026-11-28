# `nfs3_types`

`nfs3_types` provides type definitions and utilities for working with `NFSv3`. Its main purpose is to be used with the `nfs3_server` crate, but it can also be used independently.

## Features

- Type definitions for `NFSv3` protocol, including Mount and Port Mapper
- XDR encoding and decoding on [`bytes`](https://docs.rs/bytes) buffers: large opaque payloads (READ/WRITE data) share the record buffer instead of being copied
- Utilities for handling `NFSv3` operations
