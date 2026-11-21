use nfs3_client::nfs3_types::nfs3::nfs_fh3;
use nfs3_server::tcp::ServerLimits;
use nfs3_server::test_reexports::TestConnection;
use nfs3_server::vfs::NfsFileSystem;

pub struct Server<IO, FS: NfsFileSystem> {
    connection: TestConnection<FS>,
    io: IO,
}

impl<IO, FS> Server<IO, FS>
where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    FS: NfsFileSystem + 'static,
{
    pub fn new(io: IO, memfs: FS, limits: ServerLimits) -> anyhow::Result<Self> {
        let connection = TestConnection::new(memfs, "/mnt", limits)?;
        let this = Self { connection, io };
        Ok(this)
    }

    pub fn root_dir(&self) -> nfs_fh3 {
        self.connection.root_dir()
    }

    pub async fn run(self) -> Result<(), anyhow::Error> {
        self.connection.run(self.io).await;
        Ok(())
    }
}

/// Intentional finite workspace-fixture policy, not library or CLI defaults.
///
/// Covers the existing 1 MiB bulk/large-directory scenarios and reserves one
/// operation at a time. Bench source uses the same engine/policy; historical
/// ignored measurements are not modified or reinterpreted.
#[must_use]
pub const fn server_limits() -> ServerLimits {
    use std::time::Duration;

    use nfs3_server::nfs3_types::async_io::RecordLimits;
    use nfs3_server::tcp::{DecodePolicy, DirectoryLimits, EncodePolicy};

    ServerLimits {
        record: RecordLimits {
            block_bytes: 64 * 1024,
            max_record_bytes: 2 * 1024 * 1024,
            max_fragments: 4096,
        },
        decode: DecodePolicy {
            max_metadata_bytes: 2 * 1024 * 1024,
            max_collection_elements: 65536,
            max_depth: 64,
            max_reservation_bytes: 16 * 1024 * 1024,
        },
        encode: EncodePolicy {
            max_wire_bytes: 2 * 1024 * 1024,
            max_metadata_bytes: 2 * 1024 * 1024,
            max_chunks: 256,
            max_depth: 64,
            max_reservation_bytes: 16 * 1024 * 1024,
        },
        max_read_bytes: 1024 * 1024,
        max_write_bytes: 1024 * 1024,
        directory: DirectoryLimits {
            max_wire_bytes: 1024 * 1024,
            max_entries: 10000,
            max_name_bytes: 4096,
            max_metadata_bytes: 2 * 1024 * 1024,
        },
        max_connections: 1,
        max_operations: 1,
        backing_bytes: 128 * 1024 * 1024,
        idle_timeout: Duration::from_secs(30),
        progress_timeout: Duration::from_secs(5),
        receive_timeout: Duration::from_secs(30),
        send_timeout: Duration::from_secs(30),
    }
}

/// Explicit client policy for workspace fixtures, matching their server record
/// and bulk envelopes; retained output can coexist with the next admitted reply.
#[must_use]
pub const fn client_limits() -> nfs3_client::rpc::RpcLimits {
    use nfs3_client::nfs3_types::xdr_codec::{DecodeLimits, EncodeLimits, PayloadLimits};
    let server = server_limits();
    nfs3_client::rpc::RpcLimits {
        record: server.record,
        backing_bytes: 4 * 1024 * 1024,
        decode: DecodeLimits {
            max_metadata_bytes: server.decode.max_metadata_bytes,
            max_collection_elements: server.decode.max_collection_elements,
            max_depth: server.decode.max_depth,
            max_reservation_bytes: server.decode.max_reservation_bytes,
            payload: PayloadLimits {
                max_logical_bytes: 1024 * 1024,
                max_backing_capacity: 2 * 1024 * 1024,
                max_segments: 32,
            },
        },
        encode: EncodeLimits {
            max_wire_bytes: server.encode.max_wire_bytes,
            max_metadata_bytes: server.encode.max_metadata_bytes,
            max_chunks: server.encode.max_chunks,
            max_retained_capacity: 2 * 1024 * 1024,
            max_depth: server.encode.max_depth,
            max_reservation_bytes: server.encode.max_reservation_bytes,
        },
    }
}
