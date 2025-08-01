use std::env;

use nfs3_client::Nfs3ConnectionBuilder;
use nfs3_client::nfs3_types::nfs3;
use nfs3_client::nfs3_types::portmap::PMAP_PORT;
use nfs3_client::nfs3_types::rpc::{auth_unix, opaque_auth};
use nfs3_client::nfs3_types::xdr_codec::Opaque;
use nfs3_client::tokio::TokioConnector;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().collect::<Vec<_>>();
    let ip = args.get(1).map_or("127.0.0.1", String::as_str).to_owned();
    let mount_path = args.get(2).map_or("/", String::as_str).to_owned();
    let portmaper_port = args.get(3).map_or(PMAP_PORT, |port| {
        port.parse::<u16>().expect("invalid port number")
    });

    let auth_unix = auth_unix {
        stamp: 0xaaaa_aaaa,
        machinename: Opaque::borrowed(b"unknown"),
        uid: 0xffff_fffe,
        gid: 0xffff_fffe,
        gids: vec![],
    };
    let credential = opaque_auth::auth_unix(&auth_unix);

    let mut connection = Nfs3ConnectionBuilder::new(TokioConnector, ip, mount_path)
        .portmapper_port(portmaper_port)
        .credential(credential)
        .mount()
        .await?;

    println!("Mount result: {:?}", connection.mount_resok);

    let root = connection.root_nfs_fh3();

    println!("Calling null");
    connection.null().await?;

    println!("Calling fsinfo");
    let fsinfo = connection
        .fsinfo(&nfs3::FSINFO3args {
            fsroot: root.clone(),
        })
        .await?;

    match fsinfo {
        nfs3::FSINFO3res::Ok(ok) => {
            println!("fsinfo: {ok:?}");
        }
        nfs3::FSINFO3res::Err((err, _)) => {
            eprintln!("fsinfo error: {}", err as u32);
        }
    }

    println!("Calling access");
    let access = connection
        .access(&nfs3::ACCESS3args {
            object: root.clone(),
            access: 0,
        })
        .await?;
    println!("access: {access:?}");

    println!("Calling readdir");
    let readdir = connection
        .readdir(&nfs3::READDIR3args {
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
            eprintln!("readdir error: {err} ({})", err as u32);
        }
    }

    connection.unmount().await?;

    Ok(())
}
