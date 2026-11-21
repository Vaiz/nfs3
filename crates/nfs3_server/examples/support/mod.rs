//! Intentional finite demonstration policy, not library or CLI defaults.

use std::time::Duration;

use nfs3_server::nfs3_types::async_io::RecordLimits;
use nfs3_server::tcp::{DecodePolicy, DirectoryLimits, EncodePolicy, ServerLimits};

pub const fn limits() -> ServerLimits {
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
        max_connections: 16,
        max_operations: 4,
        backing_bytes: 512 * 1024 * 1024,
        idle_timeout: Duration::from_secs(30),
        progress_timeout: Duration::from_secs(5),
        receive_timeout: Duration::from_secs(30),
        send_timeout: Duration::from_secs(30),
    }
}
