use std::io;
use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

use anyhow;
use tokio::io::BufReader;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::context::RPCContext;
use crate::rpcwire::messages::RpcRecordReader;
use crate::rpcwire::{read_and_dispatch_loop, write_fragment};
use crate::transaction_tracker::{Cleaner, TransactionTracker};
use crate::units::KIBIBYTE;
use crate::vfs::adapters::ReadOnlyAdapter;
use crate::vfs::{NfsFileSystem, NfsReadFileSystem};

// Shared budget for running RPC calls and completed replies waiting to be written.
const REPLY_QUEUE_CAPACITY: usize = 16;

/// A NFS Tcp Connection Handler
pub struct NFSTcpListener<T: NfsFileSystem + 'static> {
    listener: TcpListener,
    port: u16,
    arcfs: Arc<T>,
    mount_signal: Option<mpsc::Sender<bool>>,
    export_name: Arc<String>,
    max_rpc_record_size: Option<NonZeroUsize>,
    transaction_tracker: Arc<TransactionTracker>,
    file_handle_converter: crate::vfs::handle::FileHandleConverter,
    stop_notify: Arc<tokio::sync::Notify>,
}

#[derive(Default)]
struct ListenerOptions {
    export_name: Option<String>,
    generation_number: Option<u64>,
    max_rpc_record_size: Option<NonZeroUsize>,
}

/// Configures an NFS TCP listener. Select a filesystem before calling [`Builder::bind`].
#[must_use]
pub struct Builder<FS = ()> {
    fs: FS,
    options: ListenerOptions,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            fs: (),
            options: ListenerOptions::default(),
        }
    }
}

impl Builder {
    /// Creates a listener builder with default options and no filesystem.
    pub fn new() -> Self {
        Self::default()
    }

    /// Selects the filesystem to export.
    pub fn filesystem<T: NfsFileSystem + 'static>(self, fs: T) -> Builder<T> {
        Builder {
            fs,
            options: self.options,
        }
    }

    /// Selects the filesystem to export with read-only access.
    pub fn read_only_filesystem<RO: NfsReadFileSystem + 'static>(
        self,
        fs: RO,
    ) -> Builder<ReadOnlyAdapter<RO>> {
        self.filesystem(ReadOnlyAdapter::new(fs))
    }
}

impl<FS> Builder<FS> {
    /// Sets the NFS export path (default: `/`).
    ///
    /// Leading and trailing slashes are stripped before adding one leading slash.
    /// For example, `data` and `/data/` both export at `/data`.
    pub fn export_name(mut self, name: impl AsRef<str>) -> Self {
        self.options.export_name = Some(normalized_export_name(name.as_ref()));
        self
    }

    /// Sets the generation number embedded in file handles.
    ///
    /// By default a generation number is generated for each listener. Servers sharing the
    /// same filesystem can use the same generation number to keep handles valid across instances.
    pub const fn generation_number(mut self, generation_number: u64) -> Self {
        self.options.generation_number = Some(generation_number);
        self
    }

    /// Limits the combined payload bytes of all fragments in an incoming RPC record.
    ///
    /// Fragment markers do not count toward the limit. The server closes the connection if
    /// a record exceeds the limit. By default there is no limit.
    pub const fn max_rpc_record_size(mut self, limit: NonZeroUsize) -> Self {
        self.options.max_rpc_record_size = Some(limit);
        self
    }
}

impl<T: NfsFileSystem + 'static> Builder<T> {
    /// Binds to an IPv4 or IPv6 socket address.
    ///
    /// Use port `0` to let the OS choose an available port. The deprecated `auto:port`
    /// syntax is only supported by the legacy string constructors.
    pub async fn bind(self, addr: SocketAddr) -> io::Result<NFSTcpListener<T>> {
        let listener = TcpListener::bind(addr).await?;
        info!("Listening on {:?}", addr);
        Ok(NFSTcpListener::from_listener(
            listener,
            self.fs,
            self.options,
        ))
    }
}

fn normalized_export_name(name: &str) -> String {
    format!("/{}", name.trim_matches('/'))
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

/// processes an established socket
pub(crate) async fn process_socket<IO, T>(
    socket: IO,
    context: RPCContext<T>,
    max_rpc_record_size: Option<NonZeroUsize>,
) -> Result<(), anyhow::Error>
where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + 'static,
    T: NfsFileSystem + 'static,
{
    let (reader, mut writer) = tokio::io::split(socket);
    let reader = BufReader::with_capacity(128 * KIBIBYTE as usize, reader);
    let reader = RpcRecordReader::new(reader, max_rpc_record_size);
    let (replies, mut pending_replies) = mpsc::channel(REPLY_QUEUE_CAPACITY);

    // Race whole loops: a reply cannot cancel a partial read, but a framing
    // error can cancel a blocked write and close the connection.
    tokio::select! {
        result = read_and_dispatch_loop(reader, context, replies) => result,
        result = async {
            while let Some(reply) = pending_replies.recv().await {
                let Ok(reply) = reply else { anyhow::bail!("Error handling RPC message") };
                write_fragment(&mut writer, reply).await?;
            }
            Ok(())
        } => result,
    }
    .inspect_err(|e| debug!("Message handling closed: {e}"))
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
    /// `auto:port` is deprecated but still supported here.
    #[deprecated(
        note = "Use Builder::new().read_only_filesystem(fs).bind(addr); auto:port is deprecated"
    )]
    pub async fn bind_ro(ipstr: &str, fs: RO) -> io::Result<Self> {
        Self::bind_inner(ipstr, ReadOnlyAdapter::new(fs), None).await
    }
}

impl<T: NfsFileSystem + 'static> NFSTcpListener<T> {
    /// Create a new `NFSTcpListener`.
    ///
    /// It binds to a ipstr of the form [ip address]:port. For instance,
    /// "127.0.0.1:12000". `fs` is an instance of an implementation
    /// of [`NfsFileSystem`].
    ///
    /// `auto:port` is deprecated but still supported here.
    #[deprecated(note = "Use Builder::new().filesystem(fs).bind(addr); auto:port is deprecated")]
    pub async fn bind(ipstr: &str, fs: T) -> io::Result<Self> {
        Self::bind_inner(ipstr, fs, None).await
    }

    /// Create a new `NFSTcpListener` with a specific generation number.
    ///
    /// The generation number is embedded in file handles and used to detect
    /// stale handles from previous server instances. When multiple NFS server
    /// instances share the same generation number, file handles remain valid
    /// across all instances (e.g. behind a load balancer).
    ///
    /// `auto:port` is deprecated but still supported here.
    #[deprecated(note = "Use Builder::generation_number before binding; auto:port is deprecated")]
    pub async fn bind_with_generation(
        ipstr: &str,
        fs: T,
        generation_number: u64,
    ) -> io::Result<Self> {
        Self::bind_inner(ipstr, fs, Some(generation_number)).await
    }

    async fn bind_inner(ipstr: &str, fs: T, generation_number: Option<u64>) -> io::Result<Self> {
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
        let options = ListenerOptions {
            generation_number,
            ..ListenerOptions::default()
        };

        let listener = if ip == "auto" {
            let mut num_tries_left = 32;

            for try_ip in 1u16.. {
                let ip = generate_host_ip(try_ip);

                match Self::bind_legacy_addr(&ip, port).await {
                    Err(err) => {
                        if num_tries_left == 0 {
                            return Err(err);
                        }
                        num_tries_left -= 1;
                    }
                    Ok(listener) => {
                        return Ok(Self::from_listener(listener, fs, options));
                    }
                }
            }
            unreachable!(); // Does not detect automatically that loop above never terminates.
        } else {
            // Otherwise, try this.
            Self::bind_legacy_addr(ip, port).await?
        };
        Ok(Self::from_listener(listener, fs, options))
    }

    async fn bind_legacy_addr(ip: &str, port: u16) -> io::Result<TcpListener> {
        let ipstr = format!("{ip}:{port}");
        let listener = TcpListener::bind(&ipstr).await?;
        info!("Listening on {:?}", &ipstr);
        Ok(listener)
    }

    fn from_listener(listener: TcpListener, fs: T, options: ListenerOptions) -> Self {
        let port = listener
            .local_addr()
            .expect("failed to get local address")
            .port();
        let file_handle_converter = options.generation_number.map_or_else(
            crate::vfs::handle::FileHandleConverter::new,
            crate::vfs::handle::FileHandleConverter::with_generation_number,
        );

        Self {
            listener,
            port,
            arcfs: Arc::new(fs),
            mount_signal: None,
            export_name: Arc::new(options.export_name.unwrap_or_else(|| "/".to_string())),
            max_rpc_record_size: options.max_rpc_record_size,
            transaction_tracker: Self::new_transaction_tracker(),
            stop_notify: Arc::new(tokio::sync::Notify::new()),
            file_handle_converter,
        }
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

    /// Sets an optional NFS export name.
    ///
    /// - `export_name`: The desired export name without slashes.
    ///
    /// Example: Name `foo` results in the export path `/foo`.
    /// Default path is `/` if not set.
    #[deprecated(note = "Use Builder::export_name before binding")]
    pub fn with_export_name<S: AsRef<str>>(&mut self, export_name: S) {
        self.export_name = Arc::new(normalized_export_name(export_name.as_ref()));
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

#[cfg(all(test, feature = "memfs"))]
mod tests {
    use std::net::Ipv4Addr;

    use super::Builder;
    use crate::memfs::{MemFs, MemFsConfig};
    use crate::vfs::{NfsFileSystem, VFSCapabilities};

    #[tokio::test]
    async fn builder_applies_options_to_read_only_listener() -> anyhow::Result<()> {
        let fs = MemFs::new(MemFsConfig::default()).expect("initialize the test filesystem");
        let listener = Builder::new()
            .generation_number(42)
            .read_only_filesystem(fs)
            .export_name("///nested/export///")
            .bind((Ipv4Addr::LOCALHOST, 0).into())
            .await?;

        assert_eq!(listener.export_name.as_str(), "/nested/export");
        assert_eq!(
            listener.file_handle_converter.verf().0,
            42_u64.to_le_bytes()
        );
        assert!(matches!(
            listener.arcfs.capabilities(),
            VFSCapabilities::ReadOnly
        ));
        Ok(())
    }
}
