use std::fmt;
use std::sync::Arc;

use nfs3_types::rpc::auth_unix;

use crate::vfs::handle::FileHandleConverter;

pub struct RPCContext<T: crate::vfs::NfsFileSystem> {
    pub local_port: u16,
    pub client_addr: String,
    pub auth: auth_unix,
    pub vfs: Arc<T>,
    pub export_name: Arc<String>,
    pub(crate) file_handle_converter: FileHandleConverter,
    pub(crate) directory_limits: crate::tcp::DirectoryLimits,
    pub(crate) reply_metadata_limit: usize,
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
            .field("export_name", &self.export_name)
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
            client_addr: self.client_addr.clone(),
            auth: self.auth.clone(),
            vfs: Arc::clone(&self.vfs),
            export_name: Arc::clone(&self.export_name),
            file_handle_converter: self.file_handle_converter,
            directory_limits: self.directory_limits,
            reply_metadata_limit: self.reply_metadata_limit,
        }
    }
}
