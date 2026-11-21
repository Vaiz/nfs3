use std::io::Cursor;
use std::num::NonZeroUsize;
use std::time::Instant;

use messages::{CompleteRpcMessage, HandleResult, IncomingRpcMessage};
use nfs3_types::rpc::{
    RPC_VERSION_2, accept_stat_data, auth_flavor, auth_stat, auth_unix, call_body, fragment_header,
};
use nfs3_types::xdr_codec::{Pack, Unpack};
use nfs3_types::{nfs3 as nfs, portmap};
use tokio::io::{AsyncBufRead, AsyncWriteExt};
use tokio::sync::mpsc;
use tracing::{error, info, trace, warn};

use crate::context::RPCContext;
use crate::transaction_tracker::{self, TransactionError, TransactionLock};
use crate::vfs::NfsFileSystem;
use crate::{mount_handlers, nfs_handlers, portmap_handlers};

pub(crate) mod messages;

// Information from RFC 5531
// https://datatracker.ietf.org/doc/html/rfc5531

const NFS_ACL_PROGRAM: u32 = 100_227;
const NFS_ID_MAP_PROGRAM: u32 = 100_270;
const NFS_METADATA_PROGRAM: u32 = 200_024;

async fn handle_rpc_message<T>(
    mut context: RPCContext<T>,
    message: CompleteRpcMessage,
) -> anyhow::Result<HandleResult>
where
    T: NfsFileSystem,
{
    let message = IncomingRpcMessage::try_from(message)?;
    let xid = message.xid();
    let call = message.body();
    let prog = call.prog;

    if call.rpcvers != RPC_VERSION_2 {
        warn!("Invalid RPC version {} != {RPC_VERSION_2}", call.rpcvers);
        return message.into_rpc_mismatch();
    }

    let unix = call.cred.flavor == auth_flavor::AUTH_UNIX;
    if unix {
        let auth = auth_unix::unpack(&mut Cursor::new(&call.cred.body.0))?.0;
        context.auth = auth;
    }
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
                return Ok(HandleResult::NoReply);
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
    mut message: IncomingRpcMessage,
    handler: impl AsyncFnOnce(RPCContext<T>, u32, I) -> O,
) -> anyhow::Result<HandleResult>
where
    I: Unpack,
    O: Pack + Send + 'static,
    T: NfsFileSystem,
{
    let mut cursor = message.take_data();
    let (args, _) = match I::unpack(&mut cursor) {
        Ok(ok) => ok,
        Err(err) => {
            error!("Failed to unpack message: {err}");
            return message.into_error_reply(accept_stat_data::GARBAGE_ARGS);
        }
    };
    if cursor.position() != cursor.get_ref().len() as u64 {
        error!("Unpacked message size does not match expected size");
        return message.into_error_reply(accept_stat_data::GARBAGE_ARGS);
    }

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

#[expect(
    clippy::cast_possible_truncation,
    reason = "The assertion bounds the length to the RPC fragment size"
)]
pub(crate) async fn write_fragment<IO: tokio::io::AsyncWrite + Unpin>(
    socket: &mut IO,
    buf: CompleteRpcMessage,
) -> Result<(), anyhow::Error> {
    // TODO: split into many fragments
    let buf = buf.into_inner();
    assert!(buf.len() < (1 << 31));
    let fragment_header = fragment_header::new(buf.len() as u32, true);
    let header_buf = fragment_header.into_xdr_buf();
    socket.write_all(&header_buf).await?;
    trace!("Writing fragment length: {}", buf.len());
    socket.write_all(&buf).await?;
    Ok(())
}

pub(crate) type SocketMessageType = Result<CompleteRpcMessage, anyhow::Error>;

/// Reads RPC records and dispatches each complete message in its own task.
#[derive(Debug)]
pub(crate) struct SocketMessageHandler<T: NfsFileSystem + 'static> {
    reply_send_channel: mpsc::UnboundedSender<SocketMessageType>,
    context: RPCContext<T>,
    max_rpc_record_size: Option<NonZeroUsize>,
}

impl<T> SocketMessageHandler<T>
where
    T: NfsFileSystem + 'static,
{
    /// Creates a new `SocketMessageHandler` with the receiver for queued message replies
    pub(crate) fn new(
        context: RPCContext<T>,
        max_rpc_record_size: Option<NonZeroUsize>,
    ) -> (Self, mpsc::UnboundedReceiver<SocketMessageType>) {
        let (msgsend, msgrecv) = mpsc::unbounded_channel();
        (
            Self {
                reply_send_channel: msgsend,
                context,
                max_rpc_record_size,
            },
            msgrecv,
        )
    }

    pub(crate) async fn read_forever(
        self,
        input: &mut (impl AsyncBufRead + Unpin),
    ) -> Result<(), anyhow::Error> {
        while let Some(message) = CompleteRpcMessage::recv(input, self.max_rpc_record_size).await? {
            let context = self.context.clone();
            let out = self.reply_send_channel.clone();
            tokio::spawn(async move {
                let result = handle_rpc_message(context, message).await;

                match result {
                    Ok(HandleResult::Reply(reply)) => {
                        let _ = out.send(Ok(reply));
                    }
                    Ok(HandleResult::NoReply) => {
                        // No reply needed
                    }
                    Err(err) => {
                        error!("Error handling RPC message: {err}");
                        let _ = out.send(Err(err));
                    }
                }
            });
        }

        Ok(())
    }
}
