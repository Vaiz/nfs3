use std::cmp::min;
use std::io::{Error, ErrorKind, Result};
use std::pin::Pin;
use std::task::{Context, Poll};

use async_trait::async_trait;
use nfs3_client::io::{AsyncRead, AsyncWrite};
use tokio::io::ReadBuf;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// A two-way channel using Tokio’s MPSC channels.
/// Data is sent as Vec<u8> messages; a small pending buffer is used
/// to allow partial consumption from a received message.
#[derive(Debug)]
pub struct MockChannel {
    /// Receiver for incoming messages.
    read_rx: UnboundedReceiver<Vec<u8>>,
    /// Sender for outgoing messages.
    write_tx: UnboundedSender<Vec<u8>>,
    /// Holds leftover bytes from a received message that weren't consumed.
    pending: Vec<u8>,
}

impl MockChannel {
    /// Create a connected pair of channels.
    ///
    /// Data written on one endpoint becomes available on the other.
    pub fn pair() -> (Self, Self) {
        let (tx_a, rx_a) = mpsc::unbounded_channel();
        let (tx_b, rx_b) = mpsc::unbounded_channel();

        let client = Self {
            read_rx: rx_a,
            write_tx: tx_b,
            pending: Vec::new(),
        };

        let server = Self {
            read_rx: rx_b,
            write_tx: tx_a,
            pending: Vec::new(),
        };

        (client, server)
    }
}

#[async_trait(?Send)]
impl AsyncRead for MockChannel {
    async fn async_read(&mut self, buf: &mut [u8]) -> Result<usize> {
        // If we have some pending data from a previous message, use that first.
        if !self.pending.is_empty() {
            let n = min(buf.len(), self.pending.len());
            buf[..n].copy_from_slice(&self.pending[..n]);
            self.pending.drain(..n);
            return Ok(n);
        }
        // Otherwise, await a new message.
        match self.read_rx.recv().await {
            Some(chunk) => {
                // Copy as much as possible into the provided buffer.
                let n = min(buf.len(), chunk.len());
                buf[..n].copy_from_slice(&chunk[..n]);
                // Save any leftover bytes for subsequent reads.
                if n < chunk.len() {
                    self.pending.extend_from_slice(&chunk[n..]);
                }
                Ok(n)
            }
            None => {
                // The channel is closed and no more messages will come.
                Err(Error::new(ErrorKind::BrokenPipe, "Channel is closed"))
            }
        }
    }
}

#[async_trait(?Send)]
impl AsyncWrite for MockChannel {
    async fn async_write(&mut self, buf: &[u8]) -> Result<usize> {
        // Send the entire slice as a single message.
        // If the send fails, it means the remote side is closed.
        self.write_tx
            .send(buf.to_vec())
            .map_err(|_| Error::new(ErrorKind::BrokenPipe, "Channel is closed"))?;
        Ok(buf.len())
    }
}

/// Tokio’s AsyncRead implementation.
impl tokio::io::AsyncRead for MockChannel {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<Result<()>> {
        // First, if we have pending data, copy as much as possible.
        if !self.pending.is_empty() {
            let to_copy = min(buf.remaining(), self.pending.len());
            buf.put_slice(&self.pending[..to_copy]);
            self.pending.drain(..to_copy);
            return Poll::Ready(Ok(()));
        }
        // Otherwise, try to receive a new message.
        match self.read_rx.poll_recv(cx) {
            Poll::Ready(Some(chunk)) => {
                let to_copy = min(buf.remaining(), chunk.len());
                buf.put_slice(&chunk[..to_copy]);
                if to_copy < chunk.len() {
                    self.pending.extend_from_slice(&chunk[to_copy..]);
                }
                Poll::Ready(Ok(()))
            }
            Poll::Ready(None) => {
                // Channel is closed, signal EOF.
                Poll::Ready(Err(Error::new(ErrorKind::BrokenPipe, "Channel is closed")))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Tokio’s AsyncWrite implementation.
impl tokio::io::AsyncWrite for MockChannel {
    fn poll_write(self: Pin<&mut Self>, _cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize>> {
        match self.write_tx.send(buf.to_vec()) {
            Ok(()) => Poll::Ready(Ok(buf.len())),
            Err(_) => Poll::Ready(Err(Error::new(ErrorKind::BrokenPipe, "Channel is closed"))),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<()>> {
        // No buffering is performed beyond each message.
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<()>> {
        // not implemented
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Result;

    use nfs3_client::io::AsyncRead;

    use super::MockChannel;

    const N: usize = 4;
    const CLIENT_MSG: &[u8] = b"Hello, server!";
    const SERVER_MSG: &[u8] = b"Hello, client!";

    #[tokio::test]
    async fn test_channel_custom_traits() -> Result<()> {
        use nfs3_client::io::{AsyncRead, AsyncWrite};
        let (mut client, mut server) = MockChannel::pair();

        // Spawn server task that reads a message and responds.
        let server_task = async move {
            let mut buf = vec![0u8; 64];

            for _ in 0..N {
                let n = server
                    .async_read(&mut buf)
                    .await
                    .expect("Server failed to read");

                let received = &buf[..n];
                assert_eq!(received, CLIENT_MSG);

                server
                    .async_write_all(SERVER_MSG)
                    .await
                    .expect("Server failed to write");
            }

            let result = server.async_read(&mut buf).await;
            println!("received: {result:?}");
            assert!(result.is_err());
        };

        // Client sends a message and waits for a response.
        let client_task = async move {
            for _ in 0..N {
                client
                    .async_write_all(CLIENT_MSG)
                    .await
                    .expect("Client failed to write");
                let mut buf = vec![0u8; 64];
                let n = client
                    .async_read(&mut buf)
                    .await
                    .expect("Client failed to read");
                let received = &buf[..n];
                assert_eq!(received, SERVER_MSG);
            }
        };

        let _ = tokio::join!(server_task, client_task);
        Ok(())
    }

    #[tokio::test]
    async fn test_channel_tokio_traits() -> Result<()> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let (mut client, mut server) = MockChannel::pair();

        // Use Tokio's AsyncWrite/AsyncRead.
        let server_task = async move {
            let mut buf = vec![0u8; 64];
            for _ in 0..N {
                let n = server.read(&mut buf).await.expect("Server failed to read");
                let received = &buf[..n];
                assert_eq!(received, CLIENT_MSG);

                server
                    .write_all(SERVER_MSG)
                    .await
                    .expect("Server failed to write");
            }

            let result = server.async_read(&mut buf).await;
            assert!(result.is_err());
        };

        let client_task = async move {
            let mut buf = vec![0u8; 64];
            for _ in 0..N {
                client
                    .write_all(CLIENT_MSG)
                    .await
                    .expect("Client failed to write");

                let n = client.read(&mut buf).await.expect("Client failed to read");
                let received = &buf[..n];
                assert_eq!(received, SERVER_MSG);
            }
        };

        let _ = tokio::join!(server_task, client_task);
        Ok(())
    }
}
