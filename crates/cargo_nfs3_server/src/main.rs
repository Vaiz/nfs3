use std::sync::LazyLock;

use clap::Parser;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};

mod memfs;
mod mirror;

/// CLI tool for the nfs3_server
#[derive(Parser, Debug)]
#[command(name = "nfs3_server", version, about = "A simple NFSv3 server", long_about = None)]
struct Args {
    /// Path to the directory to serve
    #[arg(long)]
    path: String,

    /// Mount path (default is the same as `path`)
    #[arg(long)]
    mount_path: Option<String>,

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

fn main() {
    let args = Args::parse();

    init_logging(&args);
}

fn init_logging(args: &Args) {
    let log_level = match args.log_level.to_lowercase().as_str() {
        "error" => tracing::Level::ERROR,
        "warn" => tracing::Level::WARN,
        "info" => tracing::Level::INFO,
        "debug" => tracing::Level::DEBUG,
        "trace" => tracing::Level::TRACE,
        _ => panic!("invalid log level: {}", args.log_level),
    };

    let builder = tracing_subscriber::fmt().with_max_level(log_level);

    match (args.quiet, args.log_file.as_deref()) {
        (true, None) => {
            // No logging
        }
        (false, None) => {
            // Console logging
            builder.with_writer(stdout_logger).init();
        }
        (true, Some(log_file)) => {
            // File logging only
            init_file_logger(log_file);
            builder.with_writer(file_logger).init();
        }
        (false, Some(log_file)) => {
            // both console and file logging
            init_file_logger(log_file);
            builder
                .with_writer(file_logger)
                .with_writer(stdout_logger)
                .init();
        }
    }
}

static STDOUT_LOGGER: LazyLock<(NonBlocking, WorkerGuard)> =
    LazyLock::new(|| tracing_appender::non_blocking(std::io::stdout()));

fn stdout_logger() -> impl std::io::Write {
    STDOUT_LOGGER.0.clone()
}

static FILE_LOGGER: std::sync::OnceLock<(NonBlocking, WorkerGuard)> = std::sync::OnceLock::new();

fn file_logger() -> impl std::io::Write {
    FILE_LOGGER
        .get()
        .expect("file logger not initialized")
        .0
        .clone()
}

fn init_file_logger(log_file: &str) {
    let path = std::path::Path::new(log_file);
    let file_appender = tracing_appender::rolling::never(
        path.parent().unwrap_or_else(|| std::path::Path::new(".")),
        path.file_name().expect("log file name is empty"),
    );
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    FILE_LOGGER
        .set((non_blocking, guard))
        .expect("file logger already initialized");
}
