use std::env;

use nfs3_client::io::tokio::TokioIo;
use nfs3_client::portmapper;
use nfs3_client::rpc::RpcClient;
use tokio::net::TcpStream;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ip = match env::args().nth(1) {
        Some(ip) => ip,
        None => "127.0.0.1".to_string(),
    };

    let stream = TcpStream::connect(format!("{ip}:{}", nfs3_types::portmap::PMAP_PORT)).await?;
    println!("Connected to portmapper on {}", ip);
    let rpc = RpcClient::new(TokioIo::new(stream));
    let mut portmapper = portmapper::PortmapperClient::new(rpc);

    portmapper.null().await?;

    /*
    // Prepare request to get the NFSv3 TCP port
    let args = mapping {
        prog: nfs3_types::nfs3::PROGRAM,        // NFS
        vers: nfs3_types::nfs3::VERSION,        // v3
        prot: nfs3_types::portmap::IPPROTO_TCP, // TCP
        port: 0,
    };
    let _reply = rpc.call(100_000, 2, 3, args).await?;


    // Parse the returned port number
    let data = reply.ar_results.as_ref();
    let port = if data.len() >= 4 {
        u32::from_be_bytes(data[..4].try_into()?) as u16
    } else {
        0
    };

    println!("Resolved NFSv3 port: {}", port);
     */
    Ok(())
}
