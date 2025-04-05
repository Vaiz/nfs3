use std::fmt;
use std::sync::Arc;

use nfs3_types::rpc::auth_unix;
use tokio::sync::mpsc;

use crate::transaction_tracker::TransactionTracker;
use crate::vfs::NFSFileSystem;

#[derive(Clone)]
pub struct RPCContext {
    pub local_port: u16,
    pub client_addr: String,
    pub auth: auth_unix,
    pub vfs: Arc<dyn NFSFileSystem + Send + Sync>,
    pub mount_signal: Option<mpsc::Sender<bool>>,
    pub export_name: Arc<String>,
    pub transaction_tracker: Arc<TransactionTracker>,
}

#[allow(clippy::missing_fields_in_debug)]
impl fmt::Debug for RPCContext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("RPCContext")
            .field("local_port", &self.local_port)
            .field("client_addr", &self.client_addr)
            .field("auth", &self.auth)
            .field("export_name", &self.export_name)
            .finish()
    }
}
