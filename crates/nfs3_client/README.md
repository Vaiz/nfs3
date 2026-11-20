# `nfs3_client`

`nfs3_client` is a Rust crate that provides an asynchronous client implementation for interacting with `NFSv3` servers. It includes functionality for connecting to NFS servers, performing various NFS operations, and handling the underlying RPC communication.

## Features

This crate supports multiple async runtimes via feature flags:

- `tokio` - Support for Tokio runtime
- `smol` - Support for Smol runtime

## Opt-in bounded client

The IO traits remain available at `nfs3_client::io` as compatibility reexports
from `nfs3_types::async_io`. Client and bounded server share that runtime-neutral
framing implementation; the types crate has no runtime dependency.

`bounded::BoundedRpcClient` accepts the existing runtime-neutral IO traits and
explicit `RpcLimits`. Use `TokioIo` or `SmolIo` around an already connected stream.
It sends `Encode` arguments and decodes `Decode` results; legacy clients and
listeners do not opt in automatically. Mount/port lookup and reconnect/retry
are not added here.
`bounded_nfs::BoundedNfsClient` adds only READ, WRITE and generic NFS `call`.

Every call reserves its **entire maximum reply backing** before request IO or
fails immediately. Backing is allocated incrementally into initialized final
blocks, independently of RPC fragment boundaries, with no read-ahead. Returned
`Payload` slices/clones/SendPlans keep the original backing charge, even after
client drop. Drop all retained outputs (including shared send plans) to recover
admission capacity. Independent client domains are not a process-wide limit.
Reservation work bounds descriptors/control layouts, not allocator usable size,
Arc headers or arbitrary application metadata retained after decode. Actual
accepted receive Vec capacities are charged; transient allocator over-allocation
is rejected, not promised impossible. There is no pool.

Limits have no defaults. NFS construction additionally checks worst-case valid
READ RPC/auth/attributes/padding overhead and unaligned payload retention.
READ requests above the configured bulk bound fail before IO; each request
tightens the decode payload length bound. READ requires count == data length
and count <= requested. WRITE takes `Payload`, derives its u32 wire count, and
requires the returned count <= sent. Generic RPC/NFS calls remain structural.

Dropping an unpolled call leaves the client untouched. Pre-IO admission, encoding
and argument failures permit reuse. Immediately before IO the connection becomes
unusable; cancellation, partial IO, framing/decode/limit errors or failed NFS
semantic checks leave it poisoned. Only complete valid replies with exact
consumption restore reuse, including valid RPC rejections and NFS errors.
`BoundedError` exposes `io_started` and `connection_usable`; **a started WRITE
with an unusable connection has an uncertain outcome, not a safe retry**.
Diagnostics are bounded values, except `std::io::Error` may contain a custom
transport-owned inner error.

Sending shares bulk bytes. The transport additionally copies at most 1020 leading
metadata bytes onto a fixed stack buffer with the marker, so small requests take
one write even on custom IO's first-nonempty vectored fallback. Both runtime
adapters support real vectored writes. Short/zero/Interrupted IO is handled
explicitly. This small metadata copy and receive zero-fill are real costs;
no measured copy/memory/performance target pass is implied.

```rust
use nfs3_client::bounded::{BoundedErrorKind, BoundedRpcClient, RpcLimits};
use nfs3_client::bounded_nfs::BoundedNfsClient;
use nfs3_types::rpc::opaque_auth;
use nfs3_types::xdr_codec::{DecodeLimits, EncodeLimits, PayloadLimits, RecordBuilder};

// Chosen solely for this example, NOT recommended product defaults.
fn bounded<IO>(io: IO) -> Result<BoundedNfsClient<IO>, BoundedErrorKind> {
    let policy = RpcLimits {
        block_bytes: 4096,
        max_record_bytes: 16384,
        max_fragments: 128,
        backing_bytes: 32768,
        max_builder_reservation_bytes: RecordBuilder::reservation_bound(4)
            .ok_or(BoundedErrorKind::Configuration)?,
        decode: DecodeLimits {
            max_metadata_bytes: 4096,
            max_collection_elements: 1024,
            max_depth: 32,
            max_reservation_bytes: 65536,
            payload: PayloadLimits {
                max_logical_bytes: 8191,
                max_backing_capacity: 12288,
                max_segments: 3,
            },
        },
        encode: EncodeLimits {
            max_wire_bytes: 16384,
            max_metadata_bytes: 4096,
            max_chunks: 128,
            max_retained_capacity: 16384,
            max_depth: 32,
            max_reservation_bytes: 65536,
        },
    };
    let rpc = BoundedRpcClient::new(
        io, policy, opaque_auth::default(), opaque_auth::default(),
    )?;
    BoundedNfsClient::new(rpc, 8191)
}
```

With an existing NFS file handle, call `client.read(&READ3args { file, offset,
count }).await`; successful data is a `Payload` (`chunks()`, `try_slice()`,
explicit copying with `try_to_vec()`). Use
`client.write(file, offset, stable_how::FILE_SYNC, payload).await` to send it.

## Examples

### Using with Tokio

```rust,no_run
# #[cfg(feature = "tokio")]
use nfs3_client::tokio::TokioConnector;
# #[cfg(feature = "tokio")]
use nfs3_client::Nfs3ConnectionBuilder;
# #[cfg(feature = "tokio")]
use nfs3_types::nfs3;

# #[cfg(feature = "tokio")]
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut connection = Nfs3ConnectionBuilder::new(TokioConnector, "127.0.0.1", "/")
        .mount()
        .await?;

    let root = connection.root_nfs_fh3();

    println!("Calling readdir");
    let readdir = connection
        .readdir(&nfs3::READDIR3args {
            dir: root,
            cookie: 0,
            cookieverf: nfs3::cookieverf3::default(),
            count: 128 * 1024 * 1024,
        })
        .await?;

    println!("{readdir:?}");

    connection.unmount().await?;

    Ok(())
}
# #[cfg(not(feature = "tokio"))]
# fn main() {}
```

### Using with Smol

```rust,no_run
# #[cfg(feature = "smol")]
use nfs3_client::smol::SmolConnector;
# #[cfg(feature = "smol")]
use nfs3_client::Nfs3ConnectionBuilder;
# #[cfg(feature = "smol")]
use nfs3_client::nfs3_types::nfs3;

# #[cfg(feature = "smol")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    smol::block_on(async {
        let mut connection = Nfs3ConnectionBuilder::new(SmolConnector, "127.0.0.1", "/")
            .mount()
            .await?;

        let root = connection.root_nfs_fh3();

        println!("Calling readdir");
        let readdir = connection
            .readdir(&nfs3::READDIR3args {
                dir: root.clone(),
                cookie: 0,
                cookieverf: nfs3::cookieverf3::default(),
                count: 128 * 1024 * 1024,
            })
            .await?;

        println!("{readdir:?}");

        connection.unmount().await?;

        Ok(())
    })
}
# #[cfg(not(feature = "smol"))]
# fn main() {}
```
