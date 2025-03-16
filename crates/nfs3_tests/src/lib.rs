mod server;
pub mod wasm_fs;

use std::ops::{Deref, DerefMut};

use nfs3_client::tokio::TokioIo;
use nfs3_types::nfs3::nfs_fh3;
pub use server::{FsConfig, Server};
use tokio::io::{DuplexStream, duplex};

pub struct TestContext<IO> {
    server_handle: tokio::task::JoinHandle<anyhow::Result<()>>,
    client: nfs3_client::Nfs3Client<IO>,
    root_dir: nfs_fh3,
}

impl TestContext<TokioIo<DuplexStream>> {
    pub async fn setup() -> Self {
        let mut config = server::FsConfig::default();

        config.add_file("/a.txt", "hello world\n".as_bytes());
        config.add_file("/b.txt", "Greetings to xet data\n".as_bytes());
        config.add_dir("/another_dir");
        config.add_file("/another_dir/thisworks.txt", "i hope\n".as_bytes());

        Self::setup_with_config(config, tracing::Level::DEBUG).await
    }

    pub async fn setup_with_config(fs_config: server::FsConfig, log_level: tracing::Level) -> Self {
        init_logging(log_level);

        let (server, client) = duplex(1024 * 1024);
        let server = Server::new(server, fs_config).await.unwrap();
        let root_dir = server.root_dir();
        let server_handle = tokio::task::spawn(server.run());
        let client = nfs3_client::tokio::TokioIo::new(client);
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

impl<IO> Deref for TestContext<IO> {
    type Target = nfs3_client::Nfs3Client<IO>;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

impl<IO> DerefMut for TestContext<IO> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.client
    }
}

static LOGGING: std::sync::Once = std::sync::Once::new();

pub fn init_logging(level: tracing::Level) {
    LOGGING.call_once(|| {
        tracing_subscriber::fmt()
            .with_max_level(level)
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
