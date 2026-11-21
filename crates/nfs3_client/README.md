# `nfs3_client`

An asynchronous `NFSv3` client with one checked RPC transport. Optional `tokio` and
`smol` features provide runtime IO/connectors; the core also builds without either.
`io::{AsyncRead, AsyncWrite}` reexports the runtime-neutral traits from
`nfs3_types::async_io`, shared with the server.

## Checked RPC and NFS clients

`rpc::RpcClient::new(io, limits, credential, verifier)` requires explicit
`rpc::RpcLimits` and returns `Result<_, RpcErrorKind>`. `Nfs3Client::new(rpc,
max_bulk_bytes)` adds READ/WRITE feasibility and semantic checks. There is no
parallel legacy transport or implicit unlimited policy.

All named procedures remain available: `null`, `getattr`, `setattr`, `lookup`,
`access`, `readlink`, `read`, `write`, `create`, `mkdir`, `symlink`, `mknod`,
`remove`, `rmdir`, `rename`, `link`, `readdir`, `readdirplus`, `fsstat`, `fsinfo`,
`pathconf`, `commit`. Generic RPC/NFS `call` requires `C: Encode + Sync, R: Decode`.
`MountClient` and `PortmapperClient` use the same checked core.

Each call reserves its **entire maximum reply backing** before request IO or
fails immediately. Initialized backing grows incrementally, independently of RPC
fragment boundaries, without read-ahead or whole-record assembly copies. Returned
`Payload` slices/clones/send plans retain original backing charges, even after
client drop. Drop retained outputs to recover admission capacity.

Limits have no defaults. NFS construction checks worst-case reply overhead,
depth, auth metadata, descriptors and unaligned bulk retention. READ rejects
requests above the bulk limit, tightens decode to requested count, and requires
count == data length and count <= requested. WRITE borrows `&Payload`, derives
its u32 wire count, and requires response count <= sent. Generic calls remain
structural, permitting independent raw procedure tests.

Dropping an unpolled call leaves state untouched. Pre-IO admission, encoding and
argument failures permit reuse. Immediately before IO the connection becomes
unusable; cancellation, partial IO, framing/decode/limit errors or failed semantic
checks leave it poisoned. Only complete valid replies with exact consumption
restore reuse, including valid RPC rejections and NFS errors.
`rpc::{RpcError, RpcErrorKind}` (also root reexports) implement Display/Error with
cause chaining. `RpcError` carries `kind`, `io_started`, `connection_usable`;
`is_connection_reusable()` returns that state. **A started WRITE with an unusable
connection has an uncertain outcome, not a safe retry.** There is no automatic
retry, reconnect or deadline.

Sending shares bulk bytes. Up to 1020 leading metadata bytes are additionally
copied onto a fixed marker stack buffer. Both runtime adapters use vectored IO;
custom IO may use the first-nonempty fallback. Short/zero/Interrupted IO is
handled explicitly. Actual accepted receive Vec capacities are charged;
transient allocator excess is rejected, not promised impossible. Domain budgets
exclude Arc/runtime/allocator overhead, arbitrary application metadata and other
clients: they are not whole-process memory bounds. There is no pool.

```rust
use nfs3_client::{Nfs3Client, RpcErrorKind};
use nfs3_client::rpc::{RpcClient, RpcLimits};
use nfs3_types::async_io::RecordLimits;
use nfs3_types::rpc::opaque_auth;
use nfs3_types::xdr_codec::{DecodeLimits, EncodeLimits, PayloadLimits};

// Chosen solely for this example, NOT recommended product defaults.
fn client<IO>(io: IO) -> Result<Nfs3Client<IO>, RpcErrorKind> {
    let policy = RpcLimits {
        record: RecordLimits {
            block_bytes: 4096, max_record_bytes: 16384, max_fragments: 128,
        },
        backing_bytes: 32768,
        decode: DecodeLimits {
            max_metadata_bytes: 4096, max_collection_elements: 1024,
            max_depth: 32, max_reservation_bytes: 65536,
            payload: PayloadLimits {
                max_logical_bytes: 8191, max_backing_capacity: 12288, max_segments: 3,
            },
        },
        encode: EncodeLimits {
            max_wire_bytes: 16384, max_metadata_bytes: 4096, max_chunks: 128,
            max_retained_capacity: 16384, max_depth: 32, max_reservation_bytes: 65536,
        },
    };
    let rpc = RpcClient::new(io, policy, opaque_auth::default(), opaque_auth::default())?;
    Nfs3Client::new(rpc, 8191)
}
```

With an existing file handle use `client.read(&READ3args { file, offset,
count }).await`. Successful data is a `Payload`: iterate `chunks()`, share via
`try_slice()`/`try_clone()`, or copy explicitly with `try_to_vec()`.
`client.write(file, offset, stable_how::FILE_SYNC, &payload).await` borrows the
input, including on pre-IO rejection and unpolled future drop.

## Connection builder and examples

`Nfs3ConnectionBuilder::new(connector, host, mount_path, limits, max_bulk_bytes)?`
validates explicit policy before IO. Existing port overrides, credential/verifier
setters and `connect_from_privileged_port(false)` remain. Privileged source ports
are attempted by default. `mount().await` resolves missing ports, mounts, then
constructs the NFS client; failures propagate as `ConnectError`, not a usable
placeholder connection. `Nfs3Connection` retains root handle, Deref to the NFS
client, unmount and conversion to a long-lived NFS client.

Use `TokioConnector`/`SmolConnector` for builders or `TokioIo::new(stream)`/
`SmolIo::new(stream)` for existing IO. The `nfs`, `nfs_smol`, `ls`,
`download_folder` and `portmap` examples supply documented finite policy in
`examples/support`. Downloads write each payload chunk without coalescing.

Migration from the old API: constructors are fallible and require policy;
`RpcClient::new_with_auth` is replaced by the one explicit constructor.
NFS READ returns `READ3res<Payload>`; WRITE takes file/offset/stability/`&Payload`
rather than a copied opaque argument. Old RPC enum errors become `RpcErrorKind`
plus state flags (e.g. unavailable procedure is
`NotAccepted(accept_stat_data::PROC_UNAVAIL)`). The interim bounded client names
are removed, not aliases.
