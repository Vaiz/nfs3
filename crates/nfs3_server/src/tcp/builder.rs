use std::io;
use std::net::SocketAddr;
use std::num::NonZeroUsize;
use std::sync::Arc;

use tokio::net::TcpListener;
use tracing::info;

use super::{NFSTcpListener, normalized_export_name};
use crate::vfs::adapters::ReadOnlyAdapter;
use crate::vfs::{NfsFileSystem, NfsReadFileSystem};

/// Configures an NFS TCP listener. Select a filesystem before calling [`Builder::bind`].
#[must_use]
pub struct Builder<FS = ()> {
    fs: FS,
    export_name: Arc<str>,
    generation_number: Option<u64>,
    max_rpc_record_size: Option<NonZeroUsize>,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            fs: (),
            export_name: Arc::from("/"),
            generation_number: None,
            max_rpc_record_size: None,
        }
    }
}

impl Builder {
    /// Creates a listener builder with default options and no filesystem.
    pub fn new() -> Self {
        Self::default()
    }

    /// Selects the filesystem to export.
    pub fn filesystem<T: NfsFileSystem + 'static>(self, fs: T) -> Builder<T> {
        Builder {
            fs,
            export_name: self.export_name,
            generation_number: self.generation_number,
            max_rpc_record_size: self.max_rpc_record_size,
        }
    }

    /// Selects the filesystem to export with read-only access.
    pub fn read_only_filesystem<RO: NfsReadFileSystem + 'static>(
        self,
        fs: RO,
    ) -> Builder<ReadOnlyAdapter<RO>> {
        self.filesystem(ReadOnlyAdapter::new(fs))
    }
}

impl<FS> Builder<FS> {
    /// Sets the NFS export path (default: `/`).
    ///
    /// Leading and trailing slashes are stripped before adding one leading slash.
    /// For example, `data` and `/data/` both export at `/data`.
    pub fn export_name(mut self, name: impl AsRef<str>) -> Self {
        self.export_name = normalized_export_name(name.as_ref());
        self
    }

    /// Sets the generation number embedded in file handles.
    ///
    /// By default a generation number is generated for each listener. Servers sharing the
    /// same filesystem can use the same generation number to keep handles valid across instances.
    pub const fn generation_number(mut self, generation_number: u64) -> Self {
        self.generation_number = Some(generation_number);
        self
    }

    /// Limits the combined payload bytes of all fragments in an incoming RPC record.
    ///
    /// Fragment markers do not count toward the limit. The server closes the connection if
    /// a record exceeds the limit. By default there is no limit.
    pub const fn max_rpc_record_size(mut self, limit: NonZeroUsize) -> Self {
        self.max_rpc_record_size = Some(limit);
        self
    }
}

impl<T: NfsFileSystem + 'static> Builder<T> {
    /// Binds to an IPv4 or IPv6 socket address.
    ///
    /// Use port `0` to let the OS choose an available port. The deprecated `auto:port`
    /// syntax is only supported by the legacy string constructors.
    pub async fn bind(self, addr: SocketAddr) -> io::Result<NFSTcpListener<T>> {
        let listener = TcpListener::bind(addr).await?;
        info!("Listening on {addr}");
        Ok(NFSTcpListener::from_listener(
            listener,
            self.fs,
            self.export_name,
            self.generation_number,
            self.max_rpc_record_size,
        ))
    }
}
