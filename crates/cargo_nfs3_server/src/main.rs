use std::path::Path;

use clap::Parser;
use nfs3_server::memfs::MemFs;
use nfs3_server::tcp::NFSTcp;
use nfs3_server::vfs::NfsFileSystem;
use nfs3_server::vfs::adapter::ReadOnlyAdapter;
use tracing_appender::non_blocking::WorkerGuard;

mod logging;
mod memfs;
mod mirror;

/// CLI tool for the `nfs3_server`
#[derive(Parser, Debug)]
#[command(name = "nfs3_server", version, about = "A simple NFSv3 server", long_about = None)]
struct Args {
    /// Path to the directory to serve for `MirrorFs`
    #[arg(long)]
    path: Option<String>,

    /// Export path
    #[arg(long, default_value = "/")]
    export_name: String,

    /// IP address to bind the server to
    #[arg(short = 'i', long, default_value = "0.0.0.0")]
    bind_ip: String,

    /// Port to bind the server to
    #[arg(short = 'p', long, default_value_t = 11111)]
    bind_port: u16,

    /// Serve the directory as read-only
    #[arg(short, long)]
    readonly: bool,

    /// Use an in-memory filesystem
    #[arg(long)]
    memfs: bool,

    /// Log level (default is "info")
    #[arg(long, default_value = "info")]
    log_level: String,

    /// Log file path
    #[arg(long)]
    log_file: Option<String>,

    /// Disable console logging
    #[arg(long)]
    quiet: bool,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let guards = logging::init_logging(&args.log_level, args.log_file.as_deref(), !args.quiet);

    let bind_addr = format!("{}:{}", args.bind_ip, args.bind_port);
    let export_path = args.export_name;

    if args.memfs {
        let memfs = MemFs::new(memfs::default_config(args.readonly))
            .expect("failed to create memfs instance");
        if args.readonly {
            start_server(bind_addr, export_path, ReadOnlyAdapter::new(memfs), guards).await;
        } else {
            start_server(bind_addr, export_path, memfs, guards).await;
        }
    } else {
        let path = args
            .path
            .as_deref()
            .unwrap_or_else(|| panic!("--path is required when not using --memfs"));

        assert!(Path::new(path).exists(), "path [{path}] does not exist",);
        let mirror_fs = mirror::MirrorFs::new(path);
        if args.readonly {
            start_server(
                bind_addr,
                export_path,
                ReadOnlyAdapter::new(mirror_fs),
                guards,
            )
            .await;
        } else {
            start_server(bind_addr, export_path, mirror_fs, guards).await;
        }
    }
}

#[expect(
    clippy::collection_is_never_read,
    reason = "it's not expected to be read"
)]
async fn start_server(
    bind_addr: String,
    export_name: String,
    fs: impl NfsFileSystem + 'static,
    mut guards: Vec<WorkerGuard>,
) {
    use nfs3_server::tcp::NFSTcpListener;

    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut tx = Some(tx);
    ctrlc::set_handler(move || {
        if let Some(tx) = tx.take() {
            tracing::info!("Received Ctrl-C, shutting down...");
            guards.clear();
            let _ = tx.send(());
        }
    })
    .expect("Error setting Ctrl-C handler");

    let mut listener = NFSTcpListener::bind(&bind_addr, fs)
        .await
        .expect("failed to bind server");
    listener.with_export_name(export_name);
    let handle_future = listener.handle_forever();

    tokio::select! {
        result = handle_future => {
            tracing::info!("Server stopped");
            if let Err(e) = result {
                tracing::error!("Error: {e}");
            }
        }
        _ = rx => { }
    }
}
