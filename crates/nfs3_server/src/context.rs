use std::sync::Arc;

use derive_where::derive_where;
use nfs3_types::rpc::{auth_flavor, auth_unix, call_body};
use nfs3_types::xdr_codec::{Decoder, Error};
use tokio::sync::mpsc;

use crate::rpcwire::unpack_exact;
use crate::transaction_tracker::TransactionTracker;
use crate::vfs::NfsFileSystem;
use crate::vfs::handle::FileHandleConverter;

const MAX_AUTH_BYTES: usize = 400;
const MAX_MACHINENAME_LEN: usize = 255;
const MAX_GIDS: usize = 16;

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
    #[derive_where(skip(Debug))]
    pub(crate) file_handle_converter: FileHandleConverter,
}

impl<T: NfsFileSystem> RPCContext<T> {
    /// Decodes the call's `AUTH_UNIX` credential into `auth`.
    ///
    /// Returns whether the call uses `AUTH_UNIX`, or an error if the credential
    /// is malformed or exceeds the RFC 5531 limits.
    pub(crate) fn set_auth(&mut self, call: &call_body) -> Result<bool, Error> {
        if call.cred.flavor != auth_flavor::AUTH_UNIX {
            return Ok(false);
        }
        let body = &call.cred.body;
        if body.len() > MAX_AUTH_BYTES {
            return Err(Error::InvalidLength(body.len()));
        }
        let auth: auth_unix = unpack_exact(Decoder::copying(body.clone().into_bytes()))?;
        if auth.machinename.len() > MAX_MACHINENAME_LEN {
            return Err(Error::InvalidLength(auth.machinename.len()));
        }
        if auth.gids.len() > MAX_GIDS {
            return Err(Error::InvalidLength(auth.gids.len()));
        }
        self.auth = auth;
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
            file_handle_converter: FileHandleConverter::new(),
        }
    }
    #[must_use]
    pub fn root_dir(&self) -> nfs3_types::nfs3::nfs_fh3 {
        self.file_handle_converter.fh_to_nfs(&self.vfs.root_dir())
    }
}
