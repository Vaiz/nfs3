mod builder;
mod socket;
#[cfg(all(test, feature = "memfs", feature = "__test_reexports"))]
mod tests;

use std::io;
use std::net::IpAddr;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

pub use builder::Builder;
pub(crate) use socket::process_socket;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::context::RPCContext;
use crate::transaction_tracker::{Cleaner, TransactionTracker};
use crate::vfs::adapters::ReadOnlyAdapter;
use crate::vfs::{NfsFileSystem, NfsReadFileSystem};

#[derive(Default)]
struct ListenerOptions {
    generation_number: Option<u64>,
    max_rpc_record_size: Option<NonZeroUsize>,
    export_name: Option<Arc<String>>,
}

fn normalized_export_name(name: &str) -> Arc<String> {
    Arc::new(format!("/{}", name.trim_matches('/')))
}

/// A NFS Tcp Connection Handler
pub struct NFSTcpListener<T: NfsFileSystem + 'static> {
    listener: TcpListener,
    port: u16,
    arcfs: Arc<T>,
    mount_signal: Option<mpsc::Sender<bool>>,
    export_name: Arc<String>,
    transaction_tracker: Arc<TransactionTracker>,
    file_handle_converter: crate::vfs::handle::FileHandleConverter,
    stop_notify: Arc<tokio::sync::Notify>,
    max_rpc_record_size: Option<NonZeroUsize>,
}

impl<T: NfsFileSystem + 'static> Drop for NFSTcpListener<T> {
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

pub trait NFSTcp: Send + Sync {
    /// Gets the true listening port. Useful if the bound port number is 0
    fn get_listen_port(&self) -> u16;

    /// Gets the true listening IP. Useful on windows when the IP may be random
    fn get_listen_ip(&self) -> IpAddr;

    /// Sets a mount listener. A "true" signal will be sent on a mount
    /// and a "false" will be sent on an unmount
    fn set_mount_listener(&mut self, signal: mpsc::Sender<bool>);

    /// Loops forever and never returns handling all incoming connections.
    fn handle_forever(&self) -> impl Future<Output = io::Result<()>> + Send;
}

impl<RO> NFSTcpListener<ReadOnlyAdapter<RO>>
where
    RO: NfsReadFileSystem + 'static,
{
    /// Create a new `NFSTcpListener` with a read-only file system.
    ///
    /// It binds to a ipstr of the form [ip address]:port. For instance,
    /// "127.0.0.1:12000". `fs` is an instance of an implementation
    /// of [`NfsReadFileSystem`].
    ///
    /// The legacy `auto:port` mode is deprecated but still accepted here.
    #[deprecated(
        note = "use tcp::Builder::new().read_only_filesystem(fs).bind(socket_addr); auto:port is \
                deprecated"
    )]
    pub async fn bind_ro(ipstr: &str, fs: RO) -> io::Result<Self> {
        Self::bind_legacy(ipstr, ReadOnlyAdapter::new(fs), ListenerOptions::default()).await
    }
}

impl<T: NfsFileSystem + 'static> NFSTcpListener<T> {
    /// Create a new `NFSTcpListener`.
    ///
    /// It binds to a ipstr of the form [ip address]:port. For instance,
    /// "127.0.0.1:12000". `fs` is an instance of an implementation
    /// of [`NfsFileSystem`].
    ///
    /// The legacy `auto:port` mode is deprecated but still accepted here.
    #[deprecated(
        note = "use tcp::Builder::new().filesystem(fs).bind(socket_addr); auto:port is deprecated"
    )]
    pub async fn bind(ipstr: &str, fs: T) -> io::Result<Self> {
        Self::bind_legacy(ipstr, fs, ListenerOptions::default()).await
    }

    /// Create a new `NFSTcpListener` with a specific generation number.
    ///
    /// The generation number is embedded in file handles and used to detect
    /// stale handles from previous server instances. When multiple NFS server
    /// instances share the same generation number, file handles remain valid
    /// across all instances (e.g. behind a load balancer).
    ///
    /// The legacy `auto:port` mode is deprecated but still accepted here.
    #[deprecated(
        note = "use tcp::Builder::new().filesystem(fs).generation_number(number).\
                bind(socket_addr); auto:port is deprecated"
    )]
    pub async fn bind_with_generation(
        ipstr: &str,
        fs: T,
        generation_number: u64,
    ) -> io::Result<Self> {
        Self::bind_legacy(
            ipstr,
            fs,
            ListenerOptions {
                generation_number: Some(generation_number),
                ..ListenerOptions::default()
            },
        )
        .await
    }

    async fn bind_legacy(ipstr: &str, fs: T, options: ListenerOptions) -> io::Result<Self> {
        const LAST_HOST: u16 = 33;

        let (ip, port) = ipstr.split_once(':').ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::AddrNotAvailable,
                "IP Address must be of form ip:port",
            )
        })?;
        let port = port.parse::<u16>().map_err(|_parse_error| {
            io::Error::new(
                io::ErrorKind::AddrNotAvailable,
                "Port not in range 0..=65535",
            )
        })?;

        if ip != "auto" {
            let listener = TcpListener::bind((ip, port)).await?;
            return Self::from_listener(listener, fs, options);
        }

        for host in 1..=LAST_HOST {
            let ip = generate_host_ip(host);
            match TcpListener::bind((ip.as_str(), port)).await {
                Ok(listener) => return Self::from_listener(listener, fs, options),
                Err(err) if host == LAST_HOST => return Err(err),
                Err(_) => {}
            }
        }
        unreachable!("the final bind attempt always returns");
    }

    fn from_listener(listener: TcpListener, fs: T, options: ListenerOptions) -> io::Result<Self> {
        let addr = listener.local_addr()?;
        info!("Listening on {addr}");

        let file_handle_converter = options.generation_number.map_or_else(
            crate::vfs::handle::FileHandleConverter::new,
            crate::vfs::handle::FileHandleConverter::with_generation_number,
        );

        Ok(Self {
            listener,
            port: addr.port(),
            arcfs: Arc::new(fs),
            mount_signal: None,
            export_name: options.export_name.unwrap_or_else(|| Arc::new("/".into())),
            transaction_tracker: Self::new_transaction_tracker(),
            stop_notify: Arc::new(tokio::sync::Notify::new()),
            file_handle_converter,
            max_rpc_record_size: options.max_rpc_record_size,
        })
    }

    fn new_transaction_tracker() -> Arc<TransactionTracker> {
        const TRANSACTION_LIFETIME: Duration = Duration::from_mins(1);
        const MAX_ACTIVE_TRANSACTIONS: u16 = 256;
        const TRANSACTION_TRIM_THRESHOLD: usize = 2048;

        Arc::new(TransactionTracker::new(
            TRANSACTION_LIFETIME,
            MAX_ACTIVE_TRANSACTIONS,
            TRANSACTION_TRIM_THRESHOLD,
        ))
    }

    /// Sets the NFS export path. Prefer [`Builder::export_name`] before binding.
    #[deprecated(note = "use tcp::Builder::export_name before binding instead")]
    pub fn with_export_name<S: AsRef<str>>(&mut self, export_name: S) {
        self.export_name = normalized_export_name(export_name.as_ref());
    }
}

impl<T: NfsFileSystem + 'static> NFSTcp for NFSTcpListener<T> {
    /// Gets the true listening port. Useful if the bound port number is 0
    fn get_listen_port(&self) -> u16 {
        let addr = self
            .listener
            .local_addr()
            .expect("failed to get local address");
        addr.port()
    }

    fn get_listen_ip(&self) -> IpAddr {
        let addr = self
            .listener
            .local_addr()
            .expect("failed to get local address");
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
            Arc::clone(&self.transaction_tracker),
            Duration::from_secs(10),
            Arc::clone(&self.stop_notify),
        )
        .run();
        tokio::spawn(cleaner_future);

        loop {
            let (socket, _) = self.listener.accept().await?;
            let context = RPCContext {
                local_port: self.port,
                client_addr: socket
                    .peer_addr()
                    .expect("failed to get peer address")
                    .to_string(),
                auth: nfs3_types::rpc::auth_unix::default(),
                vfs: Arc::clone(&self.arcfs),
                mount_signal: self.mount_signal.clone(),
                export_name: Arc::clone(&self.export_name),
                transaction_tracker: Arc::clone(&self.transaction_tracker),
                file_handle_converter: self.file_handle_converter,
            };
            info!("Accepting connection from {}", context.client_addr);
            debug!("Accepting socket {:?} {:?}", socket, context);
            let max_rpc_record_size = self.max_rpc_record_size;
            tokio::spawn(async move {
                let _ = socket.set_nodelay(true);
                let _ = process_socket(socket, context, max_rpc_record_size).await;
            });
        }
    }
}
