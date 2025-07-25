pub mod rpc_tests;
mod server;
pub mod wasm_fs;

use std::ops::{Deref, DerefMut};

use nfs3_client::tokio::TokioIo;
use nfs3_server::memfs::{MemFs, MemFsConfig};
use nfs3_server::vfs::adapters::ReadOnlyAdapter;
use nfs3_types::nfs3::{Nfs3Result, nfs_fh3, nfsstat3};
use nfs3_types::xdr_codec::Opaque;
pub use rpc_tests::RpcTestContext;
pub use server::Server;
use tokio::io::{DuplexStream, duplex};

pub struct TestContext<IO> {
    server_handle: tokio::task::JoinHandle<anyhow::Result<()>>,
    client: nfs3_client::Nfs3Client<IO>,
    root_dir: nfs_fh3,
}

impl TestContext<TokioIo<DuplexStream>> {
    pub fn setup() -> Self {
        Self::setup_with_config(Self::config(), false, tracing::Level::DEBUG)
    }

    pub fn setup_ro() -> Self {
        Self::setup_with_config(Self::config(), true, tracing::Level::DEBUG)
    }

    fn config() -> MemFsConfig {
        let mut config = MemFsConfig::default();

        config.add_file("/a.txt", "hello world\n".as_bytes());
        config.add_file("/b.txt", "Greetings to xet data\n".as_bytes());
        config.add_dir("/another_dir");
        config.add_file("/another_dir/thisworks.txt", "i hope\n".as_bytes());

        config
    }

    pub fn setup_with_config(
        fs_config: MemFsConfig,
        readonly: bool,
        log_level: tracing::Level,
    ) -> Self {
        init_logging(log_level);

        let (server, client) = duplex(1024 * 1024);
        let memfs = MemFs::new(fs_config).unwrap();
        let (root_dir, server_handle) = if readonly {
            let memfs = ReadOnlyAdapter::new(memfs);
            let server = Server::new(server, memfs).unwrap();
            let root_dir = server.root_dir();
            let server_handle = tokio::task::spawn(server.run());
            (root_dir, server_handle)
        } else {
            let server = Server::new(server, memfs).unwrap();
            let root_dir = server.root_dir();
            let server_handle = tokio::task::spawn(server.run());
            (root_dir, server_handle)
        };

        let client = nfs3_client::tokio::TokioIo::new(client);
        let client = nfs3_client::Nfs3Client::new(client);

        Self {
            server_handle,
            client,
            root_dir,
        }
    }
}

impl<IO> TestContext<IO>
where
    IO: nfs3_client::io::AsyncRead + nfs3_client::io::AsyncWrite + Send,
{
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

    pub async fn just_lookup(&mut self, dir: nfs_fh3, filename: &str) -> Result<nfs_fh3, nfsstat3> {
        use nfs3_types::nfs3::{LOOKUP3args, diropargs3};

        let result = self
            .client
            .lookup(&LOOKUP3args {
                what: diropargs3 {
                    dir,
                    name: filename.as_bytes().into(),
                },
            })
            .await
            .expect("failed to lookup a file");

        match result {
            Nfs3Result::Ok(ok) => Ok(ok.object),
            Nfs3Result::Err((status, _)) => Err(status),
        }
    }

    pub async fn just_create(
        &mut self,
        dir: nfs_fh3,
        filename: &str,
        content: &[u8],
    ) -> Result<nfs_fh3, nfsstat3> {
        use nfs3_types::nfs3::{
            CREATE3args, Nfs3Result, WRITE3args, createhow3, sattr3, stable_how,
        };

        // Create the file
        let create_result = self
            .client
            .create(&CREATE3args {
                where_: nfs3_types::nfs3::diropargs3 {
                    dir,
                    name: filename.as_bytes().into(),
                },
                how: createhow3::UNCHECKED(sattr3::default()),
            })
            .await
            .expect("failed to create file");

        let file_handle = match create_result {
            Nfs3Result::Ok(ok) => ok.obj.unwrap(),
            Nfs3Result::Err((status, _)) => return Err(status),
        };

        // Write content if not empty
        if !content.is_empty() {
            let write_result = self
                .client
                .write(&WRITE3args {
                    file: file_handle.clone(),
                    offset: 0,
                    count: content.len() as u32,
                    stable: stable_how::UNSTABLE,
                    data: Opaque::owned(content.to_vec()),
                })
                .await
                .expect("failed to write to file");

            match write_result {
                Nfs3Result::Ok(_) => {}
                Nfs3Result::Err((status, _)) => return Err(status),
            }
        }

        Ok(file_handle)
    }

    pub async fn just_mkdir(&mut self, dir: nfs_fh3, dirname: &str) -> Result<nfs_fh3, nfsstat3> {
        use nfs3_types::nfs3::{MKDIR3args, Nfs3Result, sattr3};

        let result = self
            .client
            .mkdir(&MKDIR3args {
                where_: nfs3_types::nfs3::diropargs3 {
                    dir,
                    name: dirname.as_bytes().into(),
                },
                attributes: sattr3::default(),
            })
            .await
            .expect("failed to mkdir");

        match result {
            Nfs3Result::Ok(ok) => Ok(ok.obj.unwrap()),
            Nfs3Result::Err((status, _)) => Err(status),
        }
    }

    pub async fn just_read(&mut self, file: nfs_fh3) -> Result<Vec<u8>, nfsstat3> {
        use nfs3_types::nfs3::{Nfs3Result, READ3args};

        let mut offset = 0u64;
        let mut result = Vec::new();

        loop {
            let read_result = self
                .client
                .read(&READ3args {
                    file: file.clone(),
                    offset,
                    count: 1024 * 1024,
                })
                .await
                .expect("failed to read file");

            match read_result {
                Nfs3Result::Ok(ok) => {
                    result.extend_from_slice(&ok.data.0);
                    if ok.eof || ok.count == 0 {
                        break;
                    }
                    offset += ok.count as u64;
                }
                Nfs3Result::Err((status, _)) => return Err(status),
            }
        }

        Ok(result)
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
            print!("{byte:02x} ");
        }
        println!();
    }
}
