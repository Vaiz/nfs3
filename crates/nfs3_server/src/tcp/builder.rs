use std::io;
use std::net::SocketAddr;
use std::num::NonZeroUsize;

use nfs3_types::mount::dirpath;
use tokio::net::TcpListener;
use tracing::info;

use super::{DEFAULT_EXPORT_PATH, NFSTcpListener};
use crate::context::normalized_export_path;
use crate::vfs::adapters::ReadOnlyAdapter;
use crate::vfs::{NfsFileSystem, NfsReadFileSystem};

/// Configures an NFS TCP listener. Select a filesystem before calling [`Builder::bind`].
#[must_use]
pub struct Builder<FS = ()> {
    fs: FS,
    export_path: io::Result<dirpath<'static>>,
    hide_exports: bool,
    exact_export_match: bool,
    generation_number: Option<u64>,
    max_rpc_record_size: Option<NonZeroUsize>,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            fs: (),
            export_path: Ok(DEFAULT_EXPORT_PATH.clone()),
            hide_exports: false,
            exact_export_match: false,
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
            export_path: self.export_path,
            hide_exports: self.hide_exports,
            exact_export_match: self.exact_export_match,
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
    /// [`Builder::bind`] rejects normalized paths longer than 1,024 bytes.
    pub fn export_name(mut self, name: impl AsRef<str>) -> Self {
        self.export_path = normalized_export_path(name.as_ref());
        self
    }

    /// Makes `MOUNTPROC3_EXPORT` return an empty export list so the export path is not disclosed.
    pub const fn hidden_exports(mut self) -> Self {
        self.hide_exports = true;
        self
    }

    /// Requires `MOUNTPROC3_MNT` to match the configured export path verbatim,
    /// refusing subpath and prefix mounts.
    pub const fn exact_export_match(mut self) -> Self {
        self.exact_export_match = true;
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
    ///
    /// Returns `InvalidInput` before binding if the normalized export path
    /// exceeds 1,024 bytes. Initialization also queries the export root's
    /// FSINFO to set the READ limit and returns an error if that query fails.
    pub async fn bind(self, addr: SocketAddr) -> io::Result<NFSTcpListener<T>> {
        let export_path = self.export_path?;
        let listener = TcpListener::bind(addr).await?;
        info!("Listening on {addr}");
        NFSTcpListener::new(
            listener,
            self.fs,
            export_path,
            self.hide_exports,
            self.exact_export_match,
            self.generation_number,
            self.max_rpc_record_size,
        )
        .await
    }
}
