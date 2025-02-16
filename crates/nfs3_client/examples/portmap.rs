use std::env;

use nfs3_client::io::tokio::TokioIo;
use nfs3_client::portmapper;
use nfs3_client::rpc::RpcClient;
use tokio::net::TcpStream;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().collect::<Vec<_>>();
    let ip = match args.get(1) {
        Some(ip) => ip.as_str(),
        None => "127.0.0.1",
    };
    let port = match args.get(2) {
        Some(port) => port.parse::<u16>()?,
        None => nfs3_types::portmap::PMAP_PORT,
    };

    let stream = TcpStream::connect(format!("{ip}:{port}")).await?;
    println!("Connected to portmapper on {ip}:{port}");
    let rpc = RpcClient::new(TokioIo::new(stream));
    let mut portmapper = portmapper::PortmapperClient::new(rpc);

    portmapper.null().await?;
    let port = portmapper.getport(nfs3_types::nfs3::PROGRAM, nfs3_types::nfs3::VERSION).await?;
    println!("Resolved NFSv3 port: {}", port);

    Ok(())
}
