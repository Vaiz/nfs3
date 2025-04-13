#![allow(clippy::unwrap_used)] // FIXME: will fix this after some refactoring

use std::io::Cursor;

use nfs3_types::nfs3::*;
use nfs3_types::rpc::accept_stat_data;
use nfs3_types::xdr_codec::{BoundedList, Opaque, Pack, PackedSize, Unpack, Void};
use tracing::{debug, error, trace, warn};

use crate::context::RPCContext;
use crate::nfs_ext::{BoundedEntryPlusList, CookieVerfExt};
use crate::rpcwire::HandleResult;
use crate::rpcwire::messages::{IncomingRpcMessage, OutgoingRpcMessage};
use crate::units::{GIBIBYTE, TEBIBYTE};
use crate::vfs::{NextResult, VFSCapabilities};

pub async fn handle_nfs(
    context: &RPCContext,
    message: IncomingRpcMessage,
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

    debug!("{proc}({})", message.xid());
    match proc {
        NFSPROC3_NULL => handle(context, message, nfsproc3_null).await,
        NFSPROC3_GETATTR => handle(context, message, nfsproc3_getattr).await,
        NFSPROC3_LOOKUP => handle(context, message, nfsproc3_lookup).await,
        NFSPROC3_READ => handle(context, message, nfsproc3_read).await,
        NFSPROC3_FSINFO => handle(context, message, nfsproc3_fsinfo).await,
        NFSPROC3_ACCESS => handle(context, message, nfsproc3_access).await,
        NFSPROC3_PATHCONF => handle(context, message, nfsproc3_pathconf).await,
        NFSPROC3_FSSTAT => handle(context, message, nfsproc3_fsstat).await,
        NFSPROC3_READDIR => handle(context, message, nfsproc3_readdir).await,
        NFSPROC3_READDIRPLUS => handle(context, message, nfsproc3_readdirplus).await,
        NFSPROC3_WRITE => handle(context, message, nfsproc3_write).await,
        NFSPROC3_CREATE => handle(context, message, nfsproc3_create).await,
        NFSPROC3_SETATTR => handle(context, message, nfsproc3_setattr).await,
        NFSPROC3_REMOVE | NFSPROC3_RMDIR => handle(context, message, nfsproc3_remove).await,
        NFSPROC3_RENAME => handle(context, message, nfsproc3_rename).await,
        NFSPROC3_MKDIR => handle(context, message, nfsproc3_mkdir).await,
        NFSPROC3_SYMLINK => handle(context, message, nfsproc3_symlink).await,
        NFSPROC3_READLINK => handle(context, message, nfsproc3_readlink).await,
        NFSPROC3_MKNOD | NFSPROC3_LINK | NFSPROC3_COMMIT => {
            warn!("Unimplemented message {proc}");
            message
                .into_error_reply(accept_stat_data::PROC_UNAVAIL)
                .try_into()
        }
    }
}

async fn handle<'a, I, O>(
    context: &RPCContext,
    mut message: IncomingRpcMessage,
    handler: impl AsyncFnOnce(&RPCContext, u32, I) -> O,
) -> anyhow::Result<HandleResult>
where
    I: Unpack<Cursor<Vec<u8>>>,
    O: Pack<Cursor<&'static mut [u8]>> + PackedSize + Send + 'static,
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
async fn nfsproc3_create<'a>(context: &RPCContext, xid: u32, args: CREATE3args<'a>) -> CREATE3res {
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

async fn nfsproc3_setattr(context: &RPCContext, xid: u32, args: SETATTR3args) -> SETATTR3res {
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

async fn nfsproc3_remove(context: &RPCContext, xid: u32, args: REMOVE3args<'_>) -> REMOVE3res {
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

async fn nfsproc3_rename(context: &RPCContext, xid: u32, args: RENAME3args<'_, '_>) -> RENAME3res {
    if !matches!(context.vfs.capabilities(), VFSCapabilities::ReadWrite) {
        warn!("No write capabilities.");
        return RENAME3res::Err((nfsstat3::NFS3ERR_ROFS, RENAME3resfail::default()));
    }

    let from_dirid = match context.vfs.fh_to_id(&args.from.dir) {
        Ok(from_dirid) => from_dirid,
        Err(stat) => {
            warn!("rename error {xid} --> {stat}");
            return RENAME3res::Err((stat, RENAME3resfail::default()));
        }
    };

    let to_dirid = match context.vfs.fh_to_id(&args.to.dir) {
        Ok(to_dirid) => to_dirid,
        Err(stat) => {
            warn!("rename error {xid} --> {stat}");
            return RENAME3res::Err((stat, RENAME3resfail::default()));
        }
    };

    let pre_from_dir_attr = match context.vfs.getattr(from_dirid).await {
        Ok(v) => pre_op_attr::Some(wcc_attr {
            size: v.size,
            mtime: v.mtime,
            ctime: v.ctime,
        }),
        Err(stat) => {
            warn!("Cannot stat source directory {xid} --> {stat}");
            return RENAME3res::Err((stat, RENAME3resfail::default()));
        }
    };

    let pre_to_dir_attr = match context.vfs.getattr(to_dirid).await {
        Ok(v) => pre_op_attr::Some(wcc_attr {
            size: v.size,
            mtime: v.mtime,
            ctime: v.ctime,
        }),
        Err(stat) => {
            warn!("Cannot stat target directory {xid} --> {stat}");
            return RENAME3res::Err((stat, RENAME3resfail::default()));
        }
    };

    let result = context
        .vfs
        .rename(from_dirid, &args.from.name, to_dirid, &args.to.name)
        .await;

    let post_from_dir_attr = nfs_option_from_result(context.vfs.getattr(from_dirid).await);
    let post_to_dir_attr = nfs_option_from_result(context.vfs.getattr(to_dirid).await);

    let fromdir_wcc = wcc_data {
        before: pre_from_dir_attr,
        after: post_from_dir_attr,
    };
    let todir_wcc = wcc_data {
        before: pre_to_dir_attr,
        after: post_to_dir_attr,
    };
    match result {
        Ok(()) => {
            debug!("rename success {xid}");
            RENAME3res::Ok(RENAME3resok {
                fromdir_wcc,
                todir_wcc,
            })
        }
        Err(stat) => {
            error!("rename error {xid} --> {stat}");
            RENAME3res::Err((
                stat,
                RENAME3resfail {
                    fromdir_wcc,
                    todir_wcc,
                },
            ))
        }
    }
}
async fn nfsproc3_mkdir(context: &RPCContext, xid: u32, args: MKDIR3args<'_>) -> MKDIR3res {
    if !matches!(context.vfs.capabilities(), VFSCapabilities::ReadWrite) {
        warn!("No write capabilities.");
        return MKDIR3res::Err((nfsstat3::NFS3ERR_ROFS, MKDIR3resfail::default()));
    }

    let dirid = match context.vfs.fh_to_id(&args.where_.dir) {
        Ok(dirid) => dirid,
        Err(stat) => {
            warn!("mkdir error {xid} --> {stat}");
            return MKDIR3res::Err((stat, MKDIR3resfail::default()));
        }
    };

    let before = match context.vfs.getattr(dirid).await {
        Ok(v) => pre_op_attr::Some(wcc_attr {
            size: v.size,
            mtime: v.mtime,
            ctime: v.ctime,
        }),
        Err(stat) => {
            warn!("Cannot stat directory {xid} --> {stat}");
            return MKDIR3res::Err((stat, MKDIR3resfail::default()));
        }
    };

    let result = context.vfs.mkdir(dirid, &args.where_.name).await;
    let after = nfs_option_from_result(context.vfs.getattr(dirid).await);
    let dir_wcc = wcc_data { before, after };

    match result {
        Ok((fid, fattr)) => {
            debug!("mkdir success {xid} --> {fid:?}, {fattr:?}");
            MKDIR3res::Ok(MKDIR3resok {
                obj: post_op_fh3::Some(context.vfs.id_to_fh(fid)),
                obj_attributes: post_op_attr::Some(fattr),
                dir_wcc,
            })
        }
        Err(stat) => {
            error!("mkdir error {xid} --> {stat}");
            MKDIR3res::Err((stat, MKDIR3resfail { dir_wcc }))
        }
    }
}

async fn nfsproc3_symlink(context: &RPCContext, xid: u32, args: SYMLINK3args<'_>) -> SYMLINK3res {
    if !matches!(context.vfs.capabilities(), VFSCapabilities::ReadWrite) {
        warn!("No write capabilities.");
        return SYMLINK3res::Err((nfsstat3::NFS3ERR_ROFS, SYMLINK3resfail::default()));
    }

    let dirid = match context.vfs.fh_to_id(&args.where_.dir) {
        Ok(dirid) => dirid,
        Err(stat) => {
            warn!("symlink error {xid} --> {stat}");
            return SYMLINK3res::Err((stat, SYMLINK3resfail::default()));
        }
    };

    let pre_dir_attr = match context.vfs.getattr(dirid).await {
        Ok(v) => pre_op_attr::Some(wcc_attr {
            size: v.size,
            mtime: v.mtime,
            ctime: v.ctime,
        }),
        Err(stat) => {
            warn!("Cannot stat directory {xid} --> {stat}");
            return SYMLINK3res::Err((stat, SYMLINK3resfail::default()));
        }
    };

    match context
        .vfs
        .symlink(
            dirid,
            &args.where_.name,
            &args.symlink.symlink_data,
            &args.symlink.symlink_attributes,
        )
        .await
    {
        Ok((fid, fattr)) => {
            debug!("symlink success {xid} --> {fid:?}, {fattr:?}");
            SYMLINK3res::Ok(SYMLINK3resok {
                obj: post_op_fh3::Some(context.vfs.id_to_fh(fid)),
                obj_attributes: post_op_attr::Some(fattr),
                dir_wcc: wcc_data {
                    before: pre_dir_attr,
                    after: nfs_option_from_result(context.vfs.getattr(dirid).await),
                },
            })
        }
        Err(stat) => {
            error!("symlink error {xid} --> {stat}");
            SYMLINK3res::Err((
                stat,
                SYMLINK3resfail {
                    dir_wcc: wcc_data {
                        before: pre_dir_attr,
                        after: nfs_option_from_result(context.vfs.getattr(dirid).await),
                    },
                },
            ))
        }
    }
}

async fn nfsproc3_readlink(
    context: &RPCContext,
    xid: u32,
    args: READLINK3args,
) -> READLINK3res<'static> {
    let id = match context.vfs.fh_to_id(&args.symlink) {
        Ok(id) => id,
        Err(stat) => {
            warn!("readlink error {xid} --> {stat}");
            return READLINK3res::Err((stat, READLINK3resfail::default()));
        }
    };

    let symlink_attributes = nfs_option_from_result(context.vfs.getattr(id).await);

    match context.vfs.readlink(id).await {
        Ok(data) => {
            debug!("readlink success {xid} --> {data:?}");
            READLINK3res::Ok(READLINK3resok {
                symlink_attributes,
                data: data.into_owned(),
            })
        }
        Err(stat) => {
            error!("readlink error {xid} --> {stat}");
            READLINK3res::Err((stat, READLINK3resfail { symlink_attributes }))
        }
    }
}

fn nfs_option_from_result<T, E>(result: Result<T, E>) -> Nfs3Option<T> {
    result.map_or(Nfs3Option::None, Nfs3Option::Some)
}
