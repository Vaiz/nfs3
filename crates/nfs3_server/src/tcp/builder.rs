use std::io;
use std::net::SocketAddr;
use std::num::NonZeroUsize;

use tokio::net::TcpListener;

use super::{ListenerOptions, NFSTcpListener, normalized_export_name};
use crate::vfs::adapters::ReadOnlyAdapter;
use crate::vfs::{NfsFileSystem, NfsReadFileSystem};

/// Configures a TCP listener. Supply a filesystem before calling [`Builder::bind`].
#[must_use]
pub struct Builder<FS = ()> {
    fs: FS,
    options: ListenerOptions,
}

impl Builder {
    /// Creates a builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the filesystem to export.
    pub fn filesystem<T: NfsFileSystem + 'static>(self, fs: T) -> Builder<T> {
        Builder {
            fs,
            options: self.options,
        }
    }

    /// Sets the filesystem to export and limits it to read-only operations.
    ///
    /// It uses [`ReadOnlyAdapter`] internally, which overrides all modifying operations to enforce
    /// read-only access.
    pub fn read_only_filesystem<RO: NfsReadFileSystem + 'static>(
        self,
        fs: RO,
    ) -> Builder<ReadOnlyAdapter<RO>> {
        self.filesystem(ReadOnlyAdapter::new(fs))
    }
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            fs: (),
            options: ListenerOptions::default(),
        }
    }
}

impl<FS> Builder<FS> {
    /// Sets the NFS export path (default: `/`).
    ///
    /// For example, `data` and `/data/` both export the filesystem at `/data`.
    pub fn export_name(mut self, name: impl AsRef<str>) -> Self {
        self.options.export_name = Some(normalized_export_name(name.as_ref()));
        self
    }

    /// Sets the file handle generation number.
    ///
    /// Instances sharing the same filesystem can use the same generation number to keep file
    /// handles valid across instances.
    pub const fn generation_number(mut self, generation_number: u64) -> Self {
        self.options.generation_number = Some(generation_number);
        self
    }

    /// Sets the maximum total payload bytes in an incoming RPC record.
    ///
    /// It excludes fragment markers. Records larger than the limit cause the connection to close.
    /// By default there is no limit.
    pub const fn max_rpc_record_size(mut self, max_rpc_record_size: NonZeroUsize) -> Self {
        self.options.max_rpc_record_size = Some(max_rpc_record_size);
        self
    }
}

impl<T: NfsFileSystem + 'static> Builder<T> {
    /// Binds to an IPv4 or IPv6 socket address.
    ///
    /// Use port `0` to let the OS choose an available port. The legacy
    /// `auto:port` mode is deprecated; pass an explicit loopback IP instead.
    pub async fn bind(self, addr: SocketAddr) -> io::Result<NFSTcpListener<T>> {
        let listener = TcpListener::bind(addr).await?;
        NFSTcpListener::from_listener(listener, self.fs, self.options)
    }
}
