use nfs3_types::mount::{
    MOUNT_PROGRAM, VERSION, dirpath, export_node, exports, fhandle3, mountres3, mountres3_ok,
    mountstat3,
};
use nfs3_types::rpc::{accept_stat_data, auth_flavor};
use nfs3_types::xdr_codec::{List, Opaque, Void};
use tracing::{debug, error, warn};

use crate::context::RPCContext;
use crate::rpcwire::handle;
use crate::rpcwire::messages::{HandleResult, IncomingRpcMessage, OutgoingRpcMessage};

#[allow(clippy::enum_glob_use)]
pub async fn handle_mount(
    context: &RPCContext,
    message: IncomingRpcMessage,
) -> anyhow::Result<HandleResult> {
    use MOUNT_PROGRAM::*;

    let call = message.body();
    let xid = message.xid();

    debug!("handle_nfs({xid}, {call:?}");
    if call.vers != VERSION {
        warn!("Invalid Mount Version number {} != {VERSION}", call.vers);
        return OutgoingRpcMessage::accept_error(
            message.xid(),
            accept_stat_data::PROG_MISMATCH {
                low: VERSION,
                high: VERSION,
            },
        )
        .try_into();
    }

    let Ok(proc) = MOUNT_PROGRAM::try_from(call.proc) else {
        error!("invalid Mount Program number {}", call.proc);
        return OutgoingRpcMessage::accept_error(xid, accept_stat_data::PROC_UNAVAIL).try_into();
    };

    debug!("{proc}({})", message.xid());
    match proc {
        MOUNTPROC3_NULL => handle(context, message, mountproc3_null).await,
        MOUNTPROC3_MNT => handle(context, message, mountproc3_mnt).await,
        MOUNTPROC3_UMNT => handle(context, message, mountproc3_umnt).await,
        MOUNTPROC3_UMNTALL => handle(context, message, mountproc3_umnt_all).await,
        MOUNTPROC3_EXPORT => handle(context, message, mountproc3_export).await,
        MOUNTPROC3_DUMP => {
            warn!("Unimplemented message {proc}");
            message.into_error_reply(accept_stat_data::PROC_UNAVAIL)
        }
    }
}

async fn mountproc3_null(_: &RPCContext, _: u32, _: Void) -> Void {
    Void
}

async fn mountproc3_mnt(context: &RPCContext, xid: u32, path: dirpath<'_>) -> mountres3<'static> {
    let path = std::str::from_utf8(&path.0);
    let utf8path = match path {
        Ok(path) => path,
        Err(e) => {
            tracing::error!("{xid} --> invalid mount path: {e}");
            return mountres3::Err(mountstat3::MNT3ERR_INVAL);
        }
    };

    debug!("mountproc3_mnt({xid},{utf8path})");
    let path = if let Some(path) = utf8path.strip_prefix(context.export_name.as_str()) {
        path.trim_start_matches('/').trim_end_matches('/').trim()
    } else {
        // invalid export
        debug!("{xid} --> no matching export");
        return mountres3::Err(mountstat3::MNT3ERR_NOENT);
    };

    match context.vfs.path_to_id(path).await {
        Ok(fileid) => {
            let response = mountres3_ok {
                fhandle: fhandle3(context.vfs.id_to_fh(fileid).data),
                auth_flavors: vec![auth_flavor::AUTH_NULL as u32, auth_flavor::AUTH_UNIX as u32],
            };
            debug!("{xid} --> {response:?}");
            if let Some(ref chan) = context.mount_signal {
                let _ = chan.send(true).await;
            }
            mountres3::Ok(response)
        }
        Err(e) => {
            debug!("{xid} --> MNT3ERR_NOENT({e:?})");
            mountres3::Err(mountstat3::MNT3ERR_NOENT)
        }
    }
}

/// exports `MOUNTPROC3_EXPORT(void)` = 5;
///
/// typedef struct groupnode *groups;
///
/// struct groupnode {
/// name     `gr_name`;
/// groups   `gr_next`;
/// };
///
/// typedef struct exportnode *exports;
///
/// struct exportnode {
/// dirpath  `ex_dir`;
// groups   ex_groups;
// exports  ex_next;
/// };
///
/// DESCRIPTION
///
/// Procedure EXPORT returns a list of all the exported file
/// systems and which clients are allowed to mount each one.
/// The names in the group list are implementation-specific
/// and cannot be directly interpreted by clients. These names
/// can represent hosts or groups of hosts.
///
/// IMPLEMENTATION
///
/// This procedure generally returns the contents of a list of
/// shared or exported file systems. These are the file
/// systems which are made available to NFS version 3 protocol
/// clients.
async fn mountproc3_export(context: &RPCContext, _: u32, _: Void) -> exports<'static, 'static> {
    let export_name = context.export_name.as_bytes().to_vec();
    List(vec![export_node {
        ex_dir: dirpath(Opaque::owned(export_name)),
        ex_groups: List::default(),
    }])
}

async fn mountproc3_umnt(context: &RPCContext, xid: u32, path: dirpath<'_>) -> Void {
    let utf8path = match std::str::from_utf8(&path.0) {
        Ok(path) => path,
        Err(e) => {
            tracing::warn!("{xid} --> invalid mount path: {e}");
            return Void;
        }
    };

    debug!("mountproc3_umnt({xid},{utf8path})");
    if let Some(ref chan) = context.mount_signal {
        let _ = chan.send(false).await;
    }
    Void
}

pub async fn mountproc3_umnt_all(context: &RPCContext, xid: u32, _: Void) -> Void {
    debug!("mountproc3_umnt_all({xid})");
    if let Some(ref chan) = context.mount_signal {
        let _ = chan.send(false).await;
    }
    Void
}
