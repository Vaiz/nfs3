use std::io::{self, IoSlice};
use std::time::Instant;

use call::{RpcCall, RpcCallOutcome};
use messages::RpcRecordReader;
use nfs3_types::rpc::{RPC_VERSION_2, accept_stat_data, auth_stat, call_body};
use nfs3_types::xdr_codec::{Decoder, Encoded, Pack, Unpack};
use nfs3_types::{nfs3 as nfs, portmap};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;
use tracing::{error, info, trace, warn};

use crate::context::RPCContext;
use crate::transaction_tracker::{self, TransactionError, TransactionLock};
use crate::vfs::NfsFileSystem;
use crate::{mount_handlers, nfs_handlers, portmap_handlers};

pub(crate) mod call;
pub(crate) mod messages;

// Information from RFC 5531
// https://datatracker.ietf.org/doc/html/rfc5531

const NFS_ACL_PROGRAM: u32 = 100_227;
const NFS_ID_MAP_PROGRAM: u32 = 100_270;
const NFS_METADATA_PROGRAM: u32 = 200_024;

async fn handle_rpc_message<T>(
    mut context: RPCContext<T>,
    message: Decoder,
) -> anyhow::Result<RpcCallOutcome>
where
    T: NfsFileSystem,
{
    let message = RpcCall::try_from(message)?;
    let xid = message.xid();
    let call = message.body();
    let prog = call.prog;

    if call.rpcvers != RPC_VERSION_2 {
        warn!("Invalid RPC version {} != {RPC_VERSION_2}", call.rpcvers);
        return message.into_rpc_mismatch();
    }

    let unix = context.set_auth(call)?;
    if prog != portmap::PROGRAM && !context.vfs.authorize(unix.then_some(&context.auth)) {
        warn!(
            "refusing a call from uid {} on {}",
            context.auth.uid, context.client_addr
        );
        return message.into_auth_error(auth_stat::AUTH_TOOWEAK);
    }

    let transaction = lock_transaction(
        &context.transaction_tracker,
        &context.client_addr,
        xid,
        call,
    );
    if let Err(msg) = transaction {
        match msg {
            Some(err) => return message.into_error_reply(err),
            None => {
                // This is a retransmission, so we don't need to do anything
                return Ok(RpcCallOutcome::NoReply);
            }
        }
    }

    match prog {
        portmap::PROGRAM => portmap_handlers::handle_portmap(context, message).await,
        nfs3_types::mount::PROGRAM => mount_handlers::handle_mount(context, message).await,
        nfs::PROGRAM => nfs_handlers::handle_nfs(context, message).await,
        NFS_ACL_PROGRAM | NFS_ID_MAP_PROGRAM | NFS_METADATA_PROGRAM => {
            trace!("ignoring NFS_ACL packet");
            message.into_error_reply(accept_stat_data::PROG_UNAVAIL)
        }
        _ => {
            warn!("Unknown RPC Program number {prog} != {}", nfs::PROGRAM);
            message.into_error_reply(accept_stat_data::PROG_UNAVAIL)
        }
    }
}

/// Handles the RPC message and returns a result. The handler is an async function
pub(crate) async fn handle<I, O, T>(
    context: RPCContext<T>,
    mut message: RpcCall,
    handler: impl AsyncFnOnce(RPCContext<T>, u32, I) -> O,
) -> anyhow::Result<RpcCallOutcome>
where
    I: Unpack,
    O: Pack + Send + 'static,
    T: NfsFileSystem,
{
    let args = match message.unpack_args::<I>() {
        Ok(args) => args,
        Err(err) => {
            error!("Failed to unpack message: {err}");
            return message.into_error_reply(accept_stat_data::GARBAGE_ARGS);
        }
    };

    let result = handler(context, message.xid(), args).await;
    message.into_success_reply(&result)
}

fn lock_transaction(
    transaction_tracker: &transaction_tracker::TransactionTracker,
    client_addr: &str,
    xid: u32,
    call: &call_body<'_>,
) -> Result<TransactionLock, Option<accept_stat_data>> {
    let transaction = transaction_tracker.start_transaction(client_addr, xid, Instant::now());

    match transaction {
        Ok(lock) => Ok(lock),
        Err(TransactionError::AlreadyExists) => {
            info!(
                "Retransmission detected, xid: {xid}, client_addr: {client_addr}, call: {call:?}",
            );
            Err(None)
        }
        Err(TransactionError::TooManyRequests) => {
            warn!("Too many requests, xid: {xid}, client_addr: {client_addr}, call: {call:?}",);

            Err(Some(accept_stat_data::SYSTEM_ERR))
        }
    }
}

/// Writes an encoded reply record (marker included) with vectored writes.
pub(crate) async fn write_reply<IO: AsyncWrite + Unpin>(
    socket: &mut IO,
    reply: &Encoded,
) -> io::Result<()> {
    let mut slices = match reply.chunks() {
        [head, [], []] => return socket.write_all(head).await,
        chunks => chunks.map(IoSlice::new),
    };
    let mut slices = &mut slices[..];
    while !slices.is_empty() {
        let written = socket.write_vectored(slices).await?;
        if written == 0 {
            return Err(io::ErrorKind::WriteZero.into());
        }
        IoSlice::advance_slices(&mut slices, written);
    }
    Ok(())
}

/// Reads RPC records and dispatches each complete call in its own task.
pub(crate) async fn read_and_dispatch_loop<R, T>(
    mut record_reader: RpcRecordReader<R>,
    context: RPCContext<T>,
    replies: mpsc::Sender<anyhow::Result<Encoded>>,
) -> anyhow::Result<()>
where
    R: AsyncRead + Unpin,
    T: NfsFileSystem + 'static,
{
    // Read before reserving so the next framing error can still be
    // surfaced even when all reply slots are in use.
    while let Some(message) = record_reader.read_record().await? {
        // Reserve before spawning so a client can't accumulate unbounded
        // handler tasks: running calls and queued replies share the same slots.
        let permit = replies.clone().reserve_owned().await?;
        let context = context.clone();
        tokio::spawn(async move {
            match handle_rpc_message(context, message).await {
                Ok(RpcCallOutcome::Reply(reply)) => {
                    permit.send(Ok(reply));
                }
                Ok(RpcCallOutcome::NoReply) => {}
                Err(err) => {
                    error!("Error handling RPC message: {err}");
                    permit.send(Err(err));
                }
            }
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::{self, IoSlice};
    use std::pin::Pin;
    use std::task::{Context, Poll};

    use bytes::Bytes;
    use future_assert::FutureTestExt;
    use nfs3_types::xdr_codec::{Encoded, Encoder, Opaque, Pack};
    use tokio::io::AsyncWrite;

    use super::write_reply;

    /// Accepts at most `limit` bytes per write, across slices.
    struct Trickle {
        out: Vec<u8>,
        limit: usize,
    }

    impl AsyncWrite for Trickle {
        fn poll_write(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            self.poll_write_vectored(cx, &[IoSlice::new(buf)])
        }

        fn poll_write_vectored(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            bufs: &[IoSlice<'_>],
        ) -> Poll<io::Result<usize>> {
            let mut written = 0;
            for buf in bufs {
                let n = buf.len().min(self.limit - written);
                self.out.extend_from_slice(&buf[..n]);
                written += n;
            }
            Poll::Ready(Ok(written))
        }

        fn is_write_vectored(&self) -> bool {
            true
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    /// A reply with a shared payload chunk followed by a non-empty tail.
    fn chunked_reply() -> Encoded {
        let payload: Vec<u8> = (0..=255u8).cycle().take(1025).collect();
        let mut out = Encoder::new();
        7u32.pack(&mut out).expect("pack");
        Opaque::from_bytes(Bytes::from(payload))
            .pack(&mut out)
            .expect("pack");
        8u32.pack(&mut out).expect("pack");
        let reply = out.finish_record().expect("record fits");
        assert!(reply.chunks().iter().all(|chunk| !chunk.is_empty()));
        reply
    }

    #[test]
    fn reply_chunks_survive_partial_writes() {
        let reply = chunked_reply();
        let mut socket = Trickle {
            out: Vec::new(),
            limit: 3,
        };
        write_reply(&mut socket, &reply)
            .unwrap_ready()
            .expect("all chunks written");
        assert_eq!(socket.out, reply.chunks().concat());
    }

    #[test]
    fn zero_progress_is_write_zero() {
        let mut socket = Trickle {
            out: Vec::new(),
            limit: 0,
        };
        let err = write_reply(&mut socket, &chunked_reply())
            .unwrap_ready()
            .expect_err("no progress");
        assert_eq!(err.kind(), io::ErrorKind::WriteZero);
    }
}
