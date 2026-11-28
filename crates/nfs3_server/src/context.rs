use std::sync::Arc;

use derive_where::derive_where;
use nfs3_types::mount::{MNTPATHLEN, dirpath};
use nfs3_types::xdr_codec::BoundedOpaque;
use tokio::sync::mpsc;

use crate::transaction_tracker::TransactionTracker;
use crate::vfs::NfsFileSystem;
use crate::vfs::handle::FileHandleConverter;

#[derive_where(Clone, Debug)]
pub struct RPCContext<T: NfsFileSystem> {
    pub local_port: u16,
    pub client_addr: String,
    #[derive_where(skip(Debug))]
    pub vfs: Arc<T>,
    pub mount_signal: Option<mpsc::Sender<bool>>,
    pub(crate) export_path: dirpath<'static>,
    pub(crate) hide_exports: bool,
    pub(crate) exact_export_match: bool,
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
            vfs,
            mount_signal: None,
            export_path: normalized_export_path(export_name)?,
            hide_exports: false,
            exact_export_match: false,
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
        self.vfs
            .encode_handle(&self.file_handle_converter, &self.vfs.root_dir())
    }
}
