//! Traits for connecting to an endpoint.

use std::future::Future;
use std::net::SocketAddr;

use crate::io::{AsyncRead, AsyncWrite};

/// Trait for connecting to an endpoint.
pub trait Connector: Send {
    type Connection: AsyncRead + AsyncWrite + Send;

    /// Connect to a remote address.
    fn connect(
        &self,
        addr: SocketAddr,
    ) -> impl Future<Output = std::io::Result<Self::Connection>> + Send;

    /// Many NFS servers, especially on Linux, require that the source port used for NFS
    /// communication be in the privileged range (0-1023) by default.
    /// When the `local_port` is already in use, the function should return `AddrInUse` error.
    fn connect_with_port(
        &self,
        addr: SocketAddr,
        local_port: u16,
    ) -> impl Future<Output = std::io::Result<Self::Connection>> + Send;
}
