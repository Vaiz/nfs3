use std::io::Result;
use std::sync::Arc;

use async_trait::async_trait;
use nfs3_client::io::{AsyncRead, AsyncWrite};
use tokio::sync::{Mutex, Notify};

/// A two-way in-memory channel for mock client-server communication.
///
/// Each endpoint has:
/// - an `incoming` buffer for data coming from the remote peer,
/// - an `outgoing` buffer for data written by the local side (which becomes the remote’s incoming),
/// - a `notify` used to wake up tasks waiting on incoming data,
/// - and a `remote_notify` (when connected) so that writes notify the remote end.
#[derive(Clone)]
pub struct MockChannel {
    incoming: Arc<Mutex<Vec<u8>>>,
    outgoing: Arc<Mutex<Vec<u8>>>,
    notify: Arc<Notify>,
    remote_notify: Option<Arc<Notify>>,
}

impl MockChannel {
    /// Create a new standalone channel.
    /// (For connected endpoints, use `MockChannel::pair()`.)
    pub fn new() -> Self {
        Self {
            incoming: Arc::new(Mutex::new(Vec::new())),
            outgoing: Arc::new(Mutex::new(Vec::new())),
            notify: Arc::new(Notify::new()),
            remote_notify: None,
        }
    }

    /// Create a connected pair of channels.
    ///
    /// Data written on one channel becomes available for reading on the other, and vice versa.
    pub fn pair() -> (Self, Self) {
        let client_incoming = Arc::new(Mutex::new(Vec::new()));
        let server_incoming = Arc::new(Mutex::new(Vec::new()));

        let client_notify = Arc::new(Notify::new());
        let server_notify = Arc::new(Notify::new());

        // For the client, its incoming is its own client_incoming,
        // and its outgoing is the server's incoming.
        let client = Self {
            incoming: client_incoming.clone(),
            outgoing: server_incoming.clone(),
            notify: client_notify.clone(),
            remote_notify: Some(server_notify.clone()),
        };

        // For the server, its incoming is its own server_incoming,
        // and its outgoing is the client's incoming.
        let server = Self {
            incoming: server_incoming,
            outgoing: client_incoming,
            notify: server_notify,
            remote_notify: Some(client_notify),
        };

        (client, server)
    }

    /// For a standalone channel: manually add data to the incoming buffer.
    pub async fn add_incoming_data(&self, data: &[u8]) {
        let mut incoming = self.incoming.lock().await;
        incoming.extend_from_slice(data);
        self.notify.notify_waiters();
    }

    /// For a standalone channel: take all data from the outgoing buffer.
    pub async fn take_outgoing_data(&self) -> Vec<u8> {
        let mut outgoing = self.outgoing.lock().await;
        let data = outgoing.clone();
        outgoing.clear();
        data
    }
}

#[async_trait(?Send)]
impl AsyncRead for MockChannel {
    async fn async_read(&mut self, buf: &mut [u8]) -> Result<usize> {
        loop {
            let mut incoming = self.incoming.lock().await;
            if !incoming.is_empty() {
                let n = buf.len().min(incoming.len());
                buf[..n].copy_from_slice(&incoming[..n]);
                incoming.drain(..n);
                return Ok(n);
            }
            // Drop lock before waiting.
            drop(incoming);
            self.notify.notified().await;
        }
    }
}

#[async_trait(?Send)]
impl AsyncWrite for MockChannel {
    async fn async_write(&mut self, buf: &[u8]) -> Result<usize> {
        let mut outgoing = self.outgoing.lock().await;
        outgoing.extend_from_slice(buf);
        // Notify the remote side if applicable.
        if let Some(remote_notify) = &self.remote_notify {
            remote_notify.notify_waiters();
        }
        Ok(buf.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn test_channel() -> Result<()> {
        // Create a connected pair: client and server.
        let (mut client, mut server) = MockChannel::pair();

        // Spawn a server task: server reads a message from the client and then responds.
        let server_task = async move {
            let mut buffer = vec![0u8; 1024];
            // Server reads incoming message.
            let n = server
                .async_read(&mut buffer)
                .await
                .expect("Server failed to read");
            let received = String::from_utf8_lossy(&buffer[..n]);
            println!("Server received: {}", received);

            // Prepare and send a response.
            let response = b"Hello, client!";
            server
                .async_write_all(response)
                .await
                .expect("Server failed to write");
        };

        // Client sends a message to the server.
        let client_task = async move {
            let client_message = b"Hello, server!";
            client
                .async_write_all(client_message)
                .await
                .expect("Client failed to write");

            // Client waits and reads the server's response.
            let mut response_buffer = vec![0u8; 1024];
            let n = client
                .async_read(&mut response_buffer)
                .await
                .expect("Client failed to read");
            let response = String::from_utf8_lossy(&response_buffer[..n]);
            println!("Client received: {}", response);
        };

        tokio::join!(server_task, client_task);

        Ok(())
    }
}
