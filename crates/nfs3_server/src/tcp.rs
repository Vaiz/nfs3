use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use anyhow;
use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::context::RPCContext;
use crate::rpcwire::{SocketMessageHandler, write_fragment};
use crate::transaction_tracker::{Cleaner, TransactionTracker};
use crate::units::KIBIBYTE;
use crate::vfs::NFSFileSystem;

/// A NFS Tcp Connection Handler
pub struct NFSTcpListener<T: NFSFileSystem + Send + Sync + 'static> {
    listener: TcpListener,
    port: u16,
    arcfs: Arc<T>,
    mount_signal: Option<mpsc::Sender<bool>>,
    export_name: Arc<String>,
    transaction_tracker: Arc<TransactionTracker>,
    stop_notify: Arc<tokio::sync::Notify>,
}

impl<T: NFSFileSystem + Send + Sync + 'static> Drop for NFSTcpListener<T> {
    fn drop(&mut self) {
        self.stop_notify.notify_waiters();
    }
}

#[must_use]
pub fn generate_host_ip(hostnum: u16) -> String {
    format!(
        "127.88.{}.{}",
        ((hostnum >> 8) & 0xFF) as u8,
        (hostnum & 0xFF) as u8
    )
}

/// processes an established socket
pub(crate) async fn process_socket<IO>(
    mut socket: IO,
    context: RPCContext,
) -> Result<(), anyhow::Error>
where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + 'static,
{
    let (mut message_handler, mut socksend, mut msgrecvchan) = SocketMessageHandler::new(&context);

    tokio::spawn(async move {
        loop {
            if let Err(e) = message_handler.read().await {
                debug!("Message loop broken due to {e}");
                break;
            }
        }
    });
    let mut buf = Box::new([0u8; 128 * KIBIBYTE as usize]);
    loop {
        tokio::select! {
            result = socket.read(&mut *buf) => {
                match result {
                    Ok(0) => {
                        return Ok(());
                    }
                    Ok(n) => {
                        let _ = socksend.write_all(&buf[..n]).await;
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        continue;
                    }
                    Err(e) => {
                        debug!("Message handling closed : {e}");
                        return Err(e.into());
                    }
                }

            },
            reply = msgrecvchan.recv() => {
                match reply {
                    Some(Err(e)) => {
                        debug!("Message handling closed : {e}");
                        return Err(e);
                    }
                    Some(Ok(msg)) => {
                        if let Err(e) = write_fragment(&mut socket, &msg).await {
                            error!("Write error {e}");
                        }
                    }
                    None => {
                        return Err(anyhow::anyhow!("Unexpected socket context termination"));
                    }
                }
            }
        }
    }
}

#[async_trait]
pub trait NFSTcp: Send + Sync {
    /// Gets the true listening port. Useful if the bound port number is 0
    fn get_listen_port(&self) -> u16;

    /// Gets the true listening IP. Useful on windows when the IP may be random
    fn get_listen_ip(&self) -> IpAddr;

    /// Sets a mount listener. A "true" signal will be sent on a mount
    /// and a "false" will be sent on an unmount
    fn set_mount_listener(&mut self, signal: mpsc::Sender<bool>);

    /// Loops forever and never returns handling all incoming connections.
    async fn handle_forever(&self) -> io::Result<()>;
}

impl<T: NFSFileSystem + Send + Sync + 'static> NFSTcpListener<T> {
    /// Binds to a ipstr of the form [ip address]:port. For instance
    /// "127.0.0.1:12000". fs is an instance of an implementation
    /// of `NFSFileSystem`.
    pub async fn bind(ipstr: &str, fs: T) -> io::Result<Self> {
        let (ip, port) = ipstr.split_once(':').ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::AddrNotAvailable,
                "IP Address must be of form ip:port",
            )
        })?;
        let port = port.parse::<u16>().map_err(|_| {
            io::Error::new(
                io::ErrorKind::AddrNotAvailable,
                "Port not in range 0..=65535",
            )
        })?;

        let arcfs: Arc<T> = Arc::new(fs);

        if ip == "auto" {
            let mut num_tries_left = 32;

            for try_ip in 1u16.. {
                let ip = generate_host_ip(try_ip);

                let result = Self::bind_internal(&ip, port, arcfs.clone()).await;

                match result {
                    Err(_) => {
                        if num_tries_left == 0 {
                            return result;
                        }
                        num_tries_left -= 1;
                        continue;
                    }
                    Ok(_) => {
                        return result;
                    }
                }
            }
            unreachable!(); // Does not detect automatically that loop above never terminates.
        } else {
            // Otherwise, try this.
            Self::bind_internal(ip, port, arcfs).await
        }
    }

    async fn bind_internal(ip: &str, port: u16, arcfs: Arc<T>) -> io::Result<Self> {
        let ipstr = format!("{ip}:{port}");
        let listener = TcpListener::bind(&ipstr).await?;
        info!("Listening on {:?}", &ipstr);

        let port = match listener.local_addr().unwrap() {
            SocketAddr::V4(s) => s.port(),
            SocketAddr::V6(s) => s.port(),
        };
        Ok(Self {
            listener,
            port,
            arcfs,
            mount_signal: None,
            export_name: Arc::from("/".to_string()),
            transaction_tracker: Self::new_transaction_tracker(),
            stop_notify: Arc::new(tokio::sync::Notify::new()),
        })
    }

    fn new_transaction_tracker() -> Arc<TransactionTracker> {
        const TRANSACTION_LIFETIME: Duration = Duration::from_secs(60);
        const MAX_ACTIVE_TRANSACTIONS: u16 = 256;
        const TRANSACTION_TRIM_THRESHOLD: usize = 2048;

        Arc::new(TransactionTracker::new(
            TRANSACTION_LIFETIME,
            MAX_ACTIVE_TRANSACTIONS,
            TRANSACTION_TRIM_THRESHOLD,
        ))
    }

    /// Sets an optional NFS export name.
    ///
    /// - `export_name`: The desired export name without slashes.
    ///
    /// Example: Name `foo` results in the export path `/foo`.
    /// Default path is `/` if not set.
    pub fn with_export_name<S: AsRef<str>>(&mut self, export_name: S) {
        self.export_name = Arc::new(format!(
            "/{}",
            export_name
                .as_ref()
                .trim_end_matches('/')
                .trim_start_matches('/')
        ));
    }
}

#[async_trait]
impl<T: NFSFileSystem + Send + Sync + 'static> NFSTcp for NFSTcpListener<T> {
    /// Gets the true listening port. Useful if the bound port number is 0
    fn get_listen_port(&self) -> u16 {
        let addr = self.listener.local_addr().unwrap();
        addr.port()
    }

    fn get_listen_ip(&self) -> IpAddr {
        let addr = self.listener.local_addr().unwrap();
        addr.ip()
    }

    /// Sets a mount listener. A "true" signal will be sent on a mount
    /// and a "false" will be sent on an unmount
    fn set_mount_listener(&mut self, signal: mpsc::Sender<bool>) {
        self.mount_signal = Some(signal);
    }

    /// Loops forever and never returns handling all incoming connections.
    async fn handle_forever(&self) -> io::Result<()> {
        let cleaner_future = Cleaner::new(
            self.transaction_tracker.clone(),
            Duration::from_secs(10),
            Arc::clone(&self.stop_notify),
        )
        .run();
        tokio::spawn(cleaner_future);

        loop {
            let (socket, _) = self.listener.accept().await?;
            let context = RPCContext {
                local_port: self.port,
                client_addr: socket.peer_addr().unwrap().to_string(),
                auth: nfs3_types::rpc::auth_unix::default(),
                vfs: self.arcfs.clone(),
                mount_signal: self.mount_signal.clone(),
                export_name: self.export_name.clone(),
                transaction_tracker: self.transaction_tracker.clone(),
            };
            info!("Accepting connection from {}", context.client_addr);
            debug!("Accepting socket {:?} {:?}", socket, context);
            tokio::spawn(async move {
                let _ = socket.set_nodelay(true);
                let _ = process_socket(socket, context).await;
            });
        }
    }
}
