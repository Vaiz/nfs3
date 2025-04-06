//! Provides wrappers for tokio's types

use tokio::io::{AsyncRead as TokioAsyncRead, AsyncWrite as TokioAsyncWrite};
use tokio::net::{TcpSocket, TcpStream};

use crate::io::{AsyncRead, AsyncWrite};
use crate::net::Connector;

/// Wrapper for Tokio types
///
/// Wraps a Tokio's [`AsyncRead`](TokioAsyncRead) and [`AsyncWrite`](TokioAsyncWrite) implementor
/// to provide an [`AsyncRead`] and [`AsyncWrite`] implementation.
pub struct TokioIo<T>(T);

impl<T> TokioIo<T> {
    pub const fn new(inner: T) -> Self {
        Self(inner)
    }
}

#[async_trait::async_trait(?Send)]
impl<T> AsyncRead for TokioIo<T>
where
    T: TokioAsyncRead + Unpin,
{
    async fn async_read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        tokio::io::AsyncReadExt::read(&mut self.0, buf).await
    }
}

#[async_trait::async_trait(?Send)]
impl<T> AsyncWrite for TokioIo<T>
where
    T: TokioAsyncWrite + Unpin,
{
    async fn async_write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        tokio::io::AsyncWriteExt::write(&mut self.0, buf).await
    }
}

/// Connector for Tokio
///
/// Connects to a host and port using Tokio's [`TcpStream`].
pub struct TokioConnector;

#[async_trait::async_trait(?Send)]
impl Connector for TokioConnector {
    type Connection = TokioIo<TcpStream>;

    async fn connect(&self, host: &str, port: u16) -> std::io::Result<Self::Connection> {
        let addr = format!("{host}:{port}");
        let stream = tokio::net::TcpStream::connect(&addr).await?;
        Ok(TokioIo::new(stream))
    }

    async fn connect_with_port(
        &self,
        host: &str,
        port: u16,
        local_port: u16,
    ) -> std::io::Result<Self::Connection> {
        let socket = TcpSocket::new_v4()?;
        socket.bind(format!("0.0.0.0:{local_port}").parse().unwrap())?;

        let addr = format!("{host}:{port}").parse().unwrap();
        let stream = socket.connect(addr).await?;
        Ok(TokioIo::new(stream))
    }
}
