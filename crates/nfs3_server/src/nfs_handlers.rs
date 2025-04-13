#![allow(clippy::unwrap_used)] // FIXME: will fix this after some refactoring

use std::io::{Cursor, Read, Write};

use nfs3_types::nfs3::*;
use nfs3_types::rpc::accept_stat_data;
use nfs3_types::xdr_codec::{BoundedList, Opaque, Pack, PackedSize, Unpack, Void};
use tracing::{debug, error, trace, warn};

use crate::context::RPCContext;
use crate::nfs_ext::{BoundedEntryPlusList, CookieVerfExt};
use crate::rpc::{make_success_reply, proc_unavail_reply_message};
use crate::rpcwire::HandleResult;
use crate::rpcwire::messages::{CompleteRpcMessage, IncomingRpcMessage, OutgoingRpcMessage};
use crate::units::{GIBIBYTE, TEBIBYTE};
use crate::vfs::{NextResult, VFSCapabilities};

pub async fn handle_nfs(
    context: &RPCContext,
    mut message: IncomingRpcMessage,
) -> anyhow::Result<HandleResult> {
    use NFS_PROGRAM::*;

    let call = message.body();
    let xid = message.xid();

    debug!("handle_nfs({xid}, {call:?}");
    if call.vers != VERSION {
        error!("Invalid NFSv3 Version number {} != {VERSION}", call.vers,);
        return OutgoingRpcMessage::accept_error(
            message.xid(),
            accept_stat_data::PROG_MISMATCH {
                low: VERSION,
                high: VERSION,
            },
        )
        .try_into();
    }

    let Ok(proc) = NFS_PROGRAM::try_from(call.proc) else {
        error!("invalid NFS3 Program number {}", call.proc);
        return OutgoingRpcMessage::accept_error(xid, accept_stat_data::PROC_UNAVAIL).try_into();
    };

    match proc {
        NFSPROC3_NULL => handle(context, proc, message, nfsproc3_null).await,
        NFSPROC3_GETATTR => handle(context, proc, message, nfsproc3_getattr).await,
        NFSPROC3_LOOKUP => handle(context, proc, message, nfsproc3_lookup).await,
        NFSPROC3_READ => handle(context, proc, message, nfsproc3_read).await,
        NFSPROC3_FSINFO => handle(context, proc, message, nfsproc3_fsinfo).await,
        NFSPROC3_ACCESS => handle(context, proc, message, nfsproc3_access).await,
        NFSPROC3_PATHCONF => handle(context, proc, message, nfsproc3_pathconf).await,
        NFSPROC3_FSSTAT => handle(context, proc, message, nfsproc3_fsstat).await,
        NFSPROC3_READDIR => handle(context, proc, message, nfsproc3_readdir).await,
        NFSPROC3_READDIRPLUS => handle(context, proc, message, nfsproc3_readdirplus).await,
        NFSPROC3_WRITE => handle(context, proc, message, nfsproc3_write).await,
        NFSPROC3_CREATE => handle(context, proc, message, nfsproc3_create).await,
        NFSPROC3_SETATTR => handle(context, proc, message, nfsproc3_setattr).await,
        NFSPROC3_REMOVE | NFSPROC3_RMDIR => handle(context, proc, message, nfsproc3_remove).await,
        _ => {
            // deprecated way of handling NFS messages
            let mut input = message.take_data();
            let mut output = Cursor::<Vec<u8>>::default();
            handle_nfs_old(xid, proc, &mut input, &mut output, context).await?;
            Ok(CompleteRpcMessage::new(output.into_inner()).into())
        }
    }
}

async fn handle<'a, I, O>(
    context: &RPCContext,
    proc: NFS_PROGRAM,
    mut message: IncomingRpcMessage,
    handler: impl AsyncFnOnce(&RPCContext, u32, I) -> O,
) -> anyhow::Result<HandleResult>
where
    I: Unpack<Cursor<Vec<u8>>>,
    O: Pack<Cursor<&'static mut [u8]>> + PackedSize + Send + 'static,
{
    debug!("{proc}({})", message.xid());

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

async fn handle_nfs_old(
    xid: u32,
    proc: NFS_PROGRAM,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    match proc {
        NFS_PROGRAM::NFSPROC3_RENAME => nfsproc3_rename(xid, input, output, context).await?,
        NFS_PROGRAM::NFSPROC3_MKDIR => nfsproc3_mkdir(xid, input, output, context).await?,
        NFS_PROGRAM::NFSPROC3_SYMLINK => nfsproc3_symlink(xid, input, output, context).await?,
        NFS_PROGRAM::NFSPROC3_READLINK => nfsproc3_readlink(xid, input, output, context).await?,
        _ => {
            warn!("Unimplemented message {proc}");
            proc_unavail_reply_message(xid).pack(output)?;
        } /* NFSPROC3_MKNOD,
           * NFSPROC3_LINK,
           * NFSPROC3_COMMIT,
           * INVALID */
    }
    Ok(())
}

async fn nfsproc3_null(_: &RPCContext, _: u32, _: Void) -> Void {
    Void
}

async fn nfsproc3_getattr(
    context: &RPCContext,
    xid: u32,
    getattr3args: GETATTR3args,
) -> GETATTR3res {
    let handle = getattr3args.object;

    let id = match context.vfs.fh_to_id(&handle) {
        Ok(id) => id,
        Err(stat) => {
            warn!("getattr error {} --> {stat}", xid);
            return GETATTR3res::Err((stat, Void));
        }
    };

    match context.vfs.getattr(id).await {
        Ok(obj_attributes) => {
            debug!(" {} --> {obj_attributes:?}", xid);
            GETATTR3res::Ok(GETATTR3resok { obj_attributes })
        }
        Err(stat) => {
            warn!("getattr error {} --> {stat}", xid);
            GETATTR3res::Err((stat, Void))
        }
    }
}

async fn nfsproc3_lookup<'a>(
    context: &RPCContext,
    xid: u32,
    lookup3args: LOOKUP3args<'a>,
) -> LOOKUP3res {
    let dirops = lookup3args.what;

    let dirid = match context.vfs.fh_to_id(&dirops.dir) {
        Ok(dirid) => dirid,
        Err(stat) => {
            warn!("lookup error {}({:?}) --> {stat}", xid, dirops.name,);
            return LOOKUP3res::Err((stat, LOOKUP3resfail::default()));
        }
    };
    let dir_attributes = nfs_option_from_result(context.vfs.getattr(dirid).await);
    match context.vfs.lookup(dirid, &dirops.name).await {
        Ok(fid) => {
            let obj_attributes = nfs_option_from_result(context.vfs.getattr(fid).await);
            debug!("lookup success {} --> {:?}", xid, obj_attributes);
            LOOKUP3res::Ok(LOOKUP3resok {
                object: context.vfs.id_to_fh(fid),
                obj_attributes,
                dir_attributes,
            })
        }
        Err(stat) => {
            warn!("lookup error {}({:?}) --> {stat}", xid, dirops.name,);
            LOOKUP3res::Err((stat, LOOKUP3resfail { dir_attributes }))
        }
    }
}

async fn nfsproc3_read(context: &RPCContext, xid: u32, read3args: READ3args) -> READ3res<'static> {
    let handle = read3args.file;
    let id = match context.vfs.fh_to_id(&handle) {
        Ok(id) => id,
        Err(stat) => {
            warn!("read error {} --> {stat}", xid);
            return READ3res::Err((stat, READ3resfail::default()));
        }
    };

    let file_attributes = nfs_option_from_result(context.vfs.getattr(id).await);
    match context
        .vfs
        .read(id, read3args.offset, read3args.count)
        .await
    {
        Ok((bytes, eof)) => {
            debug!(" {} --> read {} bytes, eof: {eof}", xid, bytes.len());
            READ3res::Ok(READ3resok {
                file_attributes,
                count: u32::try_from(bytes.len()).expect("buffer is too big"),
                eof,
                data: Opaque::owned(bytes),
            })
        }
        Err(stat) => {
            error!("read error {} --> {stat}", xid);
            READ3res::Err((stat, READ3resfail { file_attributes }))
        }
    }
}

async fn nfsproc3_fsinfo(context: &RPCContext, xid: u32, args: FSINFO3args) -> FSINFO3res {
    let handle = args.fsroot;
    let id = match context.vfs.fh_to_id(&handle) {
        Ok(id) => id,
        Err(stat) => {
            warn!("fsinfo error {xid} --> {stat}");
            return FSINFO3res::Err((stat, FSINFO3resfail::default()));
        }
    };

    match context.vfs.fsinfo(id).await {
        Ok(fsinfo) => {
            debug!("fsinfo success {xid} --> {fsinfo:?}");
            FSINFO3res::Ok(fsinfo)
        }
        Err(stat) => {
            warn!("fsinfo error {xid} --> {stat}");
            FSINFO3res::Err((
                stat,
                FSINFO3resfail {
                    obj_attributes: post_op_attr::None,
                },
            ))
        }
    }
}

async fn nfsproc3_access(context: &RPCContext, xid: u32, args: ACCESS3args) -> ACCESS3res {
    let handle = args.object;
    let mut access = args.access;

    let id = match context.vfs.fh_to_id(&handle) {
        Ok(id) => id,
        Err(stat) => {
            warn!("access error {xid} --> {stat}");
            return ACCESS3res::Err((stat, ACCESS3resfail::default()));
        }
    };

    let obj_attributes = nfs_option_from_result(context.vfs.getattr(id).await);

    if !matches!(context.vfs.capabilities(), VFSCapabilities::ReadWrite) {
        access &= ACCESS3_READ | ACCESS3_LOOKUP;
    }

    debug!("access success {xid} --> {access:?}");
    ACCESS3res::Ok(ACCESS3resok {
        obj_attributes,
        access,
    })
}

async fn nfsproc3_pathconf(context: &RPCContext, xid: u32, args: PATHCONF3args) -> PATHCONF3res {
    let handle = args.object;
    debug!("nfsproc3_pathconf({xid}, {handle:?})");

    let id = match context.vfs.fh_to_id(&handle) {
        Ok(id) => id,
        Err(stat) => {
            warn!("pathconf error {xid} --> {stat}");
            return PATHCONF3res::Err((stat, PATHCONF3resfail::default()));
        }
    };

    let obj_attr = nfs_option_from_result(context.vfs.getattr(id).await);

    let res = PATHCONF3resok {
        obj_attributes: obj_attr,
        linkmax: 0,
        name_max: 32768,
        no_trunc: true,
        chown_restricted: true,
        case_insensitive: false,
        case_preserving: true,
    };

    debug!("pathconf success {xid} --> {res:?}");
    PATHCONF3res::Ok(res)
}

async fn nfsproc3_fsstat(context: &RPCContext, xid: u32, args: FSSTAT3args) -> FSSTAT3res {
    let handle = args.fsroot;
    let id = match context.vfs.fh_to_id(&handle) {
        Ok(id) => id,
        Err(stat) => {
            warn!("fsstat error {xid} --> {stat}");
            return FSSTAT3res::Err((stat, FSSTAT3resfail::default()));
        }
    };

    let obj_attr = nfs_option_from_result(context.vfs.getattr(id).await);
    let fsstat = FSSTAT3resok {
        obj_attributes: obj_attr,
        tbytes: TEBIBYTE,
        fbytes: TEBIBYTE,
        abytes: TEBIBYTE,
        tfiles: GIBIBYTE,
        ffiles: GIBIBYTE,
        afiles: GIBIBYTE,
        invarsec: u32::MAX,
    };

    debug!("fsstat success {xid} --> {fsstat:?}");
    FSSTAT3res::Ok(fsstat)
}

async fn nfsproc3_readdirplus(
    context: &RPCContext,
    xid: u32,
    args: READDIRPLUS3args,
) -> READDIRPLUS3res<'static> {
    let dirid = context.vfs.fh_to_id(&args.dir);
    // fail if unable to convert file handle
    if let Err(stat) = dirid {
        return READDIRPLUS3res::Err((stat, READDIRPLUS3resfail::default()));
    }
    let dirid = dirid.unwrap();
    let dir_attr_maybe = context.vfs.getattr(dirid).await;

    let dir_attributes = dir_attr_maybe.map_or(post_op_attr::None, post_op_attr::Some);

    let dirversion = cookieverf3::from_attr(&dir_attributes);
    debug!(" -- Dir attr {dir_attributes:?}");
    debug!(" -- Dir version {dirversion:?}");
    let has_version = args.cookieverf.is_some();
    // initial call should have empty cookie verf
    // subsequent calls should have cvf_version as defined above
    // which is based off the mtime.
    //
    // TODO: This is *far* too aggressive. and unnecessary.
    // The client should maintain this correctly typically.
    //
    // The way cookieverf is handled is quite interesting...
    //
    // There are 2 notes in the RFC of interest:
    // 1. If the
    // server detects that the cookie is no longer valid, the
    // server will reject the READDIR request with the status,
    // NFS3ERR_BAD_COOKIE. The client should be careful to
    // avoid holding directory entry cookies across operations
    // that modify the directory contents, such as REMOVE and
    // CREATE.
    //
    // 2. One implementation of the cookie-verifier mechanism might
    //  be for the server to use the modification time of the
    //  directory. This might be overly restrictive, however. A
    //  better approach would be to record the time of the last
    //  directory modification that changed the directory
    //  organization in a way that would make it impossible to
    //  reliably interpret a cookie. Servers in which directory
    //  cookies are always valid are free to use zero as the
    //  verifier always.
    //
    //  Basically, as long as the cookie is "kinda" intepretable,
    //  we should keep accepting it.
    //  On testing, the Mac NFS client pretty much expects that
    //  especially on highly concurrent modifications to the directory.
    //
    //  1. If part way through a directory enumeration we fail with BAD_COOKIE
    //  if the directory contents change, the client listing may fail resulting
    //  in a "no such file or directory" error.
    //  2. if we cache readdir results. i.e. we think of a readdir as two parts a. enumerating
    //     everything first b. the cookie is then used to paginate the enumeration we can run into
    //     file time synchronization issues. i.e. while one listing occurs and another file is
    //     touched, the listing may report an outdated file status.
    //
    //     This cache also appears to have to be *quite* long lasting
    //     as the client may hold on to a directory enumerator
    //     with unbounded time.
    //
    //  Basically, if we think about how linux directory listing works
    //  is that you just get an enumerator. There is no mechanic available for
    //  "restarting" a pagination and this enumerator is assumed to be valid
    //  even across directory modifications and should reflect changes
    //  immediately.
    //
    //  The best solution is simply to really completely avoid sending
    //  BAD_COOKIE all together and to ignore the cookie mechanism.
    //
    // if args.cookieverf != cookieverf3::default() && args.cookieverf != dirversion {
    // info!(" -- Dir version mismatch. Received {:?}", args.cookieverf);
    // make_success_reply(xid).pack(output)?;
    // nfsstat3::NFS3ERR_BAD_COOKIE.pack(output)?;
    // dir_attr.pack(output)?;
    // return Ok(());
    // }

    // subtract off the final entryplus* field (which must be false) and the eof
    if args.maxcount < 128 {
        // we have no space to write anything
        let stat = nfsstat3::NFS3ERR_TOOSMALL;
        error!("readdirplus error {xid} --> {stat}");
        return READDIRPLUS3res::Err((stat, READDIRPLUS3resfail { dir_attributes }));
    }
    let max_bytes_allowed = args.maxcount as usize - 128;

    let iter = context.vfs.readdirplus(dirid, args.cookie).await;

    if let Err(stat) = iter {
        error!("readdirplus error {xid} --> {stat}");
        return READDIRPLUS3res::Err((stat, READDIRPLUS3resfail { dir_attributes }));
    }

    let mut iter = iter.unwrap();
    let eof;

    // this is a wrapper around a writer that also just counts the number of bytes
    // written
    let mut entries_result = BoundedEntryPlusList::new(args.dircount as usize, max_bytes_allowed);
    loop {
        match iter.next().await {
            NextResult::Ok(mut entry) => {
                if entry.name_handle.is_none() {
                    entry.name_handle = post_op_fh3::Some(context.vfs.id_to_fh(dirid));
                }
                let result = entries_result.try_push(entry);
                if result.is_err() {
                    trace!(" -- insufficient space. truncating");
                    eof = false;
                    break;
                }
            }
            NextResult::Eof => {
                eof = true;
                break;
            }
            NextResult::Err(stat) => {
                error!("readdirplus error {xid} --> {stat}");
                return READDIRPLUS3res::Err((stat, READDIRPLUS3resfail { dir_attributes }));
            }
        }
    }

    let entries = entries_result.into_inner();
    if entries.0.is_empty() && !eof {
        let stat = nfsstat3::NFS3ERR_TOOSMALL;
        error!("readdirplus error {xid} --> {stat}");
        return READDIRPLUS3res::Err((stat, READDIRPLUS3resfail { dir_attributes }));
    }

    debug!("  -- readdirplus eof {eof}");
    debug!(
        "readdirplus {dirid}, has_version {has_version}, start at {}, flushing {} entries, \
         complete {eof}",
        args.cookie,
        entries.0.len()
    );

    READDIRPLUS3res::Ok(READDIRPLUS3resok {
        dir_attributes,
        cookieverf: dirversion,
        reply: dirlistplus3 { entries, eof },
    })
}

#[allow(clippy::too_many_lines)]
async fn nfsproc3_readdir(
    context: &RPCContext,
    xid: u32,
    readdir3args: READDIR3args,
) -> READDIR3res<'static> {
    let dirid = context.vfs.fh_to_id(&readdir3args.dir);
    // fail if unable to convert file handle
    if let Err(stat) = dirid {
        return READDIR3res::Err((stat, READDIR3resfail::default()));
    }

    let dirid = dirid.unwrap();
    let dir_attr_maybe = context.vfs.getattr(dirid).await;
    let dir_attributes = dir_attr_maybe.map_or(post_op_attr::None, post_op_attr::Some);
    let cookieverf = cookieverf3::from_attr(&dir_attributes);

    if readdir3args.cookieverf.is_none() {
        if readdir3args.cookie != 0 {
            warn!(
                " -- Invalid cookie. Expected 0, got {}",
                readdir3args.cookie
            );
            return READDIR3res::Err((nfsstat3::NFS3ERR_BAD_COOKIE, READDIR3resfail::default()));
        }
        debug!(" -- Start of readdir");
    } else if readdir3args.cookieverf != cookieverf {
        warn!(
            " -- Dir version mismatch. Received {:?}, Expected: {:?}",
            readdir3args.cookieverf, cookieverf
        );
        return READDIR3res::Err((nfsstat3::NFS3ERR_BAD_COOKIE, READDIR3resfail::default()));
    } else {
        debug!(" -- Resuming readdir. Cookie {}", readdir3args.cookie);
    }

    debug!(" -- Dir attr {dir_attributes:?}");
    debug!(" -- Dir version {cookieverf:?}");

    let mut resok = READDIR3resok {
        dir_attributes,
        cookieverf,
        reply: dirlist3::default(),
    };

    let empty_len = xid.packed_size() + resok.packed_size();
    if empty_len > readdir3args.count as usize {
        // we have no space to write anything
        return READDIR3res::Err((
            nfsstat3::NFS3ERR_TOOSMALL,
            READDIR3resfail {
                dir_attributes: resok.dir_attributes,
            },
        ));
    }
    let max_bytes_allowed = readdir3args.count as usize - empty_len;

    let iter = context.vfs.readdir(dirid, readdir3args.cookie).await;
    if let Err(stat) = iter {
        return READDIR3res::Err((
            stat,
            READDIR3resfail {
                dir_attributes: resok.dir_attributes,
            },
        ));
    }

    let mut iter = iter.unwrap();
    let mut entries = BoundedList::new(max_bytes_allowed);
    let eof;
    loop {
        match iter.next().await {
            NextResult::Ok(entry) => {
                let result = entries.try_push(entry);
                if result.is_err() {
                    trace!(" -- insufficient space. truncating");
                    eof = false;
                    break;
                }
            }
            NextResult::Eof => {
                eof = true;
                break;
            }
            NextResult::Err(stat) => {
                error!("readdir error {xid} --> {stat}");
                return READDIR3res::Err((
                    stat,
                    READDIR3resfail {
                        dir_attributes: resok.dir_attributes,
                    },
                ));
            }
        }
    }

    let entries = entries.into_inner();
    if entries.is_empty() && !eof {
        let stat = nfsstat3::NFS3ERR_TOOSMALL;
        error!("readdir error {xid} --> {stat}");
        return READDIR3res::Err((
            stat,
            READDIR3resfail {
                dir_attributes: resok.dir_attributes,
            },
        ));
    }

    resok.reply.entries = entries;
    resok.reply.eof = eof;
    Nfs3Result::Ok(resok)
}

async fn nfsproc3_write(context: &RPCContext, xid: u32, write3args: WRITE3args<'_>) -> WRITE3res {
    if !matches!(context.vfs.capabilities(), VFSCapabilities::ReadWrite) {
        warn!("No write capabilities.");
        return WRITE3res::Err((nfsstat3::NFS3ERR_ROFS, WRITE3resfail::default()));
    }

    if write3args.data.len() != write3args.count as usize {
        error!(
            "Data length mismatch: expected {}, got {}",
            write3args.count,
            write3args.data.len()
        );
        return WRITE3res::Err((nfsstat3::NFS3ERR_INVAL, WRITE3resfail::default()));
    }

    let id = match context.vfs.fh_to_id(&write3args.file) {
        Ok(id) => id,
        Err(stat) => {
            warn!("write error {xid} --> {stat}");
            return WRITE3res::Err((stat, WRITE3resfail::default()));
        }
    };

    let before = context
        .vfs
        .getattr(id)
        .await
        .map_or(pre_op_attr::None, |v| {
            pre_op_attr::Some(wcc_attr {
                size: v.size,
                mtime: v.mtime,
                ctime: v.ctime,
            })
        });

    match context
        .vfs
        .write(id, write3args.offset, &write3args.data)
        .await
    {
        Ok(fattr) => {
            debug!("write success {xid} --> {fattr:?}");
            WRITE3res::Ok(WRITE3resok {
                file_wcc: wcc_data {
                    before,
                    after: post_op_attr::Some(fattr),
                },
                count: write3args.count,
                committed: stable_how::FILE_SYNC,
                verf: writeverf3(context.vfs.serverid().0),
            })
        }
        Err(stat) => {
            error!("write error {xid} --> {stat}");
            WRITE3res::Err((
                stat,
                WRITE3resfail {
                    file_wcc: wcc_data {
                        before,
                        after: post_op_attr::None,
                    },
                },
            ))
        }
    }
}

#[allow(clippy::collapsible_if, clippy::too_many_lines)]
pub async fn nfsproc3_create<'a>(
    context: &RPCContext,
    xid: u32,
    args: CREATE3args<'a>,
) -> CREATE3res {
    if !matches!(context.vfs.capabilities(), VFSCapabilities::ReadWrite) {
        warn!("No write capabilities.");
        return CREATE3res::Err((nfsstat3::NFS3ERR_ROFS, CREATE3resfail::default()));
    }

    let dirops = args.where_;
    let createhow = args.how;

    debug!("nfsproc3_create({xid}, {dirops:?}, {createhow:?})");

    let dirid = match context.vfs.fh_to_id(&dirops.dir) {
        Ok(dirid) => dirid,
        Err(stat) => {
            warn!("create error {xid} --> {stat}");
            return CREATE3res::Err((stat, CREATE3resfail::default()));
        }
    };

    // get the object attributes before the write
    let before = match context.vfs.getattr(dirid).await {
        Ok(v) => pre_op_attr::Some(wcc_attr {
            size: v.size,
            mtime: v.mtime,
            ctime: v.ctime,
        }),
        Err(stat) => {
            warn!("Cannot stat directory {xid} -> {stat}");
            return CREATE3res::Err((stat, CREATE3resfail::default()));
        }
    };

    if matches!(&createhow, createhow3::GUARDED(_)) {
        if context.vfs.lookup(dirid, &dirops.name).await.is_ok() {
            let after = nfs_option_from_result(context.vfs.getattr(dirid).await);
            return CREATE3res::Err((
                nfsstat3::NFS3ERR_EXIST,
                CREATE3resfail {
                    dir_wcc: wcc_data { before, after },
                },
            ));
        }
    }

    let (fid, postopattr) = match createhow {
        createhow3::EXCLUSIVE(_) => {
            let fid = context.vfs.create_exclusive(dirid, &dirops.name).await;
            (fid, post_op_attr::None)
        }
        createhow3::UNCHECKED(target_attributes) | createhow3::GUARDED(target_attributes) => {
            match context
                .vfs
                .create(dirid, &dirops.name, target_attributes)
                .await
            {
                Ok((fid, fattr)) => (Ok(fid), post_op_attr::Some(fattr)),
                Err(e) => (Err(e), post_op_attr::None),
            }
        }
    };

    let after = nfs_option_from_result(context.vfs.getattr(dirid).await);
    let dir_wcc = wcc_data { before, after };

    match fid {
        Ok(fid) => {
            debug!("create success {xid} --> {fid:?}, {postopattr:?}");
            CREATE3res::Ok(CREATE3resok {
                obj: post_op_fh3::Some(context.vfs.id_to_fh(fid)),
                obj_attributes: postopattr,
                dir_wcc,
            })
        }
        Err(stat) => {
            error!("create error {xid} --> {stat}");
            CREATE3res::Err((stat, CREATE3resfail { dir_wcc }))
        }
    }
}
pub async fn nfsproc3_setattr(context: &RPCContext, xid: u32, args: SETATTR3args) -> SETATTR3res {
    if !matches!(context.vfs.capabilities(), VFSCapabilities::ReadWrite) {
        warn!("No write capabilities.");
        return SETATTR3res::Err((nfsstat3::NFS3ERR_ROFS, SETATTR3resfail::default()));
    }

    let id = match context.vfs.fh_to_id(&args.object) {
        Ok(id) => id,
        Err(stat) => {
            warn!("setattr error {xid} --> {stat}");
            return SETATTR3res::Err((stat, SETATTR3resfail::default()));
        }
    };

    let ctime;
    let before = match context.vfs.getattr(id).await {
        Ok(v) => {
            let wccattr = wcc_attr {
                size: v.size,
                mtime: v.mtime,
                ctime: v.ctime.clone(),
            };
            ctime = v.ctime;
            pre_op_attr::Some(wccattr)
        }
        Err(stat) => {
            warn!("Cannot stat object {xid} --> {stat}");
            return SETATTR3res::Err((stat, SETATTR3resfail::default()));
        }
    };

    if let sattrguard3::Some(guard) = args.guard {
        if guard != ctime {
            warn!("setattr guard mismatch {xid}");
            return SETATTR3res::Err((
                nfsstat3::NFS3ERR_NOT_SYNC,
                SETATTR3resfail {
                    obj_wcc: wcc_data {
                        before,
                        after: post_op_attr::None,
                    },
                },
            ));
        }
    }

    match context.vfs.setattr(id, args.new_attributes).await {
        Ok(post_op_attr) => {
            debug!("setattr success {xid} --> {post_op_attr:?}");
            SETATTR3res::Ok(SETATTR3resok {
                obj_wcc: wcc_data {
                    before,
                    after: post_op_attr::Some(post_op_attr),
                },
            })
        }
        Err(stat) => {
            error!("setattr error {xid} --> {stat}");
            SETATTR3res::Err((
                stat,
                SETATTR3resfail {
                    obj_wcc: wcc_data {
                        before,
                        after: post_op_attr::None,
                    },
                },
            ))
        }
    }
}

pub async fn nfsproc3_remove(context: &RPCContext, xid: u32, args: REMOVE3args<'_>) -> REMOVE3res {
    if !matches!(context.vfs.capabilities(), VFSCapabilities::ReadWrite) {
        warn!("No write capabilities.");
        return REMOVE3res::Err((nfsstat3::NFS3ERR_ROFS, REMOVE3resfail::default()));
    }

    let dirid = match context.vfs.fh_to_id(&args.object.dir) {
        Ok(dirid) => dirid,
        Err(stat) => {
            warn!("remove error {xid} --> {stat}");
            return REMOVE3res::Err((stat, REMOVE3resfail::default()));
        }
    };

    let before = match context.vfs.getattr(dirid).await {
        Ok(v) => pre_op_attr::Some(wcc_attr {
            size: v.size,
            mtime: v.mtime,
            ctime: v.ctime,
        }),
        Err(stat) => {
            warn!("Cannot stat directory {xid} -> {stat}");
            return REMOVE3res::Err((stat, REMOVE3resfail::default()));
        }
    };

    match context.vfs.remove(dirid, &args.object.name).await {
        Ok(()) => {
            let after = nfs_option_from_result(context.vfs.getattr(dirid).await);
            debug!("remove success {xid}");
            REMOVE3res::Ok(REMOVE3resok {
                dir_wcc: wcc_data { before, after },
            })
        }
        Err(stat) => {
            let after = nfs_option_from_result(context.vfs.getattr(dirid).await);
            error!("remove error {xid} --> {stat}");
            REMOVE3res::Err((
                stat,
                REMOVE3resfail {
                    dir_wcc: wcc_data { before, after },
                },
            ))
        }
    }
}

pub async fn nfsproc3_rename(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    // if we do not have write capabilities
    if !matches!(context.vfs.capabilities(), VFSCapabilities::ReadWrite) {
        warn!("No write capabilities.");
        make_success_reply(xid).pack(output)?;
        nfsstat3::NFS3ERR_ROFS.pack(output)?;
        wcc_data::default().pack(output)?;
        return Ok(());
    }

    let fromdirops = diropargs3::unpack(input)?.0;
    let todirops = diropargs3::unpack(input)?.0;

    debug!(
        "nfsproc3_rename({:?}, {:?}, {:?}) ",
        xid, fromdirops, todirops
    );

    // find the from directory
    let from_dirid = context.vfs.fh_to_id(&fromdirops.dir);
    if let Err(stat) = from_dirid {
        // directory does not exist
        make_success_reply(xid).pack(output)?;
        stat.pack(output)?;
        wcc_data::default().pack(output)?;
        error!("Directory does not exist");
        return Ok(());
    }

    // find the to directory
    let to_dirid = context.vfs.fh_to_id(&todirops.dir);
    if let Err(stat) = to_dirid {
        // directory does not exist
        make_success_reply(xid).pack(output)?;
        stat.pack(output)?;
        wcc_data::default().pack(output)?;
        error!("Directory does not exist");
        return Ok(());
    }

    // found the directory, get the attributes
    let from_dirid = from_dirid.unwrap();
    let to_dirid = to_dirid.unwrap();

    // get the object attributes before the write
    let pre_from_dir_attr = match context.vfs.getattr(from_dirid).await {
        Ok(v) => {
            let wccattr = wcc_attr {
                size: v.size,
                mtime: v.mtime,
                ctime: v.ctime,
            };
            pre_op_attr::Some(wccattr)
        }
        Err(stat) => {
            error!("Cannot stat directory");
            make_success_reply(xid).pack(output)?;
            stat.pack(output)?;
            wcc_data::default().pack(output)?;
            return Ok(());
        }
    };

    // get the object attributes before the write
    let pre_to_dir_attr = match context.vfs.getattr(to_dirid).await {
        Ok(v) => {
            let wccattr = wcc_attr {
                size: v.size,
                mtime: v.mtime,
                ctime: v.ctime,
            };
            pre_op_attr::Some(wccattr)
        }
        Err(stat) => {
            error!("Cannot stat directory");
            make_success_reply(xid).pack(output)?;
            stat.pack(output)?;
            wcc_data::default().pack(output)?;
            return Ok(());
        }
    };

    // rename!
    let res = context
        .vfs
        .rename(from_dirid, &fromdirops.name, to_dirid, &todirops.name)
        .await;

    // Re-read dir attributes for post op attr
    let post_from_dir_attr = nfs_option_from_result(context.vfs.getattr(from_dirid).await);
    let post_to_dir_attr = nfs_option_from_result(context.vfs.getattr(to_dirid).await);
    let from_wcc_res = wcc_data {
        before: pre_from_dir_attr,
        after: post_from_dir_attr,
    };

    let to_wcc_res = wcc_data {
        before: pre_to_dir_attr,
        after: post_to_dir_attr,
    };

    match res {
        Ok(()) => {
            debug!("rename success");
            make_success_reply(xid).pack(output)?;
            nfsstat3::NFS3_OK.pack(output)?;
            from_wcc_res.pack(output)?;
            to_wcc_res.pack(output)?;
        }
        Err(e) => {
            error!("rename error {:?} --> {:?}", xid, e);
            // serialize CREATE3resfail
            make_success_reply(xid).pack(output)?;
            e.pack(output)?;
            from_wcc_res.pack(output)?;
            to_wcc_res.pack(output)?;
        }
    }

    Ok(())
}

pub async fn nfsproc3_mkdir(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    // if we do not have write capabilities
    if !matches!(context.vfs.capabilities(), VFSCapabilities::ReadWrite) {
        warn!("No write capabilities.");
        make_success_reply(xid).pack(output)?;
        nfsstat3::NFS3ERR_ROFS.pack(output)?;
        wcc_data::default().pack(output)?;
        return Ok(());
    }
    let args = MKDIR3args::unpack(input)?.0;

    debug!("nfsproc3_mkdir({:?}, {:?}) ", xid, args);

    // find the directory we are supposed to create the
    // new file in
    let dirid = context.vfs.fh_to_id(&args.where_.dir);
    if let Err(stat) = dirid {
        // directory does not exist
        make_success_reply(xid).pack(output)?;
        stat.pack(output)?;
        wcc_data::default().pack(output)?;
        error!("Directory does not exist");
        return Ok(());
    }
    // found the directory, get the attributes
    let dirid = dirid.unwrap();

    // get the object attributes before the write
    let pre_dir_attr = match context.vfs.getattr(dirid).await {
        Ok(v) => {
            let wccattr = wcc_attr {
                size: v.size,
                mtime: v.mtime,
                ctime: v.ctime,
            };
            pre_op_attr::Some(wccattr)
        }
        Err(stat) => {
            error!("Cannot stat directory");
            make_success_reply(xid).pack(output)?;
            stat.pack(output)?;
            wcc_data::default().pack(output)?;
            return Ok(());
        }
    };

    let res = context.vfs.mkdir(dirid, &args.where_.name).await;

    // Re-read dir attributes for post op attr
    let post_dir_attr = nfs_option_from_result(context.vfs.getattr(dirid).await);
    let wcc_res = wcc_data {
        before: pre_dir_attr,
        after: post_dir_attr,
    };

    match res {
        Ok((fid, fattr)) => {
            debug!("mkdir success --> {:?}, {:?}", fid, fattr);
            make_success_reply(xid).pack(output)?;
            nfsstat3::NFS3_OK.pack(output)?;
            // serialize CREATE3resok
            let fh = context.vfs.id_to_fh(fid);
            post_op_fh3::Some(fh).pack(output)?;
            post_op_attr::Some(fattr).pack(output)?;
            wcc_res.pack(output)?;
        }
        Err(e) => {
            debug!("mkdir error {:?} --> {:?}", xid, e);
            // serialize CREATE3resfail
            make_success_reply(xid).pack(output)?;
            e.pack(output)?;
            wcc_res.pack(output)?;
        }
    }

    Ok(())
}

pub async fn nfsproc3_symlink(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    // if we do not have write capabilities
    if !matches!(context.vfs.capabilities(), VFSCapabilities::ReadWrite) {
        warn!("No write capabilities.");
        make_success_reply(xid).pack(output)?;
        nfsstat3::NFS3ERR_ROFS.pack(output)?;
        wcc_data::default().pack(output)?;
        return Ok(());
    }
    let args = SYMLINK3args::unpack(input)?.0;

    debug!("nfsproc3_symlink({:?}, {:?}) ", xid, args);

    // find the directory we are supposed to create the
    // new file in
    let dirid = context.vfs.fh_to_id(&args.where_.dir);
    if let Err(stat) = dirid {
        // directory does not exist
        make_success_reply(xid).pack(output)?;
        stat.pack(output)?;
        wcc_data::default().pack(output)?;
        error!("Directory does not exist");
        return Ok(());
    }
    // found the directory, get the attributes
    let dirid = dirid.unwrap();

    // get the object attributes before the write
    let pre_dir_attr = match context.vfs.getattr(dirid).await {
        Ok(v) => {
            let wccattr = wcc_attr {
                size: v.size,
                mtime: v.mtime,
                ctime: v.ctime,
            };
            pre_op_attr::Some(wccattr)
        }
        Err(stat) => {
            error!("Cannot stat directory");
            make_success_reply(xid).pack(output)?;
            stat.pack(output)?;
            wcc_data::default().pack(output)?;
            return Ok(());
        }
    };

    let res = context
        .vfs
        .symlink(
            dirid,
            &args.where_.name,
            &args.symlink.symlink_data,
            &args.symlink.symlink_attributes,
        )
        .await;

    // Re-read dir attributes for post op attr
    let post_dir_attr = nfs_option_from_result(context.vfs.getattr(dirid).await);
    let wcc_res = wcc_data {
        before: pre_dir_attr,
        after: post_dir_attr,
    };

    match res {
        Ok((fid, fattr)) => {
            debug!("symlink success --> {:?}, {:?}", fid, fattr);
            make_success_reply(xid).pack(output)?;
            nfsstat3::NFS3_OK.pack(output)?;
            // serialize CREATE3resok
            let fh = context.vfs.id_to_fh(fid);
            post_op_fh3::Some(fh).pack(output)?;
            post_op_attr::Some(fattr).pack(output)?;
            wcc_res.pack(output)?;
        }
        Err(e) => {
            debug!("symlink error --> {:?}", e);
            // serialize CREATE3resfail
            make_success_reply(xid).pack(output)?;
            e.pack(output)?;
            wcc_res.pack(output)?;
        }
    }

    Ok(())
}

pub async fn nfsproc3_readlink(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let handle = nfs_fh3::unpack(input)?.0;
    debug!("nfsproc3_readlink({:?},{:?}) ", xid, handle);

    let id = context.vfs.fh_to_id(&handle);
    // fail if unable to convert file handle
    let id = match id {
        Ok(id) => id,
        Err(stat) => {
            make_success_reply(xid).pack(output)?;
            stat.pack(output)?;
            post_op_attr::None.pack(output)?;
            return Ok(());
        }
    };
    // if the id does not exist, we fail
    let symlink_attr = match context.vfs.getattr(id).await {
        Ok(v) => post_op_attr::Some(v),
        Err(stat) => {
            make_success_reply(xid).pack(output)?;
            stat.pack(output)?;
            post_op_attr::None.pack(output)?;
            return Ok(());
        }
    };
    match context.vfs.readlink(id).await {
        Ok(path) => {
            debug!(" {:?} --> {:?}", xid, path);
            make_success_reply(xid).pack(output)?;
            nfsstat3::NFS3_OK.pack(output)?;
            symlink_attr.pack(output)?;
            path.pack(output)?;
        }
        Err(stat) => {
            // failed to read link
            // retry with failure and the post_op_attr
            make_success_reply(xid).pack(output)?;
            stat.pack(output)?;
            symlink_attr.pack(output)?;
        }
    }
    Ok(())
}

fn nfs_option_from_result<T, E>(result: Result<T, E>) -> Nfs3Option<T> {
    result.map_or(Nfs3Option::None, Nfs3Option::Some)
}
