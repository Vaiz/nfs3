use std::sync::Arc;

use nfs3_client::nfs3_types::nfs3::nfs_fh3;
use nfs3_server::test_reexports::RPCContext;
use nfs3_server::vfs::NfsFileSystem;

pub struct Server<IO, FS: NfsFileSystem> {
    context: RPCContext<FS>,
    io: IO,
}

impl<IO, FS> Server<IO, FS>
where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + 'static,
    FS: NfsFileSystem + 'static,
{
    pub fn new(io: IO, memfs: FS) -> anyhow::Result<Self> {
        let context = RPCContext::test_ctx("/mnt", Arc::new(memfs));
        let this = Self { context, io };
        Ok(this)
    }

    pub fn root_dir(&self) -> nfs_fh3 {
        self.context.root_dir()
    }

    pub async fn run(self) -> Result<(), anyhow::Error> {
        nfs3_server::test_reexports::process_socket(self.io, self.context).await
    }
}
