use std::env;

use nfs3_client::Nfs3ConnectionBuilder;
use nfs3_client::tokio::TokioConnector;
use nfs3_types::nfs3;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().collect::<Vec<_>>();
    let ip = match args.get(1) {
        Some(ip) => ip.clone(),
        None => "127.0.0.1".to_string(),
    };

    let mount_path = match args.get(2) {
        Some(path) => path.clone(),
        None => "/".to_string(),
    };

    let portmaper_port = match args.get(3) {
        Some(port) => port.parse::<u16>().unwrap(),
        None => nfs3_types::portmap::PMAP_PORT,
    };

    let mut connection = Nfs3ConnectionBuilder::new(TokioConnector, ip, mount_path)
        .portmapper_port(portmaper_port)
        .mount()
        .await?;

    println!("Mount result: {:?}", connection.mount_resok);

    let root = connection.root_nfs_fh3();

    println!("Calling null");
    connection.null().await?;

    println!("Calling fsinfo");
    let fsinfo = connection
        .fsinfo(nfs3::FSINFO3args {
            fsroot: root.clone(),
        })
        .await?;

    match fsinfo {
        nfs3::FSINFO3res::Ok(ok) => {
            println!("fsinfo: {:?}", ok);
        }
        nfs3::FSINFO3res::Err((err, _)) => {
            eprintln!("fsinfo error: {}", err as u32);
        }
    }

    println!("Calling access");
    let access = connection
        .access(nfs3::ACCESS3args {
            object: root.clone(),
            access: 0,
        })
        .await?;
    println!("access: {access:?}");

    println!("Calling readdir");
    let readdir = connection
        .readdir(nfs3::READDIR3args {
            dir: root,
            cookie: 0,
            cookieverf: nfs3::cookieverf3::default(),
            count: 128 * 1024 * 1024,
        })
        .await?;

    match readdir {
        nfs3::READDIR3res::Ok(ok) => {
            println!("readdir:");
            for entry in ok.reply.entries.0 {
                println!("  {}", String::from_utf8_lossy(entry.name.0.as_ref()));
            }
            println!("  eof: {}", ok.reply.eof);
        }
        nfs3::READDIR3res::Err((err, _)) => {
            eprintln!("readdir error: {}", err as u32);
        }
    }

    connection.unmount().await?;

    Ok(())
}
