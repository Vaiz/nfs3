use std::fmt;
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::transaction_tracker::TransactionTracker;
use crate::vfs::NfsFileSystem;
use crate::vfs::handle::FileHandleConverter;

/// Context shared by calls on a connection. Cloning increments one reference
/// count and does not allocate.
pub struct RPCContext<T: NfsFileSystem>(Arc<Inner<T>>);

struct Inner<T> {
    client_addr: String,
    server: Arc<ServerState<T>>,
}

pub(crate) struct ServerState<T> {
    vfs: Arc<T>,
    transaction_tracker: Arc<TransactionTracker>,
    export_name: String,
    mount_signal: Option<mpsc::Sender<bool>>,
    file_handle_converter: FileHandleConverter,
    local_port: u16,
}

impl<T> ServerState<T> {
    pub(crate) fn new(
        local_port: u16,
        vfs: Arc<T>,
        transaction_tracker: Arc<TransactionTracker>,
        file_handle_converter: FileHandleConverter,
    ) -> Self {
        Self {
            local_port,
            vfs,
            transaction_tracker,
            export_name: "/".to_owned(),
            mount_signal: None,
            file_handle_converter,
        }
    }

    pub(crate) fn set_export_name(&mut self, export_name: String) {
        self.export_name = export_name;
    }

    pub(crate) fn set_mount_listener(&mut self, signal: mpsc::Sender<bool>) {
        self.mount_signal = Some(signal);
    }

    pub(crate) fn clone_transaction_tracker(&self) -> Arc<TransactionTracker> {
        Arc::clone(&self.transaction_tracker)
    }
}

// A listener setting change must not change existing connections' settings
// or split the filesystem and transaction tracker across configurations.
impl<T> Clone for ServerState<T> {
    fn clone(&self) -> Self {
        Self {
            vfs: Arc::clone(&self.vfs),
            transaction_tracker: Arc::clone(&self.transaction_tracker),
            export_name: self.export_name.clone(),
            mount_signal: self.mount_signal.clone(),
            file_handle_converter: self.file_handle_converter,
            local_port: self.local_port,
        }
    }
}

impl<T: NfsFileSystem> RPCContext<T> {
    pub(crate) fn new(client_addr: String, server: Arc<ServerState<T>>) -> Self {
        Self(Arc::new(Inner {
            client_addr,
            server,
        }))
    }

    pub(crate) fn local_port(&self) -> u16 {
        self.0.server.local_port
    }

    pub(crate) fn client_addr(&self) -> &str {
        &self.0.client_addr
    }

    pub(crate) fn vfs(&self) -> &T {
        &self.0.server.vfs
    }

    pub(crate) fn mount_signal(&self) -> Option<&mpsc::Sender<bool>> {
        self.0.server.mount_signal.as_ref()
    }

    pub(crate) fn export_name(&self) -> &str {
        &self.0.server.export_name
    }

    pub(crate) fn transaction_tracker(&self) -> &TransactionTracker {
        &self.0.server.transaction_tracker
    }

    pub(crate) fn file_handle_converter(&self) -> &FileHandleConverter {
        &self.0.server.file_handle_converter
    }
}

impl<T> fmt::Debug for RPCContext<T>
where
    T: NfsFileSystem,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("RPCContext")
            .field("local_port", &self.local_port())
            .field("client_addr", &self.client_addr())
            .field("mount_signal", &self.mount_signal())
            .field("export_name", &self.export_name())
            .field("transaction_tracker", &self.transaction_tracker())
            .finish_non_exhaustive()
    }
}

impl<T> Clone for RPCContext<T>
where
    T: NfsFileSystem,
{
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

#[doc(hidden)]
#[cfg(feature = "__test_reexports")]
impl<T> RPCContext<T>
where
    T: NfsFileSystem + 'static,
{
    pub fn test_ctx(export_name: &str, vfs: Arc<T>) -> Self {
        let mut server = ServerState::new(
            2049,
            vfs,
            Arc::new(TransactionTracker::new(
                std::time::Duration::from_mins(1),
                256,
                1024,
            )),
            FileHandleConverter::new(),
        );
        server.set_export_name(export_name.to_owned());
        Self::new("localhost".to_owned(), Arc::new(server))
    }
    #[must_use]
    pub fn root_dir(&self) -> nfs3_types::nfs3::nfs_fh3 {
        self.file_handle_converter()
            .fh_to_nfs(&self.vfs().root_dir())
    }
}

#[cfg(all(test, feature = "__test_reexports", feature = "memfs"))]
mod tests {
    use super::*;
    use crate::memfs::{MemFs, MemFsConfig};

    #[test]
    fn clone_shares_connection_state() {
        let fs = MemFs::new(MemFsConfig::default()).expect("initialize the test filesystem");
        let context = RPCContext::test_ctx("/mnt", Arc::new(fs));
        assert!(Arc::ptr_eq(&context.0, &context.clone().0));
    }
}
