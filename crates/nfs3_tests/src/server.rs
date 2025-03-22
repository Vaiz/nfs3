use std::sync::Arc;
use std::time::Duration;

use nfs3_server::test_reexports::{RPCContext, TransactionTracker};
use nfs3_types::nfs3::nfs_fh3;

pub struct Server<IO> {
    context: RPCContext,
    io: IO,
}

impl<IO> Server<IO>
where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + 'static,
{
    pub fn new(io: IO, memfs: nfs3_server::memfs::MemFs) -> anyhow::Result<Self> {
        let context = RPCContext {
            local_port: 2049,
            client_addr: "localhost".to_string(),
            auth: nfs3_types::rpc::auth_unix::default(),
            vfs: Arc::new(memfs),
            mount_signal: None,
            export_name: Arc::new("/mnt".to_string()),
            transaction_tracker: Arc::new(TransactionTracker::new(Duration::from_secs(60))),
        };

        let this = Self { context, io };
        Ok(this)
    }

    pub fn root_dir(&self) -> nfs_fh3 {
        self.context.vfs.id_to_fh(self.context.vfs.root_dir())
    }

    pub async fn run(self) -> Result<(), anyhow::Error> {
        nfs3_server::test_reexports::process_socket(self.io, self.context).await
    }
}
