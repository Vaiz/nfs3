use std::io::Cursor;
use std::time::Instant;

use call::{RpcCall, RpcCallOutcome};
use messages::{RpcRecord, RpcRecordReader};
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

pub(crate) mod call;
pub(crate) mod messages;

// Information from RFC 5531
// https://datatracker.ietf.org/doc/html/rfc5531

const NFS_ACL_PROGRAM: u32 = 100_227;
const NFS_ID_MAP_PROGRAM: u32 = 100_270;
const NFS_METADATA_PROGRAM: u32 = 200_024;

async fn handle_rpc_message<T>(
    context: RPCContext<T>,
    message: RpcRecord,
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

    let auth = if call.cred.flavor == auth_flavor::AUTH_UNIX {
        Some(auth_unix::unpack(&mut Cursor::new(&call.cred.body.0))?.0)
    } else {
        None
    };
    if prog != portmap::PROGRAM && !context.vfs.authorize(auth.as_ref()) {
        warn!(
            "refusing a call from uid {} on {}",
            auth.as_ref().map_or(0, |auth| auth.uid),
            context.client_addr
        );
        return message.into_auth_error(auth_stat::AUTH_TOOWEAK);
    }
    drop(auth);

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
    buf: RpcRecord,
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

/// Reads RPC records and dispatches each complete call in its own task.
pub(crate) async fn read_and_dispatch_loop<R, T>(
    mut record_reader: RpcRecordReader<R>,
    context: RPCContext<T>,
    replies: mpsc::Sender<anyhow::Result<RpcRecord>>,
) -> anyhow::Result<()>
where
    R: AsyncBufRead + Unpin,
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

#[cfg(all(test, feature = "__test_reexports", feature = "memfs"))]
mod tests {
    use std::sync::Arc;

    use nfs3_types::nfs3::{NFS_PROGRAM, fattr3, filename3, nfspath3, nfsstat3};
    use nfs3_types::rpc::{msg_body, opaque_auth, rejected_reply, reply_body, rpc_msg};

    use super::*;
    use crate::memfs::{MemFs, MemFsConfig};
    use crate::vfs::adapters::ReadOnlyAdapter;
    use crate::vfs::{FileHandleU64, NfsReadFileSystem, ReadDirPlusIterator};

    struct AuthFs(MemFs);

    impl NfsReadFileSystem for AuthFs {
        type Handle = FileHandleU64;

        fn root_dir(&self) -> Self::Handle {
            self.0.root_dir()
        }

        fn authorize(&self, auth: Option<&auth_unix>) -> bool {
            auth.is_some_and(|auth| auth.uid == 1000 && auth.gids == [1001, 1002])
        }

        async fn lookup(
            &self,
            dirid: &Self::Handle,
            filename: &filename3<'_>,
        ) -> Result<Self::Handle, nfsstat3> {
            self.0.lookup(dirid, filename).await
        }

        async fn getattr(&self, id: &Self::Handle) -> Result<fattr3, nfsstat3> {
            self.0.getattr(id).await
        }

        async fn read(
            &self,
            id: &Self::Handle,
            offset: u64,
            count: u32,
        ) -> Result<(Vec<u8>, bool), nfsstat3> {
            self.0.read(id, offset, count).await
        }

        async fn readdirplus(
            &self,
            dirid: &Self::Handle,
            cookie: u64,
        ) -> Result<impl ReadDirPlusIterator<Self::Handle>, nfsstat3> {
            self.0.readdirplus(dirid, cookie).await
        }

        async fn readlink(&self, id: &Self::Handle) -> Result<nfspath3<'_>, nfsstat3> {
            self.0.readlink(id).await
        }
    }

    #[tokio::test]
    async fn calls_on_the_same_connection_use_only_their_own_credentials() -> anyhow::Result<()> {
        let fs = MemFs::new(MemFsConfig::default()).expect("initialize the test filesystem");
        let connection = RPCContext::test_ctx("/mnt", Arc::new(ReadOnlyAdapter::new(AuthFs(fs))));
        let auth = |uid| {
            opaque_auth::auth_unix(&auth_unix {
                uid,
                gids: vec![1001, 1002],
                ..auth_unix::default()
            })
        };
        for (xid, cred, authorized) in [
            (1, auth(1000), true),
            (2, opaque_auth::default(), false),
            (3, auth(2000), false),
            (4, auth(1000), true),
        ] {
            let call = rpc_msg {
                xid,
                body: msg_body::CALL(call_body {
                    rpcvers: RPC_VERSION_2,
                    prog: nfs::PROGRAM,
                    vers: nfs::VERSION,
                    proc: NFS_PROGRAM::NFSPROC3_NULL as u32,
                    cred,
                    verf: opaque_auth::default(),
                }),
            };
            let mut bytes = Vec::new();
            call.pack(&mut bytes)?;
            let context = connection.clone();
            assert!(Arc::ptr_eq(&context.client_addr, &connection.client_addr));
            let RpcCallOutcome::Reply(reply) =
                handle_rpc_message(context, RpcRecord::new(bytes)).await?
            else {
                anyhow::bail!("expected a reply to call {xid}");
            };
            let (reply, _) = rpc_msg::unpack(&mut Cursor::new(reply.into_inner()))?;
            assert_eq!(reply.xid, xid);
            match (authorized, reply.body) {
                (true, msg_body::REPLY(reply_body::MSG_ACCEPTED(reply)))
                    if matches!(reply.reply_data, accept_stat_data::SUCCESS) => {}
                (
                    false,
                    msg_body::REPLY(reply_body::MSG_DENIED(rejected_reply::AUTH_ERROR(
                        auth_stat::AUTH_TOOWEAK,
                    ))),
                ) => {}
                (_, body) => anyhow::bail!("unexpected reply to call {xid}: {body:?}"),
            }
        }
        Ok(())
    }
}
