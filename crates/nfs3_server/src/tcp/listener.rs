use std::io;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use super::{NFSTcp, NFSTcpListener, process_socket};
use crate::context::RPCContext;
use crate::transaction_tracker::Cleaner;
use crate::vfs::NfsFileSystem;

enum AcceptAction {
    RetryImmediately,
    RetryWithBackoff,
    Propagate,
}

fn classify_accept_error(err: &io::Error) -> AcceptAction {
    if matches!(
        err.kind(),
        io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::Interrupted
    ) {
        return AcceptAction::RetryImmediately;
    }

    // Linux accept(2) can return pending network errors from the new socket.
    #[cfg(target_os = "linux")]
    if matches!(
        err.raw_os_error(),
        Some(
            libc::ENETDOWN
                | libc::EPROTO
                | libc::ENOPROTOOPT
                | libc::EHOSTDOWN
                | libc::ENONET
                | libc::EHOSTUNREACH
                | libc::EOPNOTSUPP
                | libc::ENETUNREACH
        )
    ) {
        return AcceptAction::RetryImmediately;
    }

    #[cfg(unix)]
    if matches!(
        err.raw_os_error(),
        Some(libc::EMFILE | libc::ENFILE | libc::ENOBUFS)
    ) {
        return AcceptAction::RetryWithBackoff;
    }

    #[cfg(windows)]
    {
        use windows_sys::Win32::Networking::WinSock::{WSAEMFILE, WSAENOBUFS};
        if matches!(err.raw_os_error(), Some(WSAEMFILE | WSAENOBUFS)) {
            return AcceptAction::RetryWithBackoff;
        }
    }
    AcceptAction::Propagate
}

pub(crate) async fn accept_with_retry<T, F: Future<Output = io::Result<T>>>(
    mut accept: impl FnMut() -> F,
) -> io::Result<T> {
    let mut delay = Duration::from_millis(10);
    for _ in 1..16 {
        let err = match accept().await {
            Ok(connection) => return Ok(connection),
            Err(err) => err,
        };
        match classify_accept_error(&err) {
            AcceptAction::RetryImmediately => {}
            AcceptAction::RetryWithBackoff => {
                warn!(%err, delay_ms = delay.as_millis(), "TCP accept failed; retrying");
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(Duration::from_secs(1));
            }
            AcceptAction::Propagate => return Err(err),
        }
    }
    accept().await
}

impl<T: NfsFileSystem + 'static> NFSTcp for NFSTcpListener<T> {
    /// Gets the true listening port. Useful if the bound port number is 0
    fn get_listen_port(&self) -> u16 {
        self.local_addr.port()
    }

    fn get_listen_ip(&self) -> IpAddr {
        self.local_addr.ip()
    }

    /// Sets a mount listener. A "true" signal will be sent on a mount
    /// and a "false" will be sent on an unmount
    fn set_mount_listener(&mut self, signal: mpsc::Sender<bool>) {
        self.mount_signal = Some(signal);
    }

    async fn handle_forever(&self) -> io::Result<()> {
        let cleaner_future = Cleaner::new(
            Arc::clone(&self.transaction_tracker),
            Duration::from_secs(10),
            Arc::clone(&self.stop_notify),
        )
        .run();
        tokio::spawn(cleaner_future);

        loop {
            let (socket, peer_addr) = accept_with_retry(|| self.listener.accept()).await?;
            let context = RPCContext {
                local_port: self.local_addr.port(),
                client_addr: peer_addr.to_string(),
                vfs: Arc::clone(&self.arcfs),
                mount_signal: self.mount_signal.clone(),
                export_path: self.export_path.clone(),
                hide_exports: self.hide_exports,
                exact_export_match: self.exact_export_match,
                transaction_tracker: Arc::clone(&self.transaction_tracker),
                read_limit: self.read_limit,
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
