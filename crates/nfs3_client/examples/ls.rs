use std::env;
use std::time::{Duration, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use nfs3_client::Nfs3ConnectionBuilder;
use nfs3_client::tokio::TokioConnector;
use nfs3_types::nfs3::{self, Nfs3Option};
use nfs3_types::xdr_codec::Opaque;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().collect::<Vec<_>>();
    if args.len() < 3 {
        eprintln!("Usage: ls <server_ip> <mount_path> [portmapper_port]");
        return Ok(());
    }

    let ip = args[1].clone();
    let mount_path = args[2].clone();
    let portmapper_port = args
        .get(3)
        .and_then(|port| port.parse::<u16>().ok())
        .unwrap_or(nfs3_types::portmap::PMAP_PORT);

    // This auth values are the same that Windows NFS client uses
    let auth_unix = nfs3_types::rpc::auth_unix {
        stamp: 0xaaaa_aaaa,
        machinename: Opaque::borrowed(b"unknown"),
        uid: 0xffff_fffe,
        gid: 0xffff_fffe,
        gids: vec![],
    };
    let credential = nfs3_types::rpc::opaque_auth::auth_unix(auth_unix);

    let mut connection = Nfs3ConnectionBuilder::new(TokioConnector, ip, mount_path)
        .portmapper_port(portmapper_port)
        .credential(credential)
        .mount()
        .await?;

    let root = connection.root_nfs_fh3();

    println!("Listing root directory using readdir:");
    let mut cookie = nfs3::cookie3::default();
    let mut cookieverf = nfs3::cookieverf3::default();
    loop {
        let readdir = connection
            .readdir(nfs3::READDIR3args {
                dir: root.clone(),
                cookie,
                cookieverf,
                count: 128 * 1024,
            })
            .await?
            .unwrap();

        let entries = readdir.reply.entries.into_inner();
        for entry in &entries {
            let name = String::from_utf8_lossy(&entry.name.0);
            println!("{name}")
        }

        if readdir.reply.eof {
            break;
        }

        cookie = entries.last().unwrap().cookie;
        cookieverf = readdir.cookieverf;
    }

    println!();
    println!("Listing root directory using readdirplus:");
    let mut cookie = nfs3::cookie3::default();
    let mut cookieverf = nfs3::cookieverf3::default();
    loop {
        let readdirplus = connection
            .readdirplus(nfs3::READDIRPLUS3args {
                dir: root.clone(),
                cookie,
                cookieverf,
                maxcount: 128 * 1024,
                dircount: 128 * 1024,
            })
            .await?
            .unwrap();

        let entries = readdirplus.reply.entries.into_inner();
        for entry in &entries {
            let name = String::from_utf8_lossy(&entry.name.0);
            let attrs = &entry.name_attributes;
            let (is_dir, size, mtime) = if let Nfs3Option::Some(attrs) = attrs {
                let is_dir = matches!(attrs.type_, nfs3::ftype3::NF3DIR);
                let mtime = &attrs.mtime;
                let duration = Duration::new(mtime.seconds as u64, mtime.nseconds);
                let systime = UNIX_EPOCH.checked_add(duration).unwrap_or(UNIX_EPOCH);
                (is_dir, attrs.size, systime)
            } else {
                (false, 0, UNIX_EPOCH)
            };
            let mtime: DateTime<Utc> = mtime.into();
            if is_dir {
                let dirname = format!("[{name}]");
                println!("{dirname:<60} {:>10} {mtime}", " ");
            } else {
                println!("{name:<60} {size:>10} {mtime}");
            };
        }

        if readdirplus.reply.eof {
            break;
        }

        cookie = entries.last().unwrap().cookie;
        cookieverf = readdirplus.cookieverf;
    }

    connection.unmount().await?;
    Ok(())
}
