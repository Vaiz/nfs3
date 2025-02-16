#[cfg(feature = "tokio")]
pub mod tokio;

use crate::io::{AsyncRead, AsyncWrite};

/// Trait for connecting to a host and port.
#[async_trait::async_trait(?Send)]
pub trait Connector {
    type Connection: AsyncRead + AsyncWrite;

    /// Connect to a host and port.
    async fn connect(&self, host: &str, port: u16) -> std::io::Result<Self::Connection>;
}
