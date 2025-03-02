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

static LOGGING: std::sync::Once = std::sync::Once::new();

pub fn init_logging() {
    LOGGING.call_once(|| {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_writer(std::io::stderr)
            .init();
    });
}

pub fn print_hex(data: &[u8]) {
    println!("Offset | 00 01 02 03 04 05 06 07 08 09 0A 0B 0C 0D 0E 0F");
    println!("-------|------------------------------------------------");
    for (i, chunk) in data.chunks(16).enumerate() {
        print!("{:06x} | ", i * 16);
        for byte in chunk {
            print!("{:02x} ", byte);
        }
        println!();
    }
}
