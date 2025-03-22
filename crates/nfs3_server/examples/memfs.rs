use nfs3_server::tcp::*;

const HOSTPORT: u32 = 11111;

// To mount the NFS server on Linux, use the following command:
// mount -t nfs -o nolocks,vers=3,tcp,port=11111,mountport=11111,soft 127.0.0.1:/ mnt/
//
// Usage:
// cargo run --example demo [bind_ip] [bind_port]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(std::io::stderr)
        .init();

    let args = std::env::args().collect::<Vec<_>>();
    let bind_ip = args.get(1).map(|s| s.as_str()).unwrap_or("0.0.0.0");

    let bind_port = args
        .get(2)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(HOSTPORT as u16);

    let memfs = nfs3_server::memfs::MemFs::new(default_config()).unwrap();
    let listener = NFSTcpListener::bind(&format!("{bind_ip}:{bind_port}"), memfs).await?;
    listener.handle_forever().await?;

    Ok(())
}

fn default_config() -> nfs3_server::memfs::MemFsConfig {
    const CAT: &str = r#"
    /\_____/\
   /  o   o  \
  ( ==  ^  == )
   )         (
  (           )
 ( (  )   (  ) )
(__(__)___(__)__)
"#;

    let mut config = nfs3_server::memfs::MemFsConfig::default();
    config.add_file("/a.txt", "hello world\n".as_bytes());
    config.add_file("/b.txt", "Greetings\n".as_bytes());
    config.add_file("/cat.txt", CAT.as_bytes());
    config.add_dir("/a directory");
    for i in 0..10 {
        config.add_file(&format!("/a directory/{i}.txt"), i.to_string().as_bytes());
    }
    config
}
