//! Provides wrappers for smol's types

use smol::io::{AsyncRead as SmolAsyncRead, AsyncWrite as SmolAsyncWrite};
use smol::net::TcpStream;

use crate::io::{AsyncRead, AsyncWrite};
use crate::net::Connector;

/// Wrapper for Smol types
///
/// Wraps a Smol's [`AsyncRead`](SmolAsyncRead) and [`AsyncWrite`](SmolAsyncWrite) implementor
/// to provide an [`AsyncRead`] and [`AsyncWrite`] implementation.
pub struct SmolIo<T>(T);

impl<T> SmolIo<T> {
    pub const fn new(inner: T) -> Self {
        Self(inner)
    }
}

impl<T> AsyncRead for SmolIo<T>
where
    T: SmolAsyncRead + Unpin + Send,
{
    async fn async_read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        smol::io::AsyncReadExt::read(&mut self.0, buf).await
    }
}

impl<T> AsyncWrite for SmolIo<T>
where
    T: SmolAsyncWrite + Unpin + Send,
{
    async fn async_write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        smol::io::AsyncWriteExt::write(&mut self.0, buf).await
    }
}

/// Connector for Smol
///
/// Connects to a host and port using Smol's [`TcpStream`].
pub struct SmolConnector;

impl Connector for SmolConnector {
    type Connection = SmolIo<TcpStream>;

    async fn connect(&self, host: &str, port: u16) -> std::io::Result<Self::Connection> {
        let addr = format!("{host}:{port}");
        let stream = TcpStream::connect(&addr).await?;
        Ok(SmolIo::new(stream))
    }

    async fn connect_with_port(
        &self,
        host: &str,
        port: u16,
        local_port: u16,
    ) -> std::io::Result<Self::Connection> {
        // TODO: Implement proper local port binding for smol
        // For now, fall back to regular connect. This covers most use cases.
        // The privileged port binding is mainly needed for some NFS server configurations
        let _ = local_port; // Suppress unused parameter warning
        self.connect(host, port).await
    }
}