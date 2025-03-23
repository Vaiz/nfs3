//! Traits for connecting to an endpoint.

use crate::io::{AsyncRead, AsyncWrite};

/// Trait for connecting to a host and port.
#[async_trait::async_trait(?Send)]
pub trait Connector {
    type Connection: AsyncRead + AsyncWrite;

    /// Connect to a host and port.
    async fn connect(&self, host: &str, port: u16) -> std::io::Result<Self::Connection>;

    /// Many NFS clients, especially on Linux, require that the source port used for NFS
    /// communication be in the privileged range (0-1023) by default.
    /// When the `local_port` is already in use, the function should return `AddrInUse` error.
    async fn connect_with_port(
        &self,
        host: &str,
        port: u16,
        local_port: u16,
    ) -> std::io::Result<Self::Connection>;
}
