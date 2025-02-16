#[cfg(feature = "tokio")]
pub mod tokio;

use crate::io::{AsyncRead, AsyncWrite};

#[async_trait::async_trait(?Send)]
pub trait Connector {
    type Connection: AsyncRead + AsyncWrite;

    async fn connect(&self, host: &str, port: u16) -> std::io::Result<Self::Connection>;
}
