//! Provides wrappers for smol's types

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

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
        let stream = TcpStream::connect((host, port)).await?;
        Ok(SmolIo::new(stream))
    }

    async fn connect_with_port(
        &self,
        host: &str,
        port: u16,
        local_port: u16,
    ) -> std::io::Result<Self::Connection> {
        const EINPROGRESS: i32 = 115;

        let local_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), local_port);
        let host = host.parse::<IpAddr>()?;
        let remote_addr = SocketAddr::new(host, port);

        let domain = socket2::Domain::for_address(local_addr);
        let ty = socket2::Type::STREAM;

        let socket = socket2::Socket::new(domain, ty, Some(socket2::Protocol::TCP))?;
        socket.set_nonblocking(true)?;
        socket.bind(&local_addr.into())?;
        if let Err(err) = socket.connect(&remote_addr.into()) {
            #[cfg(unix)]
            if err.raw_os_error() != Some(EINPROGRESS) {
                return Err(err);
            }
            #[cfg(windows)]
            if err.kind() != std::io::ErrorKind::WouldBlock {
                return Err(err);
            }
        }
        
        let std_stream: std::net::TcpStream = socket.into();
        let async_socket = smol::Async::new_nonblocking(std_stream)?;
        // The stream becomes writable when connected.
        async_socket.writable().await?;

        Ok(SmolIo::new(async_socket.into()))
    }
}
