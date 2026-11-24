use std::io;
use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

use anyhow;
use nfs3_types::mount::dirpath;
use nfs3_types::xdr_codec::BoundedOpaque;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::context::{RPCContext, normalized_export_path, root_read_limit};
use crate::rpcwire::messages::RpcRecordReader;
use crate::rpcwire::read_and_dispatch_loop;
use crate::transaction_tracker::TransactionTracker;
use crate::vfs::adapters::ReadOnlyAdapter;
use crate::vfs::handle::FileHandleConverter;
use crate::vfs::{NfsFileSystem, NfsReadFileSystem};

mod builder;
pub(crate) mod listener;

pub use builder::Builder;

// Shared budget for running RPC calls and completed replies waiting to be written.
const REPLY_QUEUE_CAPACITY: usize = 16;
static DEFAULT_EXPORT_PATH: dirpath<'static> = dirpath(BoundedOpaque::from_array(b"/"));

/// A NFS Tcp Connection Handler
pub struct NFSTcpListener<T: NfsFileSystem + 'static> {
    listener: TcpListener,
    local_addr: SocketAddr,
    arcfs: Arc<T>,
    mount_signal: Option<mpsc::Sender<bool>>,
    export_path: dirpath<'static>,
    max_rpc_record_size: Option<NonZeroUsize>,
    transaction_tracker: Arc<TransactionTracker>,
    read_limit: u32,
    file_handle_converter: FileHandleConverter,
    stop_notify: Arc<tokio::sync::Notify>,
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
    let reader = RpcRecordReader::new(reader, max_rpc_record_size);
    let (replies, mut pending_replies) = mpsc::channel(REPLY_QUEUE_CAPACITY);

    // Race whole loops: a reply cannot cancel a partial read, but a framing
    // error can cancel a blocked write and close the connection.
    tokio::select! {
        result = read_and_dispatch_loop(reader, context, replies) => result,
        result = async {
            while let Some(reply) = pending_replies.recv().await {
                let Ok(mut reply) = reply else { anyhow::bail!("Error handling RPC message") };
                // Vectored, so a shared READ payload is not copied.
                writer.write_all_buf(&mut reply).await?;
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

    /// Handles connections until cancelled, a fatal error occurs, or 16 consecutive accepts fail.
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
    async fn new(
        listener: TcpListener,
        fs: T,
        export_path: dirpath<'static>,
        generation_number: Option<u64>,
        max_rpc_record_size: Option<NonZeroUsize>,
    ) -> io::Result<Self> {
        let local_addr = listener.local_addr()?;
        let read_limit = root_read_limit(&fs).await?;
        let file_handle_converter = generation_number.map_or_else(
            FileHandleConverter::new,
            FileHandleConverter::with_generation_number,
        );

        Ok(Self {
            listener,
            local_addr,
            arcfs: Arc::new(fs),
            mount_signal: None,
            export_path,
            max_rpc_record_size,
            transaction_tracker: Self::new_transaction_tracker(),
            read_limit,
            stop_notify: Arc::new(tokio::sync::Notify::new()),
            file_handle_converter,
        })
    }

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
                        return Self::new(
                            listener,
                            fs,
                            DEFAULT_EXPORT_PATH.clone(),
                            generation_number,
                            None,
                        )
                        .await;
                    }
                }
            }
            unreachable!(); // Does not detect automatically that loop above never terminates.
        } else {
            // Otherwise, try this.
            Self::bind_legacy_addr(ip, port).await?
        };
        Self::new(
            listener,
            fs,
            DEFAULT_EXPORT_PATH.clone(),
            generation_number,
            None,
        )
        .await
    }

    async fn bind_legacy_addr(ip: &str, port: u16) -> io::Result<TcpListener> {
        let ipstr = format!("{ip}:{port}");
        let listener = TcpListener::bind(&ipstr).await?;
        info!("Listening on {ipstr}");
        Ok(listener)
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
    ///
    /// # Panics
    ///
    /// Panics if the normalized path exceeds 1,024 bytes.
    #[deprecated(note = "Use Builder::export_name before binding")]
    pub fn with_export_name<S: AsRef<str>>(&mut self, export_name: S) {
        self.export_path =
            normalized_export_path(export_name.as_ref()).unwrap_or_else(|err| panic!("{err}"));
    }
}
