use std::io::{Read, Write};

use nfs3_types::nfs3::{
    ACCESS3_LOOKUP, ACCESS3_READ, CREATE3args, FSSTAT3resok, GETATTR3args, GETATTR3res,
    GETATTR3resok, LOOKUP3args, LOOKUP3res, LOOKUP3resfail, LOOKUP3resok, MKDIR3args, NFS_PROGRAM,
    Nfs3Result, PATHCONF3resok, READ3args, READ3resok, READDIR3args, READDIR3res, READDIR3resfail,
    READDIR3resok, READDIRPLUS3args, READDIRPLUS3res, READDIRPLUS3resfail, READDIRPLUS3resok,
    SETATTR3args, SYMLINK3args, VERSION, WRITE3args, WRITE3resok, cookieverf3, createhow3,
    dirlist3, dirlistplus3, diropargs3, fileid3, nfs_fh3, nfsstat3, post_op_attr, post_op_fh3,
    pre_op_attr, sattrguard3, stable_how, wcc_attr, wcc_data, writeverf3,
};
use nfs3_types::rpc::call_body;
use nfs3_types::xdr_codec::{BoundedList, Opaque, Pack, PackedSize, Unpack};
use tracing::{debug, error, trace, warn};

use crate::context::RPCContext;
use crate::nfs_ext::{BoundedEntryPlusList, CookieVerfExt};
use crate::rpc::{
    garbage_args_reply_message, make_success_reply, proc_unavail_reply_message,
    prog_mismatch_reply_message,
};
use crate::vfs::{NextResult, VFSCapabilities};

pub async fn handle_nfs(
    xid: u32,
    call: call_body<'_>,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    debug!("handle_nfs({xid}, {call:?}");
    if call.vers != VERSION {
        warn!("Invalid NFS Version number {} != {}", call.vers, VERSION);
        prog_mismatch_reply_message(xid, VERSION).pack(output)?;
        return Ok(());
    }
    let proc = NFS_PROGRAM::try_from(call.proc);
    if proc.is_err() {
        warn!("invalid NFS3 Program number {}", call.proc);
        proc_unavail_reply_message(xid).pack(output)?;
        return Ok(());
    }

    let proc = proc.unwrap();

    match proc {
        NFS_PROGRAM::NFSPROC3_NULL => nfsproc3_null(xid, input, output)?,
        NFS_PROGRAM::NFSPROC3_GETATTR => nfsproc3_getattr(xid, input, output, context).await?,
        NFS_PROGRAM::NFSPROC3_LOOKUP => nfsproc3_lookup(xid, input, output, context).await?,
        NFS_PROGRAM::NFSPROC3_READ => nfsproc3_read(xid, input, output, context).await?,
        NFS_PROGRAM::NFSPROC3_FSINFO => nfsproc3_fsinfo(xid, input, output, context).await?,
        NFS_PROGRAM::NFSPROC3_ACCESS => nfsproc3_access(xid, input, output, context).await?,
        NFS_PROGRAM::NFSPROC3_PATHCONF => nfsproc3_pathconf(xid, input, output, context).await?,
        NFS_PROGRAM::NFSPROC3_FSSTAT => nfsproc3_fsstat(xid, input, output, context).await?,
        NFS_PROGRAM::NFSPROC3_READDIR => nfsproc3_readdir(xid, input, output, context).await?,
        NFS_PROGRAM::NFSPROC3_READDIRPLUS => {
            nfsproc3_readdirplus(xid, input, output, context).await?;
        }
        NFS_PROGRAM::NFSPROC3_WRITE => nfsproc3_write(xid, input, output, context).await?,
        NFS_PROGRAM::NFSPROC3_CREATE => nfsproc3_create(xid, input, output, context).await?,
        NFS_PROGRAM::NFSPROC3_SETATTR => nfsproc3_setattr(xid, input, output, context).await?,
        NFS_PROGRAM::NFSPROC3_REMOVE => nfsproc3_remove(xid, input, output, context).await?,
        NFS_PROGRAM::NFSPROC3_RMDIR => nfsproc3_remove(xid, input, output, context).await?,
        NFS_PROGRAM::NFSPROC3_RENAME => nfsproc3_rename(xid, input, output, context).await?,
        NFS_PROGRAM::NFSPROC3_MKDIR => nfsproc3_mkdir(xid, input, output, context).await?,
        NFS_PROGRAM::NFSPROC3_SYMLINK => nfsproc3_symlink(xid, input, output, context).await?,
        NFS_PROGRAM::NFSPROC3_READLINK => nfsproc3_readlink(xid, input, output, context).await?,
        _ => {
            warn!("Unimplemented message {:?}", proc);
            proc_unavail_reply_message(xid).pack(output)?;
        } /* NFSPROC3_MKNOD,
           * NFSPROC3_LINK,
           * NFSPROC3_COMMIT,
           * INVALID */
    }
    Ok(())
}

pub fn nfsproc3_null(
    xid: u32,
    _: &mut impl Read,
    output: &mut impl Write,
) -> Result<(), anyhow::Error> {
    debug!("nfsproc3_null({:?}) ", xid);
    let msg = make_success_reply(xid);
    debug!("\t{:?} --> {:?}", xid, msg);
    msg.pack(output)?;
    Ok(())
}

pub async fn nfsproc3_getattr(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let getattr3args = GETATTR3args::unpack(input)?.0;
    let getattr3res = getattr_impl(xid, getattr3args, context).await?;
    make_success_reply(xid).pack(output)?;
    getattr3res.pack(output)?;

    Ok(())
}

async fn getattr_impl(
    xid: u32,
    getattr3args: GETATTR3args,
    context: &RPCContext,
) -> anyhow::Result<GETATTR3res> {
    let handle = getattr3args.object;
    debug!("nfsproc3_getattr({},{:?}) ", xid, handle);

    let id = context.vfs.fh_to_id(&handle);
    // fail if unable to convert file handle
    if let Err(stat) = id {
        return Ok(GETATTR3res::Err((stat, ())));
    }
    let id = id.unwrap();
    match context.vfs.getattr(id).await {
        Ok(obj_attributes) => {
            debug!(" {} --> {:?}", xid, obj_attributes);
            Ok(GETATTR3res::Ok(GETATTR3resok { obj_attributes }))
        }
        Err(stat) => {
            error!("getattr error {} --> {:?}", xid, stat);
            Ok(GETATTR3res::Err((stat, ())))
        }
    }
}

pub async fn nfsproc3_lookup(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let lookup3args = LOOKUP3args::unpack(input)?.0;
    let lookup3res = lookup_impl(xid, lookup3args, context).await?;
    make_success_reply(xid).pack(output)?;
    lookup3res.pack(output)?;

    Ok(())
}

async fn lookup_impl(
    xid: u32,
    lookup3args: LOOKUP3args<'_>,
    context: &RPCContext,
) -> anyhow::Result<LOOKUP3res> {
    let dirops = lookup3args.what;
    debug!("nfsproc3_lookup({:?},{:?}) ", xid, dirops);

    let dirid = context.vfs.fh_to_id(&dirops.dir);
    // fail if unable to convert file handle
    if let Err(stat) = dirid {
        return Ok(LOOKUP3res::Err((stat, LOOKUP3resfail::default())));
    }

    let dirid = dirid.unwrap();
    let dir_attributes = match context.vfs.getattr(dirid).await {
        Ok(v) => post_op_attr::Some(v),
        Err(_) => post_op_attr::None,
    };
    match context.vfs.lookup(dirid, &dirops.name).await {
        Ok(fid) => {
            let obj_attributes = match context.vfs.getattr(fid).await {
                Ok(v) => post_op_attr::Some(v),
                Err(_) => post_op_attr::None,
            };

            debug!("lookup success {:?} --> {:?}", xid, obj_attributes);
            Ok(LOOKUP3res::Ok(LOOKUP3resok {
                object: context.vfs.id_to_fh(fid),
                obj_attributes,
                dir_attributes,
            }))
        }
        Err(stat) => {
            debug!("lookup error {:?}({:?}) --> {:?}", xid, dirops.name, stat);
            Ok(LOOKUP3res::Err((stat, LOOKUP3resfail { dir_attributes })))
        }
    }
}

pub async fn nfsproc3_read(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let args = READ3args::unpack(input)?.0;
    debug!("nfsproc3_read({:?},{:?}) ", xid, args);

    let id = context.vfs.fh_to_id(&args.file);
    if let Err(stat) = id {
        make_success_reply(xid).pack(output)?;
        stat.pack(output)?;
        post_op_attr::None.pack(output)?;
        return Ok(());
    }
    let id = id.unwrap();

    let obj_attr = match context.vfs.getattr(id).await {
        Ok(v) => post_op_attr::Some(v),
        Err(_) => post_op_attr::None,
    };
    match context.vfs.read(id, args.offset, args.count).await {
        Ok((bytes, eof)) => {
            let res = READ3resok {
                file_attributes: obj_attr,
                count: bytes.len() as u32,
                eof,
                data: Opaque::owned(bytes),
            };
            make_success_reply(xid).pack(output)?;
            nfsstat3::NFS3_OK.pack(output)?;
            res.pack(output)?;
        }
        Err(stat) => {
            error!("read error {:?} --> {:?}", xid, stat);
            make_success_reply(xid).pack(output)?;
            stat.pack(output)?;
            obj_attr.pack(output)?;
        }
    }
    Ok(())
}

pub async fn nfsproc3_fsinfo(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let handle = nfs_fh3::unpack(input)?.0;
    debug!("nfsproc3_fsinfo({:?},{:?}) ", xid, handle);

    let id = context.vfs.fh_to_id(&handle);
    // fail if unable to convert file handle
    if let Err(stat) = id {
        make_success_reply(xid).pack(output)?;
        stat.pack(output)?;
        post_op_attr::None.pack(output)?;
        return Ok(());
    }
    let id = id.unwrap();

    match context.vfs.fsinfo(id).await {
        Ok(fsinfo) => {
            debug!(" {:?} --> {:?}", xid, fsinfo);
            make_success_reply(xid).pack(output)?;
            nfsstat3::NFS3_OK.pack(output)?;
            fsinfo.pack(output)?;
        }
        Err(stat) => {
            error!("fsinfo error {:?} --> {:?}", xid, stat);
            make_success_reply(xid).pack(output)?;
            stat.pack(output)?;
        }
    }
    Ok(())
}

pub async fn nfsproc3_access(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let handle = nfs_fh3::unpack(input)?.0;
    let mut access: u32 = Unpack::unpack(input)?.0;
    debug!("nfsproc3_access({:?},{:?},{:?})", xid, handle, access);

    let id = context.vfs.fh_to_id(&handle);
    // fail if unable to convert file handle
    if let Err(stat) = id {
        make_success_reply(xid).pack(output)?;
        stat.pack(output)?;
        post_op_attr::None.pack(output)?;
        return Ok(());
    }
    let id = id.unwrap();

    let obj_attr = match context.vfs.getattr(id).await {
        Ok(v) => post_op_attr::Some(v),
        Err(_) => post_op_attr::None,
    };
    // TODO better checks here
    if !matches!(context.vfs.capabilities(), VFSCapabilities::ReadWrite) {
        access &= ACCESS3_READ | ACCESS3_LOOKUP;
    }
    debug!(" {:?} ---> {:?}", xid, access);
    make_success_reply(xid).pack(output)?;
    nfsstat3::NFS3_OK.pack(output)?;
    obj_attr.pack(output)?;
    access.pack(output)?;
    Ok(())
}

pub async fn nfsproc3_pathconf(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let handle = nfs_fh3::unpack(input)?.0;
    debug!("nfsproc3_pathconf({:?},{:?})", xid, handle);

    let id = context.vfs.fh_to_id(&handle);
    // fail if unable to convert file handle
    if let Err(stat) = id {
        make_success_reply(xid).pack(output)?;
        stat.pack(output)?;
        post_op_attr::None.pack(output)?;
        return Ok(());
    }
    let id = id.unwrap();

    let obj_attr = match context.vfs.getattr(id).await {
        Ok(v) => post_op_attr::Some(v),
        Err(_) => post_op_attr::None,
    };
    let res = PATHCONF3resok {
        obj_attributes: obj_attr,
        linkmax: 0,
        name_max: 32768,
        no_trunc: true,
        chown_restricted: true,
        case_insensitive: false,
        case_preserving: true,
    };
    debug!(" {:?} ---> {:?}", xid, res);
    make_success_reply(xid).pack(output)?;
    nfsstat3::NFS3_OK.pack(output)?;
    res.pack(output)?;
    Ok(())
}

pub async fn nfsproc3_fsstat(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let handle = nfs_fh3::unpack(input)?.0;
    debug!("nfsproc3_fsstat({:?},{:?}) ", xid, handle);
    let id = context.vfs.fh_to_id(&handle);
    // fail if unable to convert file handle
    if let Err(stat) = id {
        make_success_reply(xid).pack(output)?;
        stat.pack(output)?;
        post_op_attr::None.pack(output)?;
        return Ok(());
    }
    let id = id.unwrap();

    let obj_attr = match context.vfs.getattr(id).await {
        Ok(v) => post_op_attr::Some(v),
        Err(_) => post_op_attr::None,
    };
    let res = FSSTAT3resok {
        obj_attributes: obj_attr,
        tbytes: 1024 * 1024 * 1024 * 1024,
        fbytes: 1024 * 1024 * 1024 * 1024,
        abytes: 1024 * 1024 * 1024 * 1024,
        tfiles: 1024 * 1024 * 1024,
        ffiles: 1024 * 1024 * 1024,
        afiles: 1024 * 1024 * 1024,
        invarsec: u32::MAX,
    };
    make_success_reply(xid).pack(output)?;
    nfsstat3::NFS3_OK.pack(output)?;
    debug!(" {:?} ---> {:?}", xid, res);
    res.pack(output)?;
    Ok(())
}

pub async fn nfsproc3_readdirplus(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let args = READDIRPLUS3args::unpack(input)?.0;

    let result = nfsproc3_readdirplus_impl(xid, args, context).await;
    make_success_reply(xid).pack(output)?;
    result.pack(output)?;
    Ok(())
}

async fn nfsproc3_readdirplus_impl(
    xid: u32,
    args: READDIRPLUS3args,
    context: &RPCContext,
) -> READDIRPLUS3res {
    debug!("nfsproc3_readdirplus({xid},{args:?})");

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
        error!("readdirplus error {xid} --> {stat:?}");
        return READDIRPLUS3res::Err((stat, READDIRPLUS3resfail { dir_attributes }));
    }
    let max_bytes_allowed = args.maxcount as usize - 128;

    let iter = context.vfs.readdirplus(dirid, args.cookie).await;

    if let Err(stat) = iter {
        error!("readdirplus error {xid} --> {stat:?}");
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
                error!("readdirplus error {xid} --> {stat:?}");
                return READDIRPLUS3res::Err((stat, READDIRPLUS3resfail { dir_attributes }));
            }
        }
    }

    let entries = entries_result.into_inner();
    if entries.0.is_empty() && !eof {
        let stat = nfsstat3::NFS3ERR_TOOSMALL;
        error!("readdirplus error {xid} --> {stat:?}");
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

pub async fn nfsproc3_readdir(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let readdir3args = READDIR3args::unpack(input)?.0;
    let readdir3res = readdir_impl(xid, readdir3args, context).await?;
    make_success_reply(xid).pack(output)?;
    readdir3res.pack(output)?;

    Ok(())
}

async fn readdir_impl(
    xid: u32,
    readdir3args: READDIR3args,
    context: &RPCContext,
) -> anyhow::Result<READDIR3res> {
    let dirid = context.vfs.fh_to_id(&readdir3args.dir);
    // fail if unable to convert file handle
    if let Err(stat) = dirid {
        return Ok(READDIR3res::Err((stat, READDIR3resfail::default())));
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
            return Ok(READDIR3res::Err((
                nfsstat3::NFS3ERR_BAD_COOKIE,
                READDIR3resfail::default(),
            )));
        }
        debug!(" -- Start of readdir");
    } else if readdir3args.cookieverf != cookieverf {
        warn!(
            " -- Dir version mismatch. Received {:?}, Expected: {:?}",
            readdir3args.cookieverf, cookieverf
        );
        return Ok(READDIR3res::Err((
            nfsstat3::NFS3ERR_BAD_COOKIE,
            READDIR3resfail::default(),
        )));
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
        return Ok(READDIR3res::Err((
            nfsstat3::NFS3ERR_TOOSMALL,
            READDIR3resfail {
                dir_attributes: resok.dir_attributes,
            },
        )));
    }
    let max_bytes_allowed = readdir3args.count as usize - empty_len;

    let iter = context.vfs.readdir(dirid, readdir3args.cookie).await;
    if let Err(stat) = iter {
        return Ok(READDIR3res::Err((
            stat,
            READDIR3resfail {
                dir_attributes: resok.dir_attributes,
            },
        )));
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
                error!("readdir error {xid} --> {stat:?}");
                return Ok(READDIR3res::Err((
                    stat,
                    READDIR3resfail {
                        dir_attributes: resok.dir_attributes,
                    },
                )));
            }
        }
    }

    let entries = entries.into_inner();
    if entries.is_empty() && !eof {
        let stat = nfsstat3::NFS3ERR_TOOSMALL;
        error!("readdir error {xid} --> {stat:?}");
        return Ok(READDIR3res::Err((
            stat,
            READDIR3resfail {
                dir_attributes: resok.dir_attributes,
            },
        )));
    }

    resok.reply.entries = entries;
    resok.reply.eof = eof;
    Ok(Nfs3Result::Ok(resok))
}

pub async fn nfsproc3_write(
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

    let args = WRITE3args::unpack(input)?.0;
    debug!("nfsproc3_write({:?},...) ", xid);
    // sanity check the length
    if args.data.len() != args.count as usize {
        garbage_args_reply_message(xid).pack(output)?;
        return Ok(());
    }

    let id = context.vfs.fh_to_id(&args.file);
    if let Err(stat) = id {
        make_success_reply(xid).pack(output)?;
        stat.pack(output)?;
        wcc_data::default().pack(output)?;
        return Ok(());
    }
    let id = id.unwrap();

    // get the object attributes before the write
    let pre_obj_attr = match context.vfs.getattr(id).await {
        Ok(v) => {
            let wccattr = wcc_attr {
                size: v.size,
                mtime: v.mtime,
                ctime: v.ctime,
            };
            pre_op_attr::Some(wccattr)
        }
        Err(_) => pre_op_attr::None,
    };

    match context.vfs.write(id, args.offset, &args.data).await {
        Ok(fattr) => {
            debug!("write success {:?} --> {:?}", xid, fattr);
            let res = WRITE3resok {
                file_wcc: wcc_data {
                    before: pre_obj_attr,
                    after: post_op_attr::Some(fattr),
                },
                count: args.count,
                committed: stable_how::FILE_SYNC,
                verf: writeverf3(context.vfs.serverid().0),
            };
            make_success_reply(xid).pack(output)?;
            nfsstat3::NFS3_OK.pack(output)?;
            res.pack(output)?;
        }
        Err(stat) => {
            error!("write error {:?} --> {:?}", xid, stat);
            make_success_reply(xid).pack(output)?;
            stat.pack(output)?;
            wcc_data::default().pack(output)?;
        }
    }
    Ok(())
}

#[allow(clippy::collapsible_if)]
pub async fn nfsproc3_create(
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

    let args = CREATE3args::unpack(input)?.0;
    let dirops = args.where_;
    let createhow = args.how;

    debug!("nfsproc3_create({:?}, {:?}, {:?}) ", xid, dirops, createhow);

    // find the directory we are supposed to create the
    // new file in
    let dirid = context.vfs.fh_to_id(&dirops.dir);
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

    if matches!(&createhow, createhow3::GUARDED(_)) {
        if context.vfs.lookup(dirid, &dirops.name).await.is_ok() {
            // file exists. Fail with NFS3ERR_EXIST.
            // Re-read dir attributes
            // for post op attr
            let post_dir_attr = match context.vfs.getattr(dirid).await {
                Ok(v) => post_op_attr::Some(v),
                Err(_) => post_op_attr::None,
            };

            make_success_reply(xid).pack(output)?;
            nfsstat3::NFS3ERR_EXIST.pack(output)?;
            wcc_data {
                before: pre_dir_attr,
                after: post_dir_attr,
            }
            .pack(output)?;
            return Ok(());
        }
    }

    let fid: Result<fileid3, nfsstat3>;
    let postopattr: post_op_attr;
    // fill in the fid and post op attr here
    match createhow {
        createhow3::EXCLUSIVE(_) => {
            // the API for exclusive is very slightly different
            // We are not returning a post op attribute
            fid = context.vfs.create_exclusive(dirid, &dirops.name).await;
            postopattr = post_op_attr::None;
        }
        createhow3::UNCHECKED(target_attributes) | createhow3::GUARDED(target_attributes) => {
            // create!
            let res = context
                .vfs
                .create(dirid, &dirops.name, target_attributes)
                .await;

            match res {
                Ok((fid_, fattr)) => {
                    fid = Ok(fid_);
                    postopattr = post_op_attr::Some(fattr);
                }
                Err(e) => {
                    fid = Err(e);
                    postopattr = post_op_attr::None;
                }
            }
        }
    }

    // Re-read dir attributes for post op attr
    let post_dir_attr = match context.vfs.getattr(dirid).await {
        Ok(v) => post_op_attr::Some(v),
        Err(_) => post_op_attr::None,
    };
    let wcc_res = wcc_data {
        before: pre_dir_attr,
        after: post_dir_attr,
    };

    match fid {
        Ok(fid) => {
            debug!("create success --> {:?}, {:?}", fid, postopattr);
            make_success_reply(xid).pack(output)?;
            nfsstat3::NFS3_OK.pack(output)?;
            // serialize CREATE3resok
            let fh = context.vfs.id_to_fh(fid);
            post_op_fh3::Some(fh).pack(output)?;
            postopattr.pack(output)?;
            wcc_res.pack(output)?;
        }
        Err(e) => {
            error!("create error --> {:?}", e);
            // serialize CREATE3resfail
            make_success_reply(xid).pack(output)?;
            e.pack(output)?;
            wcc_res.pack(output)?;
        }
    }

    Ok(())
}

pub async fn nfsproc3_setattr(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    if !matches!(context.vfs.capabilities(), VFSCapabilities::ReadWrite) {
        warn!("No write capabilities.");
        make_success_reply(xid).pack(output)?;
        nfsstat3::NFS3ERR_ROFS.pack(output)?;
        wcc_data::default().pack(output)?;
        return Ok(());
    }
    let args = SETATTR3args::unpack(input)?.0;
    debug!("nfsproc3_setattr({:?},{:?}) ", xid, args);

    let id = context.vfs.fh_to_id(&args.object);
    // fail if unable to convert file handle
    if let Err(stat) = id {
        make_success_reply(xid).pack(output)?;
        stat.pack(output)?;
        return Ok(());
    }
    let id = id.unwrap();

    let ctime;

    let pre_op_attr = match context.vfs.getattr(id).await {
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
            make_success_reply(xid).pack(output)?;
            stat.pack(output)?;
            wcc_data::default().pack(output)?;
            return Ok(());
        }
    };
    // handle the guard
    match args.guard {
        sattrguard3::None => {}
        sattrguard3::Some(c) => {
            if c.seconds != ctime.seconds || c.nseconds != ctime.nseconds {
                make_success_reply(xid).pack(output)?;
                nfsstat3::NFS3ERR_NOT_SYNC.pack(output)?;
                wcc_data::default().pack(output)?;
            }
        }
    }

    match context.vfs.setattr(id, args.new_attributes).await {
        Ok(post_op_attr) => {
            debug!(" setattr success {:?} --> {:?}", xid, post_op_attr);
            let wcc_res = wcc_data {
                before: pre_op_attr,
                after: post_op_attr::Some(post_op_attr),
            };
            make_success_reply(xid).pack(output)?;
            nfsstat3::NFS3_OK.pack(output)?;
            wcc_res.pack(output)?;
        }
        Err(stat) => {
            error!("setattr error {:?} --> {:?}", xid, stat);
            make_success_reply(xid).pack(output)?;
            stat.pack(output)?;
            wcc_data::default().pack(output)?;
        }
    }
    Ok(())
}

pub async fn nfsproc3_remove(
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

    let dirops = diropargs3::unpack(input)?.0;

    debug!("nfsproc3_remove({:?}, {:?}) ", xid, dirops);

    // find the directory with the file
    let dirid = context.vfs.fh_to_id(&dirops.dir);
    if let Err(stat) = dirid {
        // directory does not exist
        make_success_reply(xid).pack(output)?;
        stat.pack(output)?;
        wcc_data::default().pack(output)?;
        error!("Directory does not exist");
        return Ok(());
    }
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

    // delete!
    let res = context.vfs.remove(dirid, &dirops.name).await;

    // Re-read dir attributes for post op attr
    let post_dir_attr = match context.vfs.getattr(dirid).await {
        Ok(v) => post_op_attr::Some(v),
        Err(_) => post_op_attr::None,
    };
    let wcc_res = wcc_data {
        before: pre_dir_attr,
        after: post_dir_attr,
    };

    match res {
        Ok(()) => {
            debug!("remove success");
            make_success_reply(xid).pack(output)?;
            nfsstat3::NFS3_OK.pack(output)?;
            wcc_res.pack(output)?;
        }
        Err(e) => {
            error!("remove error {:?} --> {:?}", xid, e);
            // serialize CREATE3resfail
            make_success_reply(xid).pack(output)?;
            e.pack(output)?;
            wcc_res.pack(output)?;
        }
    }

    Ok(())
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
    let post_from_dir_attr = match context.vfs.getattr(from_dirid).await {
        Ok(v) => post_op_attr::Some(v),
        Err(_) => post_op_attr::None,
    };
    let post_to_dir_attr = match context.vfs.getattr(to_dirid).await {
        Ok(v) => post_op_attr::Some(v),
        Err(_) => post_op_attr::None,
    };
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
    let post_dir_attr = match context.vfs.getattr(dirid).await {
        Ok(v) => post_op_attr::Some(v),
        Err(_) => post_op_attr::None,
    };
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
    let post_dir_attr = match context.vfs.getattr(dirid).await {
        Ok(v) => post_op_attr::Some(v),
        Err(_) => post_op_attr::None,
    };
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
    if let Err(stat) = id {
        make_success_reply(xid).pack(output)?;
        stat.pack(output)?;
        return Ok(());
    }
    let id = id.unwrap();
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
