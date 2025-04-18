use std::fmt;
use std::sync::Arc;

use nfs3_types::rpc::auth_unix;
use tokio::sync::mpsc;

use crate::transaction_tracker::TransactionTracker;

pub struct RPCContext<T> {
    pub local_port: u16,
    pub client_addr: String,
    pub auth: auth_unix,
    pub vfs: Arc<T>,
    pub mount_signal: Option<mpsc::Sender<bool>>,
    pub export_name: Arc<String>,
    pub transaction_tracker: Arc<TransactionTracker>,
}

#[allow(clippy::missing_fields_in_debug)]
impl<T> fmt::Debug for RPCContext<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("RPCContext")
            .field("local_port", &self.local_port)
            .field("client_addr", &self.client_addr)
            .field("auth", &self.auth)
            .field("mount_signal", &self.mount_signal)
            .field("export_name", &self.export_name)
            .field("transaction_tracker", &self.transaction_tracker)
            .finish()
    }
}

impl<T> Clone for RPCContext<T> {
    fn clone(&self) -> Self {
        Self {
            local_port: self.local_port,
            client_addr: self.client_addr.clone(),
            auth: self.auth.clone(),
            vfs: Arc::clone(&self.vfs),
            mount_signal: self.mount_signal.clone(),
            export_name: Arc::clone(&self.export_name),
            transaction_tracker: Arc::clone(&self.transaction_tracker),
        }
    }
}
