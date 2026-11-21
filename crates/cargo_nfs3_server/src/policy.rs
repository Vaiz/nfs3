//! Required independent operational policy; no automatic numeric profile.

use std::time::Duration;

use nfs3_server::nfs3_types::async_io::RecordLimits;
use nfs3_server::tcp::{DecodePolicy, DirectoryLimits, EncodePolicy, ServerLimits};

#[derive(clap::Args, Debug)]
pub struct Policy {
    /// Receive block size in bytes
    #[arg(long)]
    record_block_bytes: usize,
    /// Maximum complete RPC record in bytes
    #[arg(long)]
    record_max_bytes: usize,
    /// Maximum fragments per record, including empty fragments
    #[arg(long)]
    record_max_fragments: usize,
    #[arg(long)]
    decode_metadata_bytes: usize,
    #[arg(long)]
    decode_collection_elements: usize,
    #[arg(long)]
    decode_depth: usize,
    #[arg(long)]
    decode_reservation_bytes: usize,
    #[arg(long)]
    encode_wire_bytes: usize,
    #[arg(long)]
    encode_metadata_bytes: usize,
    #[arg(long)]
    encode_chunks: usize,
    #[arg(long)]
    encode_depth: usize,
    #[arg(long)]
    encode_reservation_bytes: usize,
    #[arg(long)]
    read_max_bytes: u32,
    #[arg(long)]
    write_max_bytes: u32,
    #[arg(long)]
    directory_wire_bytes: u32,
    #[arg(long)]
    directory_entries: usize,
    #[arg(long)]
    directory_name_bytes: usize,
    #[arg(long)]
    directory_metadata_bytes: usize,
    #[arg(long)]
    max_connections: u32,
    #[arg(long)]
    max_operations: u32,
    /// Listener-wide backing/operation entitlement, not a process memory cap
    #[arg(long)]
    backing_bytes: usize,
    #[arg(long)]
    idle_timeout_ms: u64,
    #[arg(long)]
    progress_timeout_ms: u64,
    #[arg(long)]
    receive_timeout_ms: u64,
    #[arg(long)]
    send_timeout_ms: u64,
}

impl Policy {
    pub fn limits(&self) -> std::io::Result<ServerLimits> {
        ServerLimits {
            record: RecordLimits {
                block_bytes: self.record_block_bytes,
                max_record_bytes: self.record_max_bytes,
                max_fragments: self.record_max_fragments,
            },
            decode: DecodePolicy {
                max_metadata_bytes: self.decode_metadata_bytes,
                max_collection_elements: self.decode_collection_elements,
                max_depth: self.decode_depth,
                max_reservation_bytes: self.decode_reservation_bytes,
            },
            encode: EncodePolicy {
                max_wire_bytes: self.encode_wire_bytes,
                max_metadata_bytes: self.encode_metadata_bytes,
                max_chunks: self.encode_chunks,
                max_depth: self.encode_depth,
                max_reservation_bytes: self.encode_reservation_bytes,
            },
            max_read_bytes: self.read_max_bytes,
            max_write_bytes: self.write_max_bytes,
            directory: DirectoryLimits {
                max_wire_bytes: self.directory_wire_bytes,
                max_entries: self.directory_entries,
                max_name_bytes: self.directory_name_bytes,
                max_metadata_bytes: self.directory_metadata_bytes,
            },
            max_connections: self.max_connections,
            max_operations: self.max_operations,
            backing_bytes: self.backing_bytes,
            idle_timeout: Duration::from_millis(self.idle_timeout_ms),
            progress_timeout: Duration::from_millis(self.progress_timeout_ms),
            receive_timeout: Duration::from_millis(self.receive_timeout_ms),
            send_timeout: Duration::from_millis(self.send_timeout_ms),
        }
        .validate()
    }
}
