use clap::Parser;

mod mirror;
mod memfs;

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


}
