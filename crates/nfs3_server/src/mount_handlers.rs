use std::io::{Read, Write};

use nfs3_types::mount::*;
use nfs3_types::rpc::*;
use nfs3_types::xdr_codec::{List, Opaque, Pack, Unpack};
use tracing::debug;

use crate::context::RPCContext;
use crate::rpc::*;

pub async fn handle_mount(
    xid: u32,
    call: call_body<'_>,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let prog = MOUNT_PROGRAM::try_from(call.proc);

    match prog {
        Ok(MOUNT_PROGRAM::MOUNTPROC3_NULL) => mountproc3_null(xid, input, output)?,
        Ok(MOUNT_PROGRAM::MOUNTPROC3_MNT) => mountproc3_mnt(xid, input, output, context).await?,
        Ok(MOUNT_PROGRAM::MOUNTPROC3_UMNT) => mountproc3_umnt(xid, input, output, context).await?,
        Ok(MOUNT_PROGRAM::MOUNTPROC3_UMNTALL) => {
            mountproc3_umnt_all(xid, input, output, context).await?
        }
        Ok(MOUNT_PROGRAM::MOUNTPROC3_EXPORT) => mountproc3_export(xid, input, output, context)?,
        _ => {
            proc_unavail_reply_message(xid).pack(output)?;
        }
    }
    Ok(())
}

pub fn mountproc3_null(
    xid: u32,
    _: &mut impl Read,
    output: &mut impl Write,
) -> Result<(), anyhow::Error> {
    debug!("mountproc3_null({:?}) ", xid);
    // build an RPC reply
    let msg = make_success_reply(xid);
    debug!("\t{:?} --> {:?}", xid, msg);
    msg.pack(output)?;
    Ok(())
}

pub async fn mountproc3_mnt(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let path = dirpath::unpack(input)?.0;
    let result = mountproc3_mount_impl(xid, path, context).await;
    make_success_reply(xid).pack(output)?;
    result.pack(output)?;
    Ok(())
}

async fn mountproc3_mount_impl(
    xid: u32,
    path: dirpath<'_>,
    context: &RPCContext,
) -> mountres3<'static> {
    let path = std::str::from_utf8(&path.0);
    let utf8path = match path {
        Ok(path) => path,
        Err(e) => {
            tracing::error!("{xid} --> invalid mount path: {e}");
            return mountres3::Err(mountstat3::MNT3ERR_INVAL);
        }
    };

    debug!("mountproc3_mnt({:?},{:?}) ", xid, utf8path);
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

// exports MOUNTPROC3_EXPORT(void) = 5;
//
// typedef struct groupnode *groups;
//
// struct groupnode {
// name     gr_name;
// groups   gr_next;
// };
//
// typedef struct exportnode *exports;
//
// struct exportnode {
// dirpath  ex_dir;
// groups   ex_groups;
// exports  ex_next;
// };
//
// DESCRIPTION
//
// Procedure EXPORT returns a list of all the exported file
// systems and which clients are allowed to mount each one.
// The names in the group list are implementation-specific
// and cannot be directly interpreted by clients. These names
// can represent hosts or groups of hosts.
//
// IMPLEMENTATION
//
// This procedure generally returns the contents of a list of
// shared or exported file systems. These are the file
// systems which are made available to NFS version 3 protocol
// clients.

pub fn mountproc3_export(
    xid: u32,
    _: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    debug!("mountproc3_export({:?}) ", xid);

    let response: exports = List(vec![export_node {
        ex_dir: dirpath(Opaque::borrowed(context.export_name.as_bytes())),
        ex_groups: List::default(),
    }]);

    make_success_reply(xid).pack(output)?;
    response.pack(output)?;

    Ok(())
}

pub async fn mountproc3_umnt(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let path = dirpath::unpack(input)?.0;
    let utf8path = match std::str::from_utf8(&path.0) {
        Ok(path) => path,
        Err(e) => {
            tracing::error!("{xid} --> invalid mount path: {e}");
            garbage_args_reply_message(xid).pack(output)?;
            return Ok(())
        }
    };

    debug!("mountproc3_umnt({xid},{utf8path})");
    if let Some(ref chan) = context.mount_signal {
        let _ = chan.send(false).await;
    }
    make_success_reply(xid).pack(output)?;
    Ok(())
}

pub async fn mountproc3_umnt_all(
    xid: u32,
    _input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    debug!("mountproc3_umnt_all({xid})");
    if let Some(ref chan) = context.mount_signal {
        let _ = chan.send(false).await;
    }
    make_success_reply(xid).pack(output)?;
    Ok(())
}
