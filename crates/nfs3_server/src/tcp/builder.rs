use std::io;
use std::num::NonZeroUsize;

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
    /// Creates a builder with no incoming RPC record size limit.
    pub fn new() -> Self {
        Self::default()
    }

    /// Supplies a filesystem.
    pub fn filesystem<T: NfsFileSystem + 'static>(self, fs: T) -> Builder<T> {
        Builder {
            fs,
            options: self.options,
        }
    }

    /// Supplies a filesystem that only supports reads.
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
    /// Sets the NFS export path (default: `/`). For example, `data` and `/data/`
    /// both export the filesystem at `/data`.
    pub fn export_name(mut self, name: impl AsRef<str>) -> Self {
        self.options.export_name = Some(normalized_export_name(name.as_ref()));
        self
    }

    /// Sets the file handle generation number. Instances sharing the same filesystem
    /// can use the same generation number to keep file handles valid across instances.
    pub const fn generation_number(mut self, generation_number: u64) -> Self {
        self.options.generation_number = Some(generation_number);
        self
    }

    /// Sets the maximum total payload bytes in an incoming RPC record, excluding
    /// fragment markers. Records larger than the limit cause the connection to close.
    /// By default there is no limit.
    pub const fn max_rpc_record_size(mut self, max_rpc_record_size: NonZeroUsize) -> Self {
        self.options.max_rpc_record_size = Some(max_rpc_record_size);
        self
    }
}

impl<T: NfsFileSystem + 'static> Builder<T> {
    /// Binds to an `ip:port` address. Use `auto:port` to select a loopback address
    /// or port `0` to let the OS choose a port.
    pub async fn bind(self, addr: &str) -> io::Result<NFSTcpListener<T>> {
        NFSTcpListener::bind_inner(addr, self.fs, self.options).await
    }
}
