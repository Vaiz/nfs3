//! Opt-in finite listener. XIDs correlate replies only; repeated records may
//! execute again. Lost mutation replies are uncertain, not retry-safe failures.

mod dispatch;
#[cfg(all(test, feature = "memfs"))]
mod tests;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use nfs3_types::async_io::{self, AsyncRead, AsyncWrite, RecordLimits};
use nfs3_types::xdr_codec::{BackingBudget, DecodeLimits, EncodeLimits};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Semaphore, oneshot, watch};
use tokio::time::timeout;

use crate::context::RPCContext;
use crate::transaction_tracker::TransactionTracker;
use crate::vfs::NfsOwnedIo;
use crate::vfs::handle::FileHandleConverter;
use crate::vfs::owned::{Lease, WorkLease};

/// Server-owned selected directory metadata. Backend iterator caches and entries
/// before transfer remain a separate backend allocation domain.
#[derive(Debug, Copy, Clone)]
pub struct DirectoryLimits {
    pub max_wire_bytes: u32,
    pub max_entries: usize,
    pub max_name_bytes: usize,
    pub max_metadata_bytes: usize,
}

impl DirectoryLimits {
    /// Maximum metadata-only directory reply, including the 24-byte RPC
    /// header, status, full attributes, verifier, list terminator and EOF.
    /// READDIRPLUS dominates READDIR: each entry adds at most 184 bytes plus
    /// padded name (full attributes and a maximum 64-byte wire handle).
    #[must_use]
    pub fn reply_metadata_bound(self) -> Option<usize> {
        let entries = self
            .max_entries
            .min(self.max_wire_bytes as usize / 24)
            .min(self.max_metadata_bytes / 64);
        let names = entries.checked_mul(self.max_name_bytes)?.min(
            self.max_metadata_bytes
                .checked_sub(entries.checked_mul(64)?)?,
        );
        let padded_name = self.max_name_bytes.checked_add(3)? & !3;
        let padded_names = names
            .checked_add(entries.checked_mul(3)?)?
            .min(entries.checked_mul(padded_name)?);
        let host_bound = 132_usize
            .checked_add(entries.checked_mul(184)?)?
            .checked_add(padded_names)?;
        // Existing handlers cap the page's wire contribution; READDIR's
        // counting excludes up to the RPC header. Error/empty replies still
        // require their fixed envelope, even for an impossibly small page.
        Some(
            host_bound
                .min((self.max_wire_bytes as usize).checked_add(24)?)
                .max(132),
        )
    }

    pub(crate) fn admit_name(
        self,
        name: &nfs3_types::nfs3::filename3<'_>,
        entries: &mut usize,
        metadata: &mut usize,
    ) -> bool {
        let capacity = match &name.0.0 {
            std::borrow::Cow::Owned(v) => v.capacity(),
            std::borrow::Cow::Borrowed(v) => v.len(),
        };
        let Some(next) = metadata
            .checked_add(capacity)
            .and_then(|n| n.checked_add(64))
        else {
            return false;
        };
        if *entries >= self.max_entries
            || name.0.len() > self.max_name_bytes
            || name.0.len() > self.max_wire_bytes as usize
            || next > self.max_metadata_bytes
        {
            return false;
        }
        *entries += 1;
        *metadata = next;
        true
    }

    fn envelope(self) -> Option<usize> {
        // Minimum wire entry is 24 bytes. Geometric Vec growth includes its
        // initial allocation, cumulative work and conservative old+new overlap.
        let entries = self
            .max_entries
            .min(self.max_wire_bytes as usize / 24)
            .max(4);
        entries
            .checked_mul(8)?
            .checked_mul(std::mem::size_of::<nfs3_types::nfs3::entryplus3<'static>>())?
            .checked_add(self.max_metadata_bytes)
    }
}

/// No numeric defaults: every policy is chosen by the caller.
#[derive(Debug, Copy, Clone)]
pub struct ServerLimits {
    pub record: RecordLimits,
    pub decode: DecodeLimits,
    pub encode: EncodeLimits,
    pub max_read_bytes: u32,
    pub max_write_bytes: u32,
    pub directory: DirectoryLimits,
    pub max_connections: u32,
    pub max_operations: u32,
    pub backing_bytes: usize,
    /// Additional known control entitlement, retained until work/reply drop.
    /// Runtime task/allocator overhead is not an exact process-memory bound.
    pub control_bytes: usize,
    pub idle_timeout: Duration,
    pub progress_timeout: Duration,
    pub receive_timeout: Duration,
    pub send_timeout: Duration,
}

impl ServerLimits {
    /// Derived fixed RPC/auth/error/read-owner control floor, not a default
    /// policy. Callers must still supply all limits and the global domain size.
    #[must_use]
    pub fn minimum_control_bytes() -> usize {
        dispatch::control_size()
    }

    #[must_use]
    pub fn operation_envelope(self) -> Option<usize> {
        self.record
            .max_record_bytes
            .checked_add(self.max_read_bytes as usize)?
            .checked_add(self.record.max_builder_reservation_bytes)?
            .checked_add(self.decode.max_reservation_bytes)?
            .checked_add(self.decode.max_metadata_bytes)?
            .checked_add(self.encode.max_reservation_bytes)?
            .checked_add(self.encode.max_metadata_bytes)?
            .checked_add(self.directory.envelope()?)?
            .checked_add(self.control_bytes)
    }

    pub fn validate(self) -> io::Result<Self> {
        let write = (self.max_write_bytes as usize)
            .checked_add(3)
            .map(|n| n & !3)
            .and_then(|n| n.checked_add(928));
        let read = (self.max_read_bytes as usize)
            .checked_add(3)
            .map(|n| n & !3)
            .and_then(|n| n.checked_add(128));
        if !self.record.validate()
            || self.max_connections == 0
            || self.max_operations == 0
            || self.max_connections as usize > Semaphore::MAX_PERMITS
            || self.max_operations as usize > Semaphore::MAX_PERMITS
            || self.max_read_bytes == 0
            || self.max_write_bytes == 0
            || write.is_none_or(|n| n > self.record.max_record_bytes)
            || read.is_none_or(|n| n > self.encode.max_wire_bytes)
            || self.encode.max_wire_bytes > self.record.max_record_bytes
            || self.encode.max_metadata_bytes < 32
            || self.encode.max_chunks == 0
            || self.encode.max_depth == 0
            || self.encode.max_reservation_bytes == 0
            || self.encode.max_retained_capacity < self.max_read_bytes as usize
            || self.decode.max_depth == 0
            || self.decode.max_reservation_bytes == 0
            || self.decode.payload.max_logical_bytes < self.max_write_bytes as usize
            || self.decode.payload.max_backing_capacity < self.record.max_record_bytes
            || self.decode.payload.max_segments
                < self
                    .record
                    .max_record_bytes
                    .div_ceil(self.record.block_bytes)
            || self.directory.max_entries == 0
            || self.directory.max_name_bytes == 0
            || self.directory.max_wire_bytes == 0
            || self.directory.max_name_bytes > self.decode.max_metadata_bytes
            || (self.directory.max_wire_bytes as usize)
                .checked_add(24)
                .is_none_or(|n| n > self.encode.max_wire_bytes)
            || self.directory.max_metadata_bytes < self.directory.max_name_bytes
            || self.control_bytes < Self::minimum_control_bytes()
            || [
                self.idle_timeout,
                self.progress_timeout,
                self.receive_timeout,
                self.send_timeout,
            ]
            .iter()
            .any(|d| d.is_zero() || std::time::Instant::now().checked_add(*d).is_none())
            || self
                .operation_envelope()
                .is_none_or(|n| n > self.backing_bytes)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid bounded server envelope",
            ));
        }
        if !dispatch::bulk_reply_limits_fit(&self) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "codec limits cannot encode supported bulk replies",
            ));
        }
        if !dispatch::directory_reply_limits_fit(&self) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "codec limits cannot encode supported directory replies",
            ));
        }
        Ok(self)
    }
}

struct Shared<T: NfsOwnedIo> {
    vfs: Arc<T>,
    limits: ServerLimits,
    budget: BackingBudget,
    connections: Arc<Semaphore>,
    operations: Arc<Semaphore>,
    stop: watch::Sender<bool>,
    converter: FileHandleConverter,
    export: Arc<String>,
    tracker: Arc<TransactionTracker>,
    port: u16,
}

/// Drop signals readers, but never aborts admitted operations.
pub struct BoundedListener<T: NfsOwnedIo> {
    listener: TcpListener,
    shared: Arc<Shared<T>>,
}

impl<T: NfsOwnedIo + 'static> BoundedListener<T> {
    pub async fn bind(
        address: &str,
        vfs: T,
        export: String,
        limits: ServerLimits,
    ) -> io::Result<Self> {
        let limits = limits.validate()?;
        if export.len() > limits.decode.max_metadata_bytes {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let listener = TcpListener::bind(address).await?;
        let port = listener.local_addr()?.port();
        let (stop, _) = watch::channel(false);
        Ok(Self {
            listener,
            shared: Arc::new(Shared {
                vfs: Arc::new(vfs),
                limits,
                budget: BackingBudget::new(limits.backing_bytes),
                connections: Arc::new(Semaphore::new(limits.max_connections as usize)),
                operations: Arc::new(Semaphore::new(limits.max_operations as usize)),
                stop,
                converter: FileHandleConverter::new(),
                export: Arc::new(export),
                // Compatibility only: no bounded-path tracker calls or cleaner.
                tracker: Arc::new(TransactionTracker::new(Duration::from_secs(1), 0, 0)),
                port,
            }),
        })
    }
    pub fn local_addr(&self) -> io::Result<std::net::SocketAddr> {
        self.listener.local_addr()
    }
    pub fn backing_used(&self) -> usize {
        self.shared.budget.used()
    }
    pub fn active_operations(&self) -> usize {
        self.shared.limits.max_operations as usize - self.shared.operations.available_permits()
    }

    /// Accept one socket. Exhaustion closes without reading even its marker.
    pub async fn accept(&self) -> io::Result<Option<tokio::task::JoinHandle<()>>> {
        let mut stop = self.shared.stop.subscribe();
        if *stop.borrow_and_update() {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let (socket, _) = tokio::select! {
            biased;
            _ = stop.changed() => return Err(io::ErrorKind::Interrupted.into()),
            result = self.listener.accept() => result?,
        };
        if *stop.borrow_and_update() {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let Ok(slot) = Arc::clone(&self.shared.connections).try_acquire_owned() else {
            return Ok(None);
        };
        let shared = Arc::clone(&self.shared);
        let task = tokio::spawn(async move {
            let _slot = slot;
            connection(socket, shared).await;
        });
        Ok(Some(task))
    }

    pub async fn run(&self) -> io::Result<()> {
        loop {
            self.accept().await?;
        }
    }

    /// Stop admissions/readers and wait for actual jobs and replies. An
    /// uncooperative backend can keep this pending indefinitely.
    pub async fn drain(&self) {
        self.shared.stop.send_replace(true);
        let _connections = self
            .shared
            .connections
            .acquire_many(self.shared.limits.max_connections)
            .await;
        let _operations = self
            .shared
            .operations
            .acquire_many(self.shared.limits.max_operations)
            .await;
    }
}

impl<T: NfsOwnedIo> Drop for BoundedListener<T> {
    fn drop(&mut self) {
        self.shared.stop.send_replace(true);
    }
}

impl<T: crate::vfs::NfsOwnedRead + 'static>
    BoundedListener<crate::vfs::adapters::ReadOnlyAdapter<T>>
{
    pub async fn bind_ro(
        address: &str,
        vfs: T,
        export: String,
        limits: ServerLimits,
    ) -> io::Result<Self> {
        Self::bind(
            address,
            crate::vfs::adapters::ReadOnlyAdapter::new(vfs),
            export,
            limits,
        )
        .await
    }
}

struct Socket<S> {
    stream: S,
    progress: Duration,
    first_idle: Option<Duration>,
    receive_started: Option<tokio::time::Instant>,
    receive_timeout: Duration,
}

impl<S: tokio::io::AsyncRead + Unpin + Send> AsyncRead for Socket<S> {
    async fn async_read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut deadline = self.first_idle.take().unwrap_or(self.progress);
        if let Some(started) = self.receive_started {
            deadline = deadline.min(self.receive_timeout.saturating_sub(started.elapsed()));
        }
        let count = timeout(
            deadline,
            tokio::io::AsyncReadExt::read(&mut self.stream, buf),
        )
        .await
        .map_err(|_| io::ErrorKind::TimedOut)??;
        if count != 0 && self.receive_started.is_none() {
            self.receive_started = Some(tokio::time::Instant::now());
        }
        Ok(count)
    }
}
impl<S: tokio::io::AsyncWrite + Unpin + Send> AsyncWrite for Socket<S> {
    async fn async_write(&mut self, buf: &[u8]) -> io::Result<usize> {
        timeout(
            self.progress,
            tokio::io::AsyncWriteExt::write(&mut self.stream, buf),
        )
        .await
        .map_err(|_| io::ErrorKind::TimedOut)?
    }
    async fn async_write_vectored(&mut self, buf: &[io::IoSlice<'_>]) -> io::Result<usize> {
        timeout(
            self.progress,
            tokio::io::AsyncWriteExt::write_vectored(&mut self.stream, buf),
        )
        .await
        .map_err(|_| io::ErrorKind::TimedOut)?
    }
}

async fn connection<T: NfsOwnedIo + 'static>(stream: TcpStream, shared: Arc<Shared<T>>) {
    let peer = stream
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_default();
    connection_io(stream, shared, peer).await;
}

// The same receive/dispatch/send loop accepts controlled IO in deadline tests.
async fn connection_io<
    T: NfsOwnedIo + 'static,
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send,
>(
    stream: S,
    shared: Arc<Shared<T>>,
    peer: String,
) {
    let limits = shared.limits;
    let mut socket = Socket {
        stream,
        progress: limits.progress_timeout,
        first_idle: None,
        receive_started: None,
        receive_timeout: limits.receive_timeout,
    };
    let mut stop = shared.stop.subscribe();
    loop {
        if *stop.borrow() {
            return;
        }
        socket.first_idle = Some(limits.idle_timeout);
        socket.receive_started = None;
        let marker = tokio::select! {
            marker = async_io::first_marker(&mut socket) => match marker { Ok(Some(m)) => m, _ => return },
            _ = stop.changed() => return,
        };
        let Ok(slot) = Arc::clone(&shared.operations).try_acquire_owned() else {
            return;
        };
        let Some(envelope) = limits.operation_envelope() else {
            return;
        };
        let Ok(mut reservation) = shared.budget.try_reserve(envelope) else {
            return;
        };
        let remaining_receive = socket
            .receive_started
            .map_or(limits.receive_timeout, |start| {
                limits.receive_timeout.saturating_sub(start.elapsed())
            });
        let record = tokio::select! {
            result = timeout(remaining_receive, async_io::receive_with_marker(&mut socket, marker, limits.record, &mut reservation)) => match result { Ok(Ok(r)) => r, _ => return },
            _ = stop.changed() => return,
        };
        // All RPC/auth/argument decoding and exact finish happens synchronously.
        let Some(decoded) = dispatch::decode(record, &limits) else {
            return;
        };
        let authorized =
            !decoded.check_authorization || shared.vfs.authorize(decoded.auth.as_ref());
        let context = RPCContext {
            local_port: shared.port,
            client_addr: peer.clone(),
            auth: decoded.auth.unwrap_or_default(),
            vfs: Arc::clone(&shared.vfs),
            mount_signal: None,
            export_name: Arc::clone(&shared.export),
            transaction_tracker: Arc::clone(&shared.tracker),
            file_handle_converter: shared.converter,
            directory_limits: Some(limits.directory),
            reply_metadata_limit: Some(limits.encode.max_metadata_bytes),
        };
        let read_buffer = if let Some(count) = decoded.work.read_count() {
            match nfs3_types::xdr_codec::ReceiveBuffer::try_new_charged(
                count.min(limits.max_read_bytes) as usize,
                &mut reservation,
            ) {
                Ok(buffer) => Some(buffer),
                Err(_) => return,
            }
        } else {
            None
        };
        let lease = Arc::new(Lease {
            _slot: slot,
            _control: reservation,
        });
        let (tx, rx) = oneshot::channel();
        let xid = decoded.xid;
        // Intentionally detached. Neither connection cancellation nor dropping
        // the JoinHandle aborts actual work or releases its lease.
        tokio::spawn(async move {
            let plan = dispatch::execute(
                context,
                xid,
                decoded.work,
                &limits,
                read_buffer,
                Arc::clone(&lease),
                authorized,
            )
            .await;
            let _ = tx.send((plan, lease));
        });
        // Shutdown closes receive boundaries, not an admitted job's outcome.
        let (plan, lease): (_, WorkLease) = match rx.await {
            Ok(reply) => reply,
            Err(_) => return,
        };
        let Some(plan) = plan else {
            return;
        };
        let sent = timeout(limits.send_timeout, async_io::send(&mut socket, &plan)).await;
        if !matches!(sent, Ok(Ok(()))) {
            return;
        }
        drop(plan);
        drop(lease);
        // All reply storage and entitlement are gone before the next marker.
    }
}
