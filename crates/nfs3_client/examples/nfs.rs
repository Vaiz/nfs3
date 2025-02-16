use std::env;

use nfs3_client::net::tokio::TokioConnector;
use nfs3_types::nfs3;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().collect::<Vec<_>>();
    let ip = match args.get(1) {
        Some(ip) => ip.as_str(),
        None => "127.0.0.1",
    };

    let mount_path = match args.get(2) {
        Some(path) => path.as_str(),
        None => "/",
    };

    let (mut client, mount_res) = nfs3_client::nfs::connect(TokioConnector, ip, mount_path).await?;
    println!("Mount result: {:?}", mount_res);

    let root = nfs3::nfs_fh3 {
        data: mount_res.fhandle.0,
    };

    println!("Calling null");
    client.null().await?;

    println!("Calling fsinfo");
    let fsinfo = client
        .fsinfo(nfs3::FSINFO3args {
            fsroot: root.clone(),
        })
        .await?;
    println!("Fsinfo: {:?}", fsinfo);

    println!("Calling readdir");
    let readdir = client
        .readdir(nfs3::READDIR3args {
            dir: root,
            cookie: 0,
            cookieverf: nfs3::cookieverf3::default(),
            count: 128 * 1024 * 1024,
        })
        .await?;

    println!("Readdir: {:?}", readdir);

    Ok(())
}
