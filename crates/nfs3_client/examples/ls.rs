use std::env;
use nfs3_client::Nfs3ConnectionBuilder;
use nfs3_client::tokio::TokioConnector;
use nfs3_types::nfs3;


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

    let mut connection = Nfs3ConnectionBuilder::new(TokioConnector, ip, mount_path)
        .portmapper_port(portmapper_port)
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

    connection.unmount().await?;
    Ok(())
}