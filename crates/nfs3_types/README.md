# `nfs3_types`

`nfs3_types` provides type definitions and utilities for working with `NFSv3`. Its main purpose is to be used with the `nfs3_server` crate, but it can also be used independently.

## Features

- Type definitions for `NFSv3` protocol, including Mount and Port Mapper
- XDR encoding and decoding
- Utilities for handling `NFSv3` operations

The opt-in `rpc-diagnostics` feature exposes `rpc_diagnostics`, a fixed,
allocation-free OS-thread-local logical IO/initialization/copy-site ledger.
It also enables `opaque-diagnostics`; neither feature changes the allocator.
Reset only before instrumented work starts and collect after quiescence. Counts
are not task attribution, exhaustive physical copies, or exact backing lifetimes.
