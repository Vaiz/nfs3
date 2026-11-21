use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::transaction_tracker::TransactionTracker;
use crate::vfs::handle::FileHandleConverter;

/// Context shared by calls on a connection. Cloning increments one reference
/// count and does not allocate.
pub struct RPCContext<T: crate::vfs::NfsFileSystem>(Arc<ConnectionContext<T>>);

/// Connection state borrowed through an [`RPCContext`].
#[doc(hidden)]
pub struct ConnectionContext<T: crate::vfs::NfsFileSystem> {
    pub local_port: u16,
    pub client_addr: String,
    pub vfs: Arc<T>,
    pub mount_signal: Option<mpsc::Sender<bool>>,
    pub export_name: Arc<String>,
    pub transaction_tracker: Arc<TransactionTracker>,
    pub(crate) file_handle_converter: FileHandleConverter,
}

impl<T: crate::vfs::NfsFileSystem> RPCContext<T> {
    pub(crate) fn new(context: ConnectionContext<T>) -> Self {
        Self(Arc::new(context))
    }
}

impl<T: crate::vfs::NfsFileSystem> Deref for RPCContext<T> {
    type Target = ConnectionContext<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> fmt::Debug for RPCContext<T>
where
    T: crate::vfs::NfsFileSystem,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("RPCContext")
            .field("local_port", &self.local_port)
            .field("client_addr", &self.client_addr)
            .field("mount_signal", &self.mount_signal)
            .field("export_name", &self.export_name)
            .field("transaction_tracker", &self.transaction_tracker)
            .finish_non_exhaustive()
    }
}

impl<T> Clone for RPCContext<T>
where
    T: crate::vfs::NfsFileSystem,
{
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

#[doc(hidden)]
#[cfg(feature = "__test_reexports")]
impl<T> RPCContext<T>
where
    T: crate::vfs::NfsFileSystem + 'static,
{
    pub fn test_ctx(export_name: &str, vfs: Arc<T>) -> Self {
        Self::new(ConnectionContext {
            local_port: 2049,
            client_addr: "localhost".to_owned(),
            vfs,
            mount_signal: None,
            export_name: Arc::new(export_name.to_owned()),
            transaction_tracker: Arc::new(TransactionTracker::new(
                std::time::Duration::from_mins(1),
                256,
                1024,
            )),
            file_handle_converter: FileHandleConverter::new(),
        })
    }
    #[must_use]
    pub fn root_dir(&self) -> nfs3_types::nfs3::nfs_fh3 {
        self.file_handle_converter.fh_to_nfs(&self.vfs.root_dir())
    }
}
