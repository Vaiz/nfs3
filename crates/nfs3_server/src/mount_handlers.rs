use std::io::{Read, Write};

use nfs3_types::mount::*;
use nfs3_types::rpc::*;
use tracing::debug;
use xdr_codec::{Pack, Unpack};

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
    let path = String::unpack(input)?.0;
    let utf8path = &path;
    debug!("mountproc3_mnt({:?},{:?}) ", xid, utf8path);
    let path = if let Some(path) = utf8path.strip_prefix(context.export_name.as_str()) {
        path.trim_start_matches('/').trim_end_matches('/').trim()
    } else {
        // invalid export
        debug!("{:?} --> no matching export", xid);
        make_success_reply(xid).pack(output)?;
        mountstat3::MNT3ERR_NOENT.pack(output)?;
        return Ok(());
    };
    if let Ok(fileid) = context.vfs.path_to_id(path).await {
        let response = mountres3_ok {
            fhandle: fhandle3(context.vfs.id_to_fh(fileid).data),
            auth_flavors: vec![auth_flavor::AUTH_NULL as u32, auth_flavor::AUTH_UNIX as u32],
        };
        debug!("{:?} --> {:?}", xid, response);
        if let Some(ref chan) = context.mount_signal {
            let _ = chan.send(true).await;
        }
        make_success_reply(xid).pack(output)?;
        mountstat3::MNT3_OK.pack(output)?;
        response.pack(output)?;
    } else {
        debug!("{:?} --> MNT3ERR_NOENT", xid);
        make_success_reply(xid).pack(output)?;
        mountstat3::MNT3ERR_NOENT.pack(output)?;
    }
    Ok(())
}

/*
  exports MOUNTPROC3_EXPORT(void) = 5;

  typedef struct groupnode *groups;

  struct groupnode {
       name     gr_name;
       groups   gr_next;
  };

  typedef struct exportnode *exports;

  struct exportnode {
       dirpath  ex_dir;
       groups   ex_groups;
       exports  ex_next;
  };

DESCRIPTION

  Procedure EXPORT returns a list of all the exported file
  systems and which clients are allowed to mount each one.
  The names in the group list are implementation-specific
  and cannot be directly interpreted by clients. These names
  can represent hosts or groups of hosts.

IMPLEMENTATION

  This procedure generally returns the contents of a list of
  shared or exported file systems. These are the file
  systems which are made available to NFS version 3 protocol
  clients.
 */

pub fn mountproc3_export(
    xid: u32,
    _: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    debug!("mountproc3_export({:?}) ", xid);
    make_success_reply(xid).pack(output)?;
    true.pack(output)?;
    // dirpath
    context.export_name.pack(output)?;
    // groups
    false.pack(output)?;
    // next exports
    false.pack(output)?;
    Ok(())
}

pub async fn mountproc3_umnt(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let path = String::unpack(input)?.0;
    let utf8path = &path;
    debug!("mountproc3_umnt({:?},{:?}) ", xid, utf8path);
    if let Some(ref chan) = context.mount_signal {
        let _ = chan.send(false).await;
    }
    make_success_reply(xid).pack(output)?;
    mountstat3::MNT3_OK.pack(output)?;
    Ok(())
}

pub async fn mountproc3_umnt_all(
    xid: u32,
    _input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    debug!("mountproc3_umnt_all({:?}) ", xid);
    if let Some(ref chan) = context.mount_signal {
        let _ = chan.send(false).await;
    }
    make_success_reply(xid).pack(output)?;
    mountstat3::MNT3_OK.pack(output)?;
    Ok(())
}
