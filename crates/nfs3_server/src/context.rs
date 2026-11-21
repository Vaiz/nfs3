use std::fmt;
use std::sync::Arc;

use nfs3_types::rpc::auth_unix;
use tokio::sync::mpsc;

use crate::transaction_tracker::TransactionTracker;
use crate::vfs::handle::FileHandleConverter;

/// Context for an RPC call. Cloning shares the address and credentials without
/// copying their contents or allocating.
pub struct RPCContext<T: crate::vfs::NfsFileSystem> {
    pub local_port: u16,
    pub client_addr: Arc<String>,
    pub auth: Arc<auth_unix>,
    pub vfs: Arc<T>,
    pub mount_signal: Option<mpsc::Sender<bool>>,
    pub export_name: Arc<String>,
    pub transaction_tracker: Arc<TransactionTracker>,
    pub(crate) file_handle_converter: FileHandleConverter,
}

impl<T> fmt::Debug for RPCContext<T>
where
    T: crate::vfs::NfsFileSystem,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("RPCContext")
            .field("local_port", &self.local_port)
            .field("client_addr", &self.client_addr)
            .field("auth", &self.auth)
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
        Self {
            local_port: self.local_port,
            client_addr: Arc::clone(&self.client_addr),
            auth: Arc::clone(&self.auth),
            vfs: Arc::clone(&self.vfs),
            mount_signal: self.mount_signal.clone(),
            export_name: Arc::clone(&self.export_name),
            transaction_tracker: Arc::clone(&self.transaction_tracker),
            file_handle_converter: self.file_handle_converter,
        }
    }
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
            client_addr: Arc::new("localhost".to_owned()),
            auth: Arc::default(),
            vfs,
            mount_signal: None,
            export_name: Arc::new(export_name.to_owned()),
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

#[cfg(all(test, feature = "__test_reexports", feature = "memfs"))]
mod tests {
    use super::*;
    use crate::memfs::{MemFs, MemFsConfig};

    #[test]
    fn clones_share_address_and_keep_credentials_per_request() {
        let fs = Arc::new(MemFs::new(MemFsConfig::default()).unwrap());
        let connection = RPCContext::test_ctx("/mnt", fs);
        let mut request = connection.clone();
        request.auth = Arc::new(auth_unix {
            uid: 1000,
            gids: vec![1000, 1001],
            ..auth_unix::default()
        });

        let request_clone = request.clone();
        let other_request = connection.clone();
        assert!(Arc::ptr_eq(
            &connection.client_addr,
            &request_clone.client_addr
        ));
        assert!(Arc::ptr_eq(&request.auth, &request_clone.auth));
        assert_eq!(request_clone.auth.gids, [1000, 1001]);
        assert!(Arc::ptr_eq(&connection.auth, &other_request.auth));
        assert_eq!(other_request.auth.gids, []);
    }
}
