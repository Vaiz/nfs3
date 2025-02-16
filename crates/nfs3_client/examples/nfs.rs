use std::env;

use nfs3_client::io::{AsyncRead, AsyncWrite};
use nfs3_client::net::tokio::TokioConnector;
use nfs3_client::nfs::Nfs3Client;
use nfs3_types::mount::mountres3_ok;
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

    let portmaper_port = match args.get(3) {
        Some(port) => port.parse::<u16>().unwrap(),
        None => nfs3_types::portmap::PMAP_PORT,
    };

    let (mut client, mount_res) = connect(TokioConnector, ip, mount_path, portmaper_port).await?;
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

    // TODO: unmount

    Ok(())
}

pub async fn connect<C, S>(
    connector: C,
    host: &str,
    mount_path: &str,
    portmaper_port: u16,
) -> Result<(Nfs3Client<S>, mountres3_ok<'static>), nfs3_client::error::Error>
where
    C: nfs3_client::net::Connector<Connection = S>,
    S: AsyncRead + AsyncWrite + 'static,
{
    use nfs3_client::mount::MountClient;
    use nfs3_client::portmapper::PortmapperClient;
    use nfs3_client::rpc::RpcClient;
    use nfs3_types::mount::dirpath;
    use nfs3_types::xdr_codec::Opaque;

    let rpc = connector.connect(host, portmaper_port).await?;
    let rpc = RpcClient::new(rpc);
    let mut portmapper = PortmapperClient::new(rpc);

    let mount_port = portmapper
        .getport(nfs3_types::mount::PROGRAM, nfs3_types::mount::VERSION)
        .await?;
    let nfs_port = portmapper
        .getport(nfs3_types::nfs3::PROGRAM, nfs3_types::nfs3::VERSION)
        .await?;

    let mount_rpc = connector.connect(host, mount_port as u16).await?;
    let mount_rpc = RpcClient::new(mount_rpc);
    let mut mount = MountClient::new(mount_rpc);
    let mount_path = Opaque::borrowed(mount_path.as_bytes());
    let mount_res = mount.mnt(dirpath(mount_path)).await?;

    let rpc = connector.connect(host, nfs_port as u16).await?;
    let rpc = RpcClient::new(rpc);
    let nfs3_client = Nfs3Client::new(rpc);

    Ok((nfs3_client, mount_res))
}
