use std::io::Cursor;
use std::time::Instant;

use anyhow::anyhow;
use messages::{CompleteRpcMessage, IncomingRpcMessage, OutgoingRpcMessage, PackedRpcMessage};
use nfs3_types::rpc::{
    RPC_VERSION_2, accept_stat_data, auth_flavor, auth_unix, call_body, fragment_header,
};
use nfs3_types::xdr_codec::{Pack, PackedSize, Unpack};
use nfs3_types::{nfs3 as nfs, portmap};
use tokio::io::{AsyncWriteExt, DuplexStream};
use tokio::sync::mpsc;
use tracing::{error, info, trace, warn};

use crate::context::RPCContext;
use crate::transaction_tracker::{TransactionError, TransactionLock};
use crate::units::KIBIBYTE;
use crate::{mount_handlers, nfs_handlers, portmap_handlers};

pub mod messages;

// Information from RFC 5531
// https://datatracker.ietf.org/doc/html/rfc5531

const NFS_ACL_PROGRAM: u32 = 100_227;
const NFS_ID_MAP_PROGRAM: u32 = 100_270;
const NFS_METADATA_PROGRAM: u32 = 200_024;

pub enum HandleResult {
    Reply(CompleteRpcMessage),
    NoReply,
}

impl TryFrom<OutgoingRpcMessage> for HandleResult {
    type Error = anyhow::Error;

    fn try_from(msg: OutgoingRpcMessage) -> Result<Self, Self::Error> {
        let pack_result = msg.pack();
        match pack_result {
            Err(e) => {
                error!("Failed to pack RPC message: {e}");
                Err(anyhow!("Failed to pack RPC message"))
            }
            Ok(msg) => Ok(Self::Reply(msg)),
        }
    }
}

impl TryFrom<Option<OutgoingRpcMessage>> for HandleResult {
    type Error = anyhow::Error;

    fn try_from(msg: Option<OutgoingRpcMessage>) -> Result<Self, Self::Error> {
        msg.map_or(Ok(Self::NoReply), std::convert::TryInto::try_into)
    }
}

impl From<CompleteRpcMessage> for HandleResult {
    fn from(msg: CompleteRpcMessage) -> Self {
        Self::Reply(msg)
    }
}

async fn handle_rpc_message(
    mut context: RPCContext,
    message: CompleteRpcMessage,
) -> anyhow::Result<HandleResult> {
    let message = IncomingRpcMessage::try_from(message)?;
    let xid = message.xid();
    let call = message.body();
    let prog = call.prog;

    if call.rpcvers != RPC_VERSION_2 {
        warn!("Invalid RPC version {} != {RPC_VERSION_2}", call.rpcvers);
        return OutgoingRpcMessage::rpc_mismatch(xid).try_into();
    }

    if call.cred.flavor == auth_flavor::AUTH_UNIX {
        let auth = auth_unix::unpack(&mut Cursor::new(&*call.cred.body))?.0;
        context.auth = auth;
    }

    let transaction = lock_transaction(&context, xid, call);
    if let Err(msg) = transaction {
        return msg.try_into();
    }

    match prog {
        portmap::PROGRAM => portmap_handlers::handle_portmap(&context, message).await,
        nfs3_types::mount::PROGRAM => mount_handlers::handle_mount(&context, message).await,
        nfs::PROGRAM => nfs_handlers::handle_nfs(&context, message).await,
        NFS_ACL_PROGRAM | NFS_ID_MAP_PROGRAM | NFS_METADATA_PROGRAM => {
            trace!("ignoring NFS_ACL packet");
            OutgoingRpcMessage::accept_error(xid, accept_stat_data::PROG_UNAVAIL).try_into()
        }
        _ => {
            warn!("Unknown RPC Program number {prog} != {}", nfs::PROGRAM);
            OutgoingRpcMessage::accept_error(xid, accept_stat_data::PROG_UNAVAIL).try_into()
        }
    }
}

/// Handles the RPC message and returns a result. The handler is an async function
pub async fn handle<'a, I, O>(
    context: &RPCContext,
    mut message: IncomingRpcMessage,
    handler: impl AsyncFnOnce(&RPCContext, u32, I) -> O,
) -> anyhow::Result<HandleResult>
where
    I: Unpack<Cursor<Vec<u8>>>,
    O: Pack<Cursor<Vec<u8>>> + PackedSize + Send + 'static,
{
    let mut cursor = message.take_data();
    let (args, _) = match I::unpack(&mut cursor) {
        Ok(ok) => ok,
        Err(err) => {
            error!("Failed to unpack message: {err}");
            return message
                .into_error_reply(accept_stat_data::GARBAGE_ARGS)
                .try_into();
        }
    };
    if cursor.position() != cursor.get_ref().len() as u64 {
        error!("Unpacked message size does not match expected size");
        return message
            .into_error_reply(accept_stat_data::GARBAGE_ARGS)
            .try_into();
    }

    let result = handler(context, message.xid(), args).await;
    message.into_success_reply(Box::new(result)).try_into()
}

fn lock_transaction(
    context: &RPCContext,
    xid: u32,
    call: &call_body<'_>,
) -> Result<TransactionLock, Option<OutgoingRpcMessage>> {
    let transaction =
        context
            .transaction_tracker
            .start_transaction(&context.client_addr, xid, Instant::now());

    match transaction {
        Ok(lock) => Ok(lock),
        Err(TransactionError::AlreadyExists) => {
            info!(
                "Retransmission detected, xid: {xid}, client_addr: {}, call: {call:?}",
                context.client_addr
            );
            Err(None)
        }
        Err(TransactionError::TooManyRequests) => {
            warn!(
                "Too many requests, xid: {xid}, client_addr: {}, call: {call:?}",
                context.client_addr
            );

            Err(Some(OutgoingRpcMessage::accept_error(
                xid,
                accept_stat_data::SYSTEM_ERR,
            )))
        }
    }
}

#[allow(clippy::cast_possible_truncation)]
pub async fn write_fragment<IO: tokio::io::AsyncWrite + Unpin>(
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

pub type SocketMessageType = Result<CompleteRpcMessage, anyhow::Error>;

/// The Socket Message Handler reads from a `TcpStream` and spawns off
/// subtasks to handle each message. replies are queued into the
/// `reply_send_channel`.
#[derive(Debug)]
pub struct SocketMessageHandler {
    cur_fragment: PackedRpcMessage,
    socket_receive_channel: DuplexStream,
    reply_send_channel: mpsc::UnboundedSender<SocketMessageType>,
    context: RPCContext,
}

impl SocketMessageHandler {
    /// Creates a new `SocketMessageHandler` with the receiver for queued message replies
    pub fn new(
        context: &RPCContext,
    ) -> (
        Self,
        DuplexStream,
        mpsc::UnboundedReceiver<SocketMessageType>,
    ) {
        let (socksend, sockrecv) = tokio::io::duplex(256 * KIBIBYTE as usize);
        let (msgsend, msgrecv) = mpsc::unbounded_channel();
        (
            Self {
                cur_fragment: PackedRpcMessage::new(),
                socket_receive_channel: sockrecv,
                reply_send_channel: msgsend,
                context: context.clone(),
            },
            socksend,
            msgrecv,
        )
    }

    /// Reads a fragment from the socket. This should be looped.
    pub async fn read(&mut self) -> Result<(), anyhow::Error> {
        let is_last = self
            .cur_fragment
            .recv(&mut self.socket_receive_channel)
            .await?;
        if is_last {
            let message = std::mem::replace(&mut self.cur_fragment, PackedRpcMessage::new());
            let message = match message {
                PackedRpcMessage::Complete(data) => data,
                PackedRpcMessage::Incomplete(_) => {
                    unreachable!()
                }
            };

            let context = self.context.clone();
            let send = self.reply_send_channel.clone();
            tokio::spawn(async move {
                let result = handle_rpc_message(context, message).await;

                match result {
                    Ok(HandleResult::Reply(reply)) => {
                        let _ = send.send(Ok(reply));
                    }
                    Ok(HandleResult::NoReply) => {
                        // No reply needed
                    }
                    Err(err) => {
                        error!("Error handling RPC message: {err}");
                        let _ = send.send(Err(anyhow!("Error handling RPC message")));
                    }
                }
            });
        }

        Ok(())
    }
}
