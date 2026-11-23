use std::sync::Arc;

use derive_where::derive_where;
use nfs3_types::mount::{MNTPATHLEN, dirpath};
use nfs3_types::rpc::{auth_flavor, auth_unix, call_body};
use nfs3_types::xdr_codec::{BoundedOpaque, Decoder, Error};
use tokio::sync::mpsc;

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
    pub(crate) export_path: dirpath<'static>,
    pub transaction_tracker: Arc<TransactionTracker>,
    pub(crate) read_limit: u32,
    #[derive_where(skip(Debug))]
    pub(crate) file_handle_converter: FileHandleConverter,
}

pub(crate) fn normalized_export_path(name: &str) -> std::io::Result<dirpath<'static>> {
    let name = name.trim_matches('/');
    let len = name.len() + 1;
    if len > MNTPATHLEN {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("export path is {len} bytes after normalization; maximum is {MNTPATHLEN}"),
        ));
    }
    let path = format!("/{name}");
    BoundedOpaque::try_from_bytes(path.into_bytes().into())
        .map(dirpath)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))
}

pub(crate) async fn root_read_limit(vfs: &impl NfsFileSystem) -> std::io::Result<u32> {
    vfs.fsinfo(&vfs.root_dir())
        .await
        .map(|info| info.rtmax)
        .map_err(|status| std::io::Error::other(format!("root FSINFO failed: {status}")))
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
    pub async fn test_ctx(export_name: &str, vfs: Arc<T>) -> std::io::Result<Self> {
        let read_limit = root_read_limit(&*vfs).await?;
        Ok(Self {
            local_port: 2049,
            client_addr: "localhost".to_owned(),
            auth: auth_unix::default(),
            vfs,
            mount_signal: None,
            export_path: normalized_export_path(export_name)?,
            transaction_tracker: Arc::new(TransactionTracker::new(
                std::time::Duration::from_mins(1),
                256,
                1024,
            )),
            read_limit,
            file_handle_converter: FileHandleConverter::new(),
        })
    }
    #[must_use]
    pub fn root_dir(&self) -> nfs3_types::nfs3::nfs_fh3 {
        self.file_handle_converter.fh_to_nfs(&self.vfs.root_dir())
    }
}
