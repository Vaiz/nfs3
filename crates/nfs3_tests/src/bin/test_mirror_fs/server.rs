use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tempfile::TempDir;
use tokio::time::sleep;

use super::context::{ServerMode, TestContext};

/// Kill any existing server processes on the test port
fn kill_existing_servers(port: u16) -> Result<()> {
    #[cfg(windows)]
    {
        // On Windows, use netstat to find process using the port
        let output = Command::new("cmd")
            .args(["/C", &format!("netstat -ano | findstr :{}", port)])
            .output()
            .ok();

        if let Some(output) = output {
            let output_str = String::from_utf8_lossy(&output.stdout);
            for line in output_str.lines() {
                // Extract PID from the last column
                if let Some(pid_str) = line.split_whitespace().last() {
                    if let Ok(pid) = pid_str.parse::<u32>() {
                        // Kill the process
                        let _ = Command::new("taskkill")
                            .args(["/F", "/PID", &pid.to_string()])
                            .output();
                    }
                }
            }
        }
    }

    #[cfg(unix)]
    {
        // On Unix, use lsof or ss to find process using the port
        let _ = Command::new("sh")
            .arg("-c")
            .arg(format!("lsof -ti :{} | xargs -r kill -9", port))
            .output();
    }

    // Give OS time to release the port
    std::thread::sleep(Duration::from_millis(500));
    Ok(())
}

/// Configuration for the test suite
pub struct TestConfig {
    pub temp_dir: TempDir,
    pub server_process: Child,
    pub bind_port: u16,
    #[allow(dead_code)]
    pub mode: ServerMode,
}

impl Drop for TestConfig {
    fn drop(&mut self) {
        let _ = self.server_process.kill();
        let _ = self.server_process.wait();
        // Clean up port after server shutdown
        let _ = kill_existing_servers(self.bind_port);
    }
}

/// Setup the NFS server with a temporary directory
pub async fn setup_server(mode: ServerMode) -> Result<TestConfig> {
    let temp_dir = TempDir::new().context("Failed to create temp directory")?;
    let temp_path = temp_dir.path().to_string_lossy().to_string();
    let bind_port = 11111;

    let cargo_nfs3_server = if cfg!(windows) {
        "target/debug/cargo-nfs3-server.exe"
    } else {
        "target/debug/cargo-nfs3-server"
    };

    if !Path::new(cargo_nfs3_server).exists() {
        let status = Command::new("cargo")
            .args(["build", "--package", "cargo-nfs3-server"])
            .status()
            .context("Failed to build cargo-nfs3-server")?;

        if !status.success() {
            bail!("Failed to build cargo-nfs3-server");
        }
    }

    let mut cmd = Command::new(cargo_nfs3_server);
    cmd.arg("--path")
        .arg(&temp_path)
        .arg("--bind-ip")
        .arg("127.0.0.1")
        .arg("--bind-port")
        .arg(bind_port.to_string())
        .arg("--log-level")
        .arg("warn")
        .arg("--quiet")
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    if mode == ServerMode::ReadOnly {
        cmd.arg("--readonly");
    }

    // Kill any existing server on this port before starting
    let _ = kill_existing_servers(bind_port);

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
    port: u16,
) -> Result<nfs3_client::Nfs3Connection<nfs3_client::tokio::TokioIo<tokio::net::TcpStream>>> {
    for attempt in 1..=10 {
        sleep(Duration::from_millis(100 * attempt)).await;

        match nfs3_client::Nfs3ConnectionBuilder::new(
            nfs3_client::tokio::TokioConnector,
            "127.0.0.1",
            "/",
        )
        .nfs3_port(port)
        .mount_port(port)
        .connect_from_privileged_port(false)
        .mount()
        .await
        {
            Ok(client) => {
                return Ok(client);
            }
            Err(e) if attempt < 10 => {
                tracing::debug!("Connection attempt {} failed: {}", attempt, e);
            }
            Err(e) => {
                bail!(
                    "Failed to connect NFS client after {} attempts: {}",
                    attempt,
                    e
                );
            }
        }
    }

    unreachable!()
}

/// Initialize a test context with server and client
pub async fn init_context(mode: ServerMode) -> Result<TestContext> {
    let config = setup_server(mode).await?;
    sleep(Duration::from_millis(500)).await;
    let client = connect_client(config.bind_port).await?;

    let ctx = TestContext::new(client, mode, config);

    Ok(ctx)
}
