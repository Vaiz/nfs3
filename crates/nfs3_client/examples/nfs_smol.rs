use std::env;

use nfs3_client::Nfs3ConnectionBuilder;
use nfs3_client::nfs3_types::nfs3;
use nfs3_client::nfs3_types::portmap::PMAP_PORT;
use nfs3_client::smol::SmolConnector;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    smol::block_on(async {
        let args = env::args().collect::<Vec<_>>();
        let ip = args.get(1).map_or("127.0.0.1", String::as_str).to_owned();
        let mount_path = args.get(2).map_or("/", String::as_str).to_owned();
        let portmaper_port = args.get(3).map_or(PMAP_PORT, |port| {
            port.parse::<u16>().expect("invalid port number")
        });

        let mut connection = Nfs3ConnectionBuilder::new(SmolConnector, ip, mount_path)
            .portmapper_port(portmaper_port)
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

        println!("fsinfo: {fsinfo:?}");

        println!("Calling readdir");
        let readdir = connection
            .readdir(&nfs3::READDIR3args {
                dir: root,
                cookie: 0,
                cookieverf: nfs3::cookieverf3::default(),
                count: 128 * 1024 * 1024,
            })
            .await?;

        println!("{readdir:?}");

        connection.unmount().await?;

        Ok(())
    })
}
