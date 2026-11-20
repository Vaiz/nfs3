use std::io;
use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, BufReader};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::context::RPCContext;
use crate::rpcwire::{SocketMessageHandler, write_fragment};
use crate::transaction_tracker::{Cleaner, TransactionTracker};
use crate::units::KIBIBYTE;
use crate::vfs::adapters::ReadOnlyAdapter;
use crate::vfs::{NfsFileSystem, NfsReadFileSystem};

/// Configures a TCP listener. Supply a filesystem before calling [`Builder::bind`].
#[must_use]
pub struct Builder<FS = ()> {
    fs: FS,
    options: ListenerOptions,
}

#[derive(Clone, Copy, Default)]
struct ListenerOptions {
    generation_number: Option<u64>,
    max_rpc_record_size: Option<NonZeroUsize>,
}

impl Builder {
    /// Creates a builder with no incoming RPC record size limit.
    pub fn new() -> Self {
        Self::default()
    }

    /// Supplies a filesystem.
    pub const fn filesystem<T: NfsFileSystem + 'static>(self, fs: T) -> Builder<T> {
        Builder {
            fs,
            options: self.options,
        }
    }

    /// Supplies a filesystem that only supports reads.
    pub const fn read_only_filesystem<RO: NfsReadFileSystem + 'static>(
        self,
        fs: RO,
    ) -> Builder<ReadOnlyAdapter<RO>> {
        self.filesystem(ReadOnlyAdapter::new(fs))
    }
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            fs: (),
            options: ListenerOptions::default(),
        }
    }
}

impl<FS> Builder<FS> {
    /// Sets the file handle generation number. Instances sharing the same filesystem
    /// can use the same generation number to keep file handles valid across instances.
    pub const fn generation_number(mut self, generation_number: u64) -> Self {
        self.options.generation_number = Some(generation_number);
        self
    }

    /// Sets the maximum total payload bytes in an incoming RPC record, excluding
    /// fragment markers. Records larger than the limit cause the connection to close.
    /// By default there is no limit.
    pub const fn max_rpc_record_size(mut self, max_rpc_record_size: NonZeroUsize) -> Self {
        self.options.max_rpc_record_size = Some(max_rpc_record_size);
        self
    }
}

impl<T: NfsFileSystem + 'static> Builder<T> {
    /// Binds to an `ip:port` address. Use `auto:port` to select a loopback address
    /// or port `0` to let the OS choose a port.
    pub async fn bind(self, addr: &str) -> io::Result<NFSTcpListener<T>> {
        NFSTcpListener::bind_inner(addr, self.fs, self.options).await
    }
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

/// processes an established socket
pub(crate) async fn process_socket<IO, T>(
    socket: IO,
    context: RPCContext<T>,
    max_rpc_record_size: Option<NonZeroUsize>,
) -> Result<(), anyhow::Error>
where
    IO: AsyncRead + AsyncWrite + Unpin,
    T: NfsFileSystem + 'static,
{
    let (reader, mut writer) = tokio::io::split(socket);
    let mut reader = BufReader::with_capacity(128 * KIBIBYTE as usize, reader);
    let (handler, mut replies) = SocketMessageHandler::new(context, max_rpc_record_size);

    // Race the whole loops so replies cannot interrupt a partially read record,
    // and a parser failure can close the socket even during a blocked write.
    let result = tokio::select! {
        biased;
        result = handler.read_forever(&mut reader) => result,
        result = async {
            while let Some(reply) = replies.recv().await {
                write_fragment(&mut writer, reply?).await?;
            }
            Ok(())
        } => result,
    };
    if let Err(e) = &result {
        debug!("Message handling closed: {e}");
    }
    result
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
    #[deprecated(note = "use tcp::Builder::new().read_only_filesystem(fs).bind(addr) instead")]
    pub async fn bind_ro(ipstr: &str, fs: RO) -> io::Result<Self> {
        Builder::new().read_only_filesystem(fs).bind(ipstr).await
    }
}

impl<T: NfsFileSystem + 'static> NFSTcpListener<T> {
    /// Create a new `NFSTcpListener`.
    ///
    /// It binds to a ipstr of the form [ip address]:port. For instance,
    /// "127.0.0.1:12000". `fs` is an instance of an implementation
    /// of [`NfsFileSystem`].
    #[deprecated(note = "use tcp::Builder::new().filesystem(fs).bind(addr) instead")]
    pub async fn bind(ipstr: &str, fs: T) -> io::Result<Self> {
        Builder::new().filesystem(fs).bind(ipstr).await
    }

    /// Create a new `NFSTcpListener` with a specific generation number.
    ///
    /// The generation number is embedded in file handles and used to detect
    /// stale handles from previous server instances. When multiple NFS server
    /// instances share the same generation number, file handles remain valid
    /// across all instances (e.g. behind a load balancer).
    #[deprecated(
        note = "use tcp::Builder::new().filesystem(fs).generation_number(number).bind(addr) \
                instead"
    )]
    pub async fn bind_with_generation(
        ipstr: &str,
        fs: T,
        generation_number: u64,
    ) -> io::Result<Self> {
        Builder::new()
            .filesystem(fs)
            .generation_number(generation_number)
            .bind(ipstr)
            .await
    }

    async fn bind_inner(ipstr: &str, fs: T, options: ListenerOptions) -> io::Result<Self> {
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

        let arcfs: Arc<T> = Arc::new(fs);

        if ip == "auto" {
            let mut num_tries_left = 32;

            for try_ip in 1u16.. {
                let ip = generate_host_ip(try_ip);

                let result = Self::bind_internal(&ip, port, Arc::clone(&arcfs), options).await;

                match result {
                    Err(_) => {
                        if num_tries_left == 0 {
                            return result;
                        }
                        num_tries_left -= 1;
                    }
                    Ok(_) => {
                        return result;
                    }
                }
            }
            unreachable!(); // Does not detect automatically that loop above never terminates.
        } else {
            // Otherwise, try this.
            Self::bind_internal(ip, port, arcfs, options).await
        }
    }

    async fn bind_internal(
        ip: &str,
        port: u16,
        arcfs: Arc<T>,
        options: ListenerOptions,
    ) -> io::Result<Self> {
        let ipstr = format!("{ip}:{port}");
        let listener = TcpListener::bind(&ipstr).await?;
        info!("Listening on {:?}", &ipstr);

        let port = match listener.local_addr().expect("failed to get local address") {
            SocketAddr::V4(s) => s.port(),
            SocketAddr::V6(s) => s.port(),
        };

        let file_handle_converter = options.generation_number.map_or_else(
            crate::vfs::handle::FileHandleConverter::new,
            crate::vfs::handle::FileHandleConverter::with_generation_number,
        );

        Ok(Self {
            listener,
            port,
            arcfs,
            mount_signal: None,
            export_name: Arc::from("/".to_string()),
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

#[cfg(all(test, feature = "memfs", feature = "__test_reexports"))]
mod tests {
    use nfs3_types::portmap;
    use nfs3_types::rpc::{
        RPC_VERSION_2, call_body, fragment_header, msg_body, opaque_auth, rpc_msg,
    };
    use nfs3_types::xdr_codec::Pack;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;
    use crate::memfs::{MemFs, MemFsConfig};

    #[tokio::test]
    async fn truncated_record_closes_connection_during_pending_reply() {
        let fs = MemFs::new(MemFsConfig::default()).expect("in-memory filesystem");
        let context = RPCContext::test_ctx("/", Arc::new(fs));
        let (mut client, server) = tokio::io::duplex(8);
        let mut call = Vec::new();
        rpc_msg {
            xid: 1,
            body: msg_body::CALL(call_body {
                rpcvers: RPC_VERSION_2,
                prog: portmap::PROGRAM,
                vers: portmap::VERSION,
                proc: portmap::PMAP_PROG::PMAPPROC_NULL as u32,
                cred: opaque_auth::default(),
                verf: opaque_auth::default(),
            }),
        }
        .pack(&mut call)
        .expect("encode NULL call");
        let marker = fragment_header::new(u32::try_from(call.len()).expect("small call"), true)
            .into_xdr_buf();

        let client_io = async {
            client.write_all(&marker).await.expect("send marker");
            client.write_all(&call).await.expect("send NULL call");
            client
                .write_all(&marker[..2])
                .await
                .expect("start next marker");
            // Read only one byte, so the reply still cannot fit in the duplex buffer.
            client.read_u8().await.expect("reply started");
            client.shutdown().await.expect("finish writing");
        };
        let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
            tokio::join!(process_socket(server, context, None), client_io)
        })
        .await
        .expect("parser must not wait for the reply to finish");
        let err = result.expect_err("partial marker is a parser failure");
        assert_eq!(
            err.downcast_ref::<io::Error>().map(io::Error::kind),
            Some(io::ErrorKind::UnexpectedEof)
        );
    }
}
