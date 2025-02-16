use std::env;

use nfs3_client::error::{Error, PortmapError};
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

    let result = portmapper
        .getport(nfs3_types::mount::PROGRAM, nfs3_types::mount::VERSION)
        .await;
    match result {
        Ok(port) => println!("Resolved MOUNT3 port: {port}"),
        Err(Error::Portmap(PortmapError::ProgramUnavailable)) => {
            eprintln!("MOUNT3 program is unavailable")
        }
        Err(e) => eprintln!("Failed to resolve MOUNT3 port: {e}"),
    }

    let result = portmapper
        .getport(nfs3_types::nfs3::PROGRAM, nfs3_types::nfs3::VERSION)
        .await;
    match result {
        Ok(port) => println!("Resolved NFSv3 port: {port}"),
        Err(Error::Portmap(PortmapError::ProgramUnavailable)) => {
            eprintln!("NFSv3 program is unavailable")
        }
        Err(e) => eprintln!("Failed to resolve NFSv3 port: {e}"),
    }

    let dump = portmapper.dump().await?;
    println!("Portmap dump:");
    println!("Program | Version |  Port");
    for mapping in dump {
        println!("{:>7}   {:>7}   {:>5}", mapping.prog, mapping.vers, mapping.port);
    }

    Ok(())
}
