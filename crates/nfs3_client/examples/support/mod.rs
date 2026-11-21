use nfs3_client::rpc::RpcLimits;
use nfs3_types::async_io::RecordLimits;
use nfs3_types::xdr_codec::{DecodeLimits, EncodeLimits, PayloadLimits};

/// Explicit example policy: 1 MiB transfers, 2 MiB records/metadata and retained
/// output plus another record. Applications must select their own policy.
pub const fn limits() -> RpcLimits {
    RpcLimits {
        record: RecordLimits {
            block_bytes: 64 * 1024,
            max_record_bytes: 2 * 1024 * 1024,
            max_fragments: 4096,
        },
        backing_bytes: 4 * 1024 * 1024,
        decode: DecodeLimits {
            max_metadata_bytes: 2 * 1024 * 1024,
            max_collection_elements: 65536,
            max_depth: 64,
            max_reservation_bytes: 16 * 1024 * 1024,
            payload: PayloadLimits {
                max_logical_bytes: 1024 * 1024,
                max_backing_capacity: 2 * 1024 * 1024,
                max_segments: 32,
            },
        },
        encode: EncodeLimits {
            max_wire_bytes: 2 * 1024 * 1024,
            max_metadata_bytes: 2 * 1024 * 1024,
            max_chunks: 256,
            max_retained_capacity: 2 * 1024 * 1024,
            max_depth: 64,
            max_reservation_bytes: 16 * 1024 * 1024,
        },
    }
}
