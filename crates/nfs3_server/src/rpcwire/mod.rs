use std::time::Instant;

use call::{DecodedCall, RpcCall, RpcCallOutcome};
use messages::RpcRecordReader;
use nfs3_types::rpc::{RPC_VERSION_2, accept_stat_data, auth_stat, call_body};
use nfs3_types::xdr_codec::{Decoder, Encoded, Error, Pack, Unpack};
use nfs3_types::{nfs3 as nfs, portmap};
use tokio::io::AsyncRead;
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

pub(crate) async fn handle_rpc_message<T>(
    mut context: RPCContext<T>,
    mut args: Decoder,
) -> anyhow::Result<RpcCallOutcome>
where
    T: NfsFileSystem,
{
    let message = match RpcCall::unpack(&mut args)? {
        DecodedCall::Call(message) => message,
        DecodedCall::Rejected(reply) => return Ok(reply),
    };
    let xid = message.xid();
    let call = message.body();
    let prog = call.prog;

    if call.rpcvers != RPC_VERSION_2 {
        warn!("Invalid RPC version {} != {RPC_VERSION_2}", call.rpcvers);
        return message.into_rpc_mismatch();
    }

    let unix = match context.set_auth(call) {
        Ok(unix) => unix,
        Err(err) => {
            warn!("Invalid AUTH_SYS credential for XID {xid}: {err}");
            return message.into_auth_error(auth_stat::AUTH_BADCRED);
        }
    };
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
        portmap::PROGRAM => portmap_handlers::handle_portmap(context, message, args).await,
        nfs3_types::mount::PROGRAM => mount_handlers::handle_mount(context, message, args).await,
        nfs::PROGRAM => nfs_handlers::handle_nfs(context, message, args).await,
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
    message: RpcCall,
    args: Decoder,
    handler: impl AsyncFnOnce(RPCContext<T>, u32, I) -> O,
) -> anyhow::Result<RpcCallOutcome>
where
    I: Unpack,
    O: Pack + Send + 'static,
    T: NfsFileSystem,
{
    let args = match unpack_exact::<I>(args) {
        Ok(args) => args,
        Err(err) => {
            error!("Failed to unpack message: {err}");
            return message.into_error_reply(accept_stat_data::GARBAGE_ARGS);
        }
    };

    let result = handler(context, message.xid(), args).await;
    message.into_success_reply(&result)
}

/// Decodes a value that must span the rest of `input`.
///
/// Consumes `input`, so only decoded values that share its buffer keep it in
/// memory afterwards.
pub(crate) fn unpack_exact<T: Unpack>(mut input: Decoder) -> Result<T, Error> {
    let value = T::unpack(&mut input)?;
    if !input.is_empty() {
        return Err(Error::TrailingBytes(input.remaining()));
    }
    Ok(value)
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
    use nfs3_types::rpc::auth_unix;
    use nfs3_types::xdr_codec::{Decoder, Error};

    use super::unpack_exact;

    #[test]
    fn unpack_exact_rejects_trailing_bytes() {
        let mut bytes = nfs3_types::rpc::opaque_auth::auth_unix(&auth_unix::default())
            .body
            .as_ref()
            .to_vec();
        bytes.extend_from_slice(&[0; 4]);

        let result = unpack_exact::<auth_unix>(Decoder::copying(bytes));
        assert!(matches!(result, Err(Error::TrailingBytes(4))));
    }
}
