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
    local_port: u16,
    client_addr: String,
    vfs: Arc<T>,
    mount_signal: Option<mpsc::Sender<bool>>,
    export_name: Arc<String>,
    transaction_tracker: Arc<TransactionTracker>,
    file_handle_converter: FileHandleConverter,
}

impl<T: NfsFileSystem> RPCContext<T> {
    pub(crate) fn new(
        local_port: u16,
        client_addr: String,
        vfs: Arc<T>,
        mount_signal: Option<mpsc::Sender<bool>>,
        export_name: Arc<String>,
        transaction_tracker: Arc<TransactionTracker>,
        file_handle_converter: FileHandleConverter,
    ) -> Self {
        Self(Arc::new(Inner {
            local_port,
            client_addr,
            vfs,
            mount_signal,
            export_name,
            transaction_tracker,
            file_handle_converter,
        }))
    }

    pub(crate) fn local_port(&self) -> u16 {
        self.0.local_port
    }

    pub(crate) fn client_addr(&self) -> &str {
        &self.0.client_addr
    }

    pub(crate) fn vfs(&self) -> &T {
        &self.0.vfs
    }

    pub(crate) fn mount_signal(&self) -> Option<&mpsc::Sender<bool>> {
        self.0.mount_signal.as_ref()
    }

    pub(crate) fn export_name(&self) -> &str {
        &self.0.export_name
    }

    pub(crate) fn transaction_tracker(&self) -> &TransactionTracker {
        &self.0.transaction_tracker
    }

    pub(crate) fn file_handle_converter(&self) -> &FileHandleConverter {
        &self.0.file_handle_converter
    }
}

impl<T> fmt::Debug for RPCContext<T>
where
    T: NfsFileSystem,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("RPCContext")
            .field("local_port", &self.0.local_port)
            .field("client_addr", &self.0.client_addr)
            .field("mount_signal", &self.0.mount_signal)
            .field("export_name", &self.0.export_name)
            .field("transaction_tracker", &self.0.transaction_tracker)
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
        Self::new(
            2049,
            "localhost".to_owned(),
            vfs,
            None,
            Arc::new(export_name.to_owned()),
            Arc::new(TransactionTracker::new(
                std::time::Duration::from_mins(1),
                256,
                1024,
            )),
            FileHandleConverter::new(),
        )
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
