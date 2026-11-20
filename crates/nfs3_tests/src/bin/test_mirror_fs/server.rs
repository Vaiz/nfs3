use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tempfile::TempDir;
use tokio::time::sleep;

use super::context::{ServerMode, TestContext};

/// Configuration for the test suite
pub struct TestConfig {
    pub temp_dir: TempDir,
    pub server_process: Child,
    pub bind_port: u16,
    #[allow(
        dead_code,
        reason = "Retain the server mode as part of the test configuration"
    )]
    pub mode: ServerMode,
}

impl Drop for TestConfig {
    fn drop(&mut self) {
        match self.server_process.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => {}
            Err(error) => tracing::warn!("Failed to check mirror server status: {error}"),
        }
        if let Err(error) = self.server_process.kill() {
            tracing::warn!("Failed to stop mirror server: {error}");
        }
        if let Err(error) = self.server_process.wait() {
            tracing::warn!("Failed to reap mirror server: {error}");
        }
    }
}

impl TestConfig {
    fn ensure_running(&mut self) -> Result<()> {
        if let Some(status) = self
            .server_process
            .try_wait()
            .context("Failed to check mirror server status")?
        {
            bail!(
                "Mirror server exited with {status} on port {}; check server diagnostics above",
                self.bind_port
            );
        }
        Ok(())
    }
}

/// Setup the NFS server with a temporary directory
pub fn setup_server(mode: ServerMode) -> Result<TestConfig> {
    let binary = std::env::var_os("NFS3_MIRROR_SERVER")
        .context("Set NFS3_MIRROR_SERVER to an explicitly prebuilt cargo-nfs3-server binary")?;
    let binary = Path::new(&binary);
    let binary = binary
        .canonicalize()
        .with_context(|| format!("Prebuilt mirror server not found: {}", binary.display()))?;
    let reservation = TcpListener::bind(("127.0.0.1", 0))
        .context("Failed to select an available mirror test port")?;
    let bind_port = reservation.local_addr()?.port();
    let temp_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/mirror-tests");
    std::fs::create_dir_all(&temp_root).context("Failed to create mirror test directory")?;
    let temp_dir = TempDir::new_in(&temp_root).context("Failed to create temp directory")?;

    let mut cmd = Command::new(binary);
    cmd.arg("--path")
        .arg(temp_dir.path())
        .arg("--bind-ip")
        .arg("127.0.0.1")
        .arg("--bind-port")
        .arg(bind_port.to_string())
        .arg("--log-level")
        .arg("warn")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    if mode == ServerMode::ReadOnly {
        cmd.arg("--readonly");
    }

    // The CLI cannot inherit a listener; a bind race is reported via child exit.
    drop(reservation);
    let server_process = cmd.spawn().context("Failed to start cargo-nfs3-server")?;

    Ok(TestConfig {
        temp_dir,
        server_process,
        bind_port,
        mode,
    })
}

/// Connect NFS client to the server
pub async fn connect_client(
    config: &mut TestConfig,
) -> Result<nfs3_client::Nfs3Connection<nfs3_client::tokio::TokioIo<tokio::net::TcpStream>>> {
    for attempt in 1..=10 {
        sleep(Duration::from_millis(100 * attempt)).await;
        config.ensure_running()?;

        let connection = nfs3_client::Nfs3ConnectionBuilder::new(
            nfs3_client::tokio::TokioConnector,
            "127.0.0.1",
            "/",
        )
        .nfs3_port(config.bind_port)
        .mount_port(config.bind_port)
        .connect_from_privileged_port(false)
        .mount();
        let result = tokio::time::timeout(Duration::from_secs(5), connection)
            .await
            .context("Timed out connecting to mirror server")
            .and_then(|result| result.map_err(anyhow::Error::from));
        match result {
            Ok(client) => {
                config.ensure_running()?;
                return Ok(client);
            }
            Err(e) if attempt < 10 => {
                tracing::debug!("Connection attempt {} failed: {}", attempt, e);
            }
            Err(e) => {
                bail!("Failed to connect NFS client after {attempt} attempts: {e}");
            }
        }
    }

    unreachable!()
}

/// Initialize a test context with server and client
pub async fn init_context(mode: ServerMode) -> Result<TestContext> {
    let mut config = setup_server(mode)?;
    let client = connect_client(&mut config).await?;

    let ctx = TestContext::new(client, mode, config);

    Ok(ctx)
}
