use std::sync::Arc;

use derive_where::derive_where;
use nfs3_types::rpc::auth_unix;
use tokio::sync::mpsc;

use crate::transaction_tracker::TransactionTracker;
use crate::vfs::handle::FileHandleConverter;

#[derive_where(Clone, Debug)]
pub struct RPCContext<T: crate::vfs::NfsFileSystem> {
    pub local_port: u16,
    pub client_addr: String,
    pub auth: auth_unix,
    #[derive_where(skip(Debug))]
    pub vfs: Arc<T>,
    pub mount_signal: Option<mpsc::Sender<bool>>,
    pub export_name: Arc<str>,
    pub transaction_tracker: Arc<TransactionTracker>,
    #[derive_where(skip(Debug))]
    pub(crate) file_handle_converter: FileHandleConverter,
}

#[doc(hidden)]
#[cfg(feature = "__test_reexports")]
impl<T> RPCContext<T>
where
    T: crate::vfs::NfsFileSystem + 'static,
{
    pub fn test_ctx(export_name: &str, vfs: Arc<T>) -> Self {
        Self {
            local_port: 2049,
            client_addr: "localhost".to_owned(),
            auth: auth_unix::default(),
            vfs,
            mount_signal: None,
            export_name: Arc::from(export_name),
            transaction_tracker: Arc::new(TransactionTracker::new(
                std::time::Duration::from_mins(1),
                256,
                1024,
            )),
            file_handle_converter: FileHandleConverter::new(),
        }
    }
    #[must_use]
    pub fn root_dir(&self) -> nfs3_types::nfs3::nfs_fh3 {
        self.file_handle_converter.fh_to_nfs(&self.vfs.root_dir())
    }
}
