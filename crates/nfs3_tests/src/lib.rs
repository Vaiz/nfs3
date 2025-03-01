mod io;
mod server;

use std::ops::{Deref, DerefMut};

pub use io::MockChannel;
use nfs3_types::nfs3::nfs_fh3;
pub use server::Server;

pub struct TestContext {
    server_handle: tokio::task::JoinHandle<anyhow::Result<()>>,
    client: nfs3_client::Nfs3Client<MockChannel>,
    root_dir: nfs_fh3,
}

impl TestContext {
    pub async fn setup() -> Self {
        init_logging();

        let (server, client) = MockChannel::pair();
        let server = Server::new(server);
        let root_dir = server.root_dir();
        let server_handle = tokio::task::spawn(server.run());
        let client = nfs3_client::Nfs3Client::new(client);

        Self {
            server_handle,
            client,
            root_dir,
        }
    }

    pub fn root_dir(&self) -> &nfs_fh3 {
        &self.root_dir
    }

    pub async fn shutdown(self) -> anyhow::Result<()> {
        let Self {
            server_handle,
            client,
            root_dir: _,
        } = self;

        drop(client);

        server_handle.await?
    }
}

impl Deref for TestContext {
    type Target = nfs3_client::Nfs3Client<MockChannel>;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

impl DerefMut for TestContext {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.client
    }
}

pub fn init_logging() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(std::io::stderr)
        .init();
}

#[cfg(test)]
mod tests {
    use nfs3_types::nfs3::{diropargs3, LOOKUP3args};

    use super::*;

    #[tokio::test]
    async fn lookup_root() -> Result<(), anyhow::Error> {
        let mut client = TestContext::setup().await;
        let root = client.root_dir().clone();

        client.null().await?;
        let lookup = client
            .lookup(LOOKUP3args {
                what: diropargs3 {
                    dir: root.clone(),
                    name: b".".as_slice().into(),
                },
            })
            .await?
            .unwrap();

        tracing::info!("{lookup:?}");
        assert_eq!(lookup.object, root);

        client.shutdown().await
    }
}
