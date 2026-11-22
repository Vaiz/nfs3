use std::sync::Arc;

use derive_where::derive_where;
use nfs3_types::rpc::{auth_flavor, auth_unix, call_body};
use nfs3_types::xdr_codec::{Decoder, Error};
use tokio::sync::{OnceCell, mpsc};

use crate::rpcwire::unpack_exact;
use crate::transaction_tracker::TransactionTracker;
use crate::vfs::NfsFileSystem;
use crate::vfs::handle::FileHandleConverter;

#[derive_where(Clone, Debug)]
pub struct RPCContext<T: NfsFileSystem> {
    pub local_port: u16,
    pub client_addr: String,
    // TODO: should it be removed from this struct?
    pub auth: auth_unix,
    #[derive_where(skip(Debug))]
    pub vfs: Arc<T>,
    pub mount_signal: Option<mpsc::Sender<bool>>,
    pub export_name: Arc<str>,
    pub transaction_tracker: Arc<TransactionTracker>,
    pub(crate) read_limit: Arc<OnceCell<u32>>,
    #[derive_where(skip(Debug))]
    pub(crate) file_handle_converter: FileHandleConverter,
}

impl<T: NfsFileSystem> RPCContext<T> {
    /// Decodes the call's `AUTH_UNIX` credential into `auth`.
    ///
    /// Returns whether the call uses `AUTH_UNIX`.
    pub(crate) fn set_auth(&mut self, call: &call_body) -> Result<bool, Error> {
        if call.cred.flavor != auth_flavor::AUTH_UNIX {
            return Ok(false);
        }
        self.auth = unpack_exact(Decoder::copying(call.cred.body.clone().into_bytes()))?;
        Ok(true)
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
            read_limit: Arc::default(),
            file_handle_converter: FileHandleConverter::new(),
        }
    }
    #[must_use]
    pub fn root_dir(&self) -> nfs3_types::nfs3::nfs_fh3 {
        self.file_handle_converter.fh_to_nfs(&self.vfs.root_dir())
    }
}
