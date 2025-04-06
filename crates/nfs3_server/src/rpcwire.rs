use std::f32::consts::E;
use std::io::{Cursor, Read, Write};
use std::time::Instant;

use anyhow::anyhow;
use messages::{IncomingRpcMessage, OutgoingRpcMessage, PackedRpcMessage};
use nfs3_types::rpc::{accept_stat_data, auth_flavor, auth_unix, call_body, fragment_header, msg_body, rpc_msg, RPC_VERSION_2};
use nfs3_types::xdr_codec::{Pack, Unpack};
use nfs3_types::{nfs3 as nfs, portmap};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::sync::mpsc;
use tracing::{error, info, trace, warn};

use crate::context::RPCContext;
use crate::rpc::{prog_unavail_reply_message, rpc_vers_mismatch, system_err_reply_message};
use crate::transaction_tracker::{TransactionError, TransactionLock};
use crate::units::KIBIBYTE;
use crate::{mount_handlers, nfs_handlers, portmap_handlers};

pub(crate) mod messages;

// Information from RFC 5531
// https://datatracker.ietf.org/doc/html/rfc5531

const NFS_ACL_PROGRAM: u32 = 100_227;
const NFS_ID_MAP_PROGRAM: u32 = 100_270;
const NFS_METADATA_PROGRAM: u32 = 200_024;

async fn handle_rpc_message(    
    mut context: RPCContext,
    message: PackedRpcMessage,
) -> anyhow::Result<Option<OutgoingRpcMessage>> {
    let message = IncomingRpcMessage::try_from(message)?;
    let xid = message.rpc().xid;
    let call = match &message.rpc().body {
        msg_body::CALL(call) => call,
        msg_body::REPLY(_) => {
            error!("Unexpectedly received a Reply instead of a Call");
            return Err(anyhow!("Bad RPC Call format"));
        }
    };

    if call.rpcvers != RPC_VERSION_2 {
        warn!("Invalid RPC version {} != {RPC_VERSION_2}", call.rpcvers);
        return Ok(Some(OutgoingRpcMessage::rpc_mismatch(xid)));
    }

    if call.cred.flavor == auth_flavor::AUTH_UNIX {
        let auth = auth_unix::unpack(&mut Cursor::new(&*call.cred.body))?.0;
        context.auth = auth;
    }

    let transaction = lock_transaction(&context, xid, call);
    if let Err(e) = transaction {
        return Ok(e);
    }

    match call.prog {
        // nfs::PROGRAM => {
        //     nfs_handlers::handle_nfs(xid, call, &message.data, &mut context).await?;
        // }
        // portmap::PROGRAM => {
        //     portmap_handlers::handle_portmap(xid, call, &message.data, &mut context)?;
        // }
        // nfs3_types::mount::PROGRAM => {
        //     mount_handlers::handle_mount(xid, call, &message.data, &mut context).await?;
        // }
        NFS_ACL_PROGRAM | NFS_ID_MAP_PROGRAM | NFS_METADATA_PROGRAM => {
            trace!("ignoring NFS_ACL packet");
            Ok(Some(OutgoingRpcMessage::accept_error(xid, accept_stat_data::PROG_UNAVAIL)))
        }
        _ => {
            warn!(
                "Unknown RPC Program number {} != {}",
                call.prog,
                nfs::PROGRAM
            );
            Ok(Some(OutgoingRpcMessage::accept_error(xid, accept_stat_data::PROG_UNAVAIL)))
        }
    }
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
            return Err(None);
        }
        Err(TransactionError::TooManyRequests) => {
            warn!(
                "Too many requests, xid: {xid}, client_addr: {}, call: {call:?}",
                context.client_addr
            );

            Err(Some(OutgoingRpcMessage::accept_error(xid, accept_stat_data::SYSTEM_ERR)))
        }
    }
}

async fn handle_rpc(
    input: &mut impl Read,
    output: &mut impl Write,
    mut context: RPCContext,
) -> Result<bool, anyhow::Error> {
    let recv = rpc_msg::unpack(input)?.0;
    let xid = recv.xid;

    let call = match recv.body {
        msg_body::CALL(call) => call,
        msg_body::REPLY(_) => {
            error!("Unexpectedly received a Reply instead of a Call");
            return Err(anyhow!("Bad RPC Call format"));
        }
    };

    if call.cred.flavor == auth_flavor::AUTH_UNIX {
        let auth = auth_unix::unpack(&mut Cursor::new(&*call.cred.body))?.0;
        context.auth = auth;
    }
    if call.rpcvers != RPC_VERSION_2 {
        warn!("Invalid RPC version {} != {RPC_VERSION_2}", call.rpcvers);
        rpc_vers_mismatch(xid).pack(output)?;
        return Ok(true);
    }

    let transaction =
        context
            .transaction_tracker
            .start_transaction(&context.client_addr, xid, Instant::now());

    let _lock = match transaction {
        Ok(lock) => lock,
        Err(TransactionError::AlreadyExists) => {
            info!(
                "Retransmission detected, xid: {xid}, client_addr: {}, call: {call:?}",
                context.client_addr
            );
            return Ok(false);
        }
        Err(TransactionError::TooManyRequests) => {
            warn!(
                "Too many requests, xid: {xid}, client_addr: {}, call: {call:?}",
                context.client_addr
            );

            system_err_reply_message(xid).pack(output)?;
            return Ok(true);
        }
    };

    if call.prog == nfs::PROGRAM {
        nfs_handlers::handle_nfs(xid, call, input, output, &context).await?;
    } else if call.prog == portmap::PROGRAM {
        portmap_handlers::handle_portmap(xid, &call, input, output, &context)?;
    } else if call.prog == nfs3_types::mount::PROGRAM {
        mount_handlers::handle_mount(xid, call, input, output, &context).await?;
    } else if call.prog == NFS_ACL_PROGRAM
        || call.prog == NFS_ID_MAP_PROGRAM
        || call.prog == NFS_METADATA_PROGRAM
    {
        trace!("ignoring NFS_ACL packet");
        prog_unavail_reply_message(xid).pack(output)?;
    } else {
        warn!(
            "Unknown RPC Program number {} != {}",
            call.prog,
            nfs::PROGRAM
        );
        prog_unavail_reply_message(xid).pack(output)?;
    }

    Ok(true)
}

/// RFC 1057 Section 10
/// When RPC messages are passed on top of a byte stream transport
/// protocol (like TCP), it is necessary to delimit one message from
/// another in order to detect and possibly recover from protocol errors.
/// This is called record marking (RM).  Sun uses this RM/TCP/IP
/// transport for passing RPC messages on TCP streams.  One RPC message
/// fits into one RM record.
///
/// A record is composed of one or more record fragments.  A record
/// fragment is a four-byte header followed by 0 to (2**31) - 1 bytes of
/// fragment data.  The bytes encode an unsigned binary number; as with
/// XDR integers, the byte order is from highest to lowest.  The number
/// encodes two values -- a boolean which indicates whether the fragment
/// is the last fragment of the record (bit value 1 implies the fragment
/// is the last fragment) and a 31-bit unsigned binary value which is the
/// length in bytes of the fragment's data.  The boolean value is the
/// highest-order bit of the header; the length is the 31 low-order bits.
/// (Note that this record specification is NOT in XDR standard form!)
async fn read_fragment(
    socket: &mut DuplexStream,
    append_to: &mut Vec<u8>,
) -> Result<bool, anyhow::Error> {
    let mut header_buf = [0_u8; 4];
    socket.read_exact(&mut header_buf).await?;
    let fragment_header: fragment_header = header_buf.into();
    let is_last = fragment_header.eof();
    let length = fragment_header.fragment_length() as usize;
    trace!("Reading fragment length:{length}, last:{is_last}");
    let start_offset = append_to.len();
    append_to.resize(append_to.len() + length, 0);
    socket.read_exact(&mut append_to[start_offset..]).await?;
    trace!("Finishing Reading fragment length:{length}, last:{is_last}",);
    Ok(is_last)
}

#[allow(clippy::cast_possible_truncation)]
pub async fn write_fragment<IO: tokio::io::AsyncWrite + Unpin>(
    socket: &mut IO,
    buf: &[u8],
) -> Result<(), anyhow::Error> {
    // TODO: split into many fragments
    assert!(buf.len() < (1 << 31));
    let fragment_header = fragment_header::new(buf.len() as u32, true);
    let header_buf = fragment_header.into_xdr_buf();
    socket.write_all(&header_buf).await?;
    trace!("Writing fragment length:{}", buf.len());
    socket.write_all(buf).await?;
    Ok(())
}

pub type SocketMessageType = Result<Vec<u8>, anyhow::Error>;

/// The Socket Message Handler reads from a `TcpStream` and spawns off
/// subtasks to handle each message. replies are queued into the
/// `reply_send_channel`.
#[derive(Debug)]
pub struct SocketMessageHandler {
    cur_fragment: Vec<u8>,
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
                cur_fragment: Vec::new(),
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
        let is_last =
            read_fragment(&mut self.socket_receive_channel, &mut self.cur_fragment).await?;
        if is_last {
            let fragment = std::mem::take(&mut self.cur_fragment);
            let context = self.context.clone();
            let send = self.reply_send_channel.clone();
            tokio::spawn(async move {
                let mut write_buf: Vec<u8> = Vec::new();
                let mut write_cursor = Cursor::new(&mut write_buf);
                let maybe_reply =
                    handle_rpc(&mut Cursor::new(fragment), &mut write_cursor, context).await;
                match maybe_reply {
                    Err(e) => {
                        error!("RPC Error: {:?}", e);
                        let _ = send.send(Err(e));
                    }
                    Ok(true) => {
                        let _ = std::io::Write::flush(&mut write_cursor);
                        let _ = send.send(Ok(write_buf));
                    }
                    Ok(false) => {
                        // do not reply
                    }
                }
            });
        }
        Ok(())
    }
}
