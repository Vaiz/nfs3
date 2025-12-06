use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use nfs3_client::nfs3_types::nfs3::{LOOKUP3args, diropargs3, filename3, nfs_fh3};
use nfs3_client::nfs3_types::xdr_codec::Opaque;
use nfs3_tests::JustClient;

use crate::server::TestConfig;

static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);
type IO = nfs3_client::tokio::TokioIo<tokio::net::TcpStream>;

/// Server mode for testing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerMode {
    ReadOnly,
    ReadWrite,
}

/// Test context for individual tests
pub struct TestContext {
    pub client: nfs3_client::Nfs3Connection<IO>,
    /// Server configuration - kept alive to ensure temp directory and server process lifecycle
    config: TestConfig,
}

impl TestContext {
    pub fn new(
        client: nfs3_client::Nfs3Connection<IO>,
        _mode: ServerMode,
        config: TestConfig,
    ) -> Self {
        Self { client, config }
    }

    pub fn local_path(&self) -> &Path {
        self.config.temp_dir.path()
    }

    pub fn root_fh(&self) -> nfs_fh3 {
        self.client.root_nfs_fh3()
    }

    /// Create a unique subdirectory for a test to avoid interference and caching issues
    pub fn create_test_subdir(&self, test_name: &str) -> PathBuf {
        let counter = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir_name = format!("test_{test_name}_{counter}");
        let test_dir = self.local_path().join(dir_name);
        std::fs::create_dir(&test_dir).expect("failed to create test subdirectory");
        test_dir
    }

    /// Get the NFS filehandle for a subdirectory by name (relative to root)
    pub async fn get_subdir_fh(&mut self, subdir_path: &Path) -> nfs_fh3 {
        let dir_name = subdir_path
            .file_name()
            .expect("subdirectory must have a name")
            .to_str()
            .expect("subdirectory name must be valid UTF-8");

        let root_fh = self.root_fh();
        let lookup_resok = self
            .client
            .lookup(&LOOKUP3args {
                what: diropargs3 {
                    dir: root_fh,
                    name: filename3(Opaque::borrowed(dir_name.as_bytes())),
                },
            })
            .await
            .expect("lookup subdirectory failed")
            .unwrap();

        lookup_resok.object
    }
}

impl JustClient for TestContext {
    type IO = IO;

    fn client(&mut self) -> &mut nfs3_client::Nfs3Client<Self::IO> {
        &mut self.client
    }
}
