use tokio::net::TcpStream;

use super::Connector;
use crate::io::tokio::TokioIo;

pub struct TokioConnector;

#[async_trait::async_trait(?Send)]
impl Connector for TokioConnector {
    type Connection = TokioIo<TcpStream>;

    async fn connect(&self, host: &str, port: u16) -> std::io::Result<Self::Connection> {
        let addr = format!("{}:{}", host, port);
        let stream = tokio::net::TcpStream::connect(&addr).await?;
        Ok(TokioIo::new(stream))
    }
}
