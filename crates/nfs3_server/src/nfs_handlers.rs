#![allow(clippy::upper_case_acronyms)]
#![allow(dead_code)]
use std::io::{Read, Write};

use nfs3_types::nfs3::*;
use nfs3_types::rpc::*;
use nfs3_types::xdr_codec::Opaque;
use tracing::{debug, error, trace, warn};
use xdr_codec::{Pack, Unpack};

use crate::context::RPCContext;
use crate::rpc::*;
use crate::vfs::VFSCapabilities;

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
            nfsproc3_readdirplus(xid, input, output, context).await?
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
        } /*
          NFSPROC3_MKNOD,
          NFSPROC3_LINK,
          NFSPROC3_COMMIT,
          INVALID*/
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
    let handle = nfs_fh3::unpack(input)?.0;
    debug!("nfsproc3_getattr({:?},{:?}) ", xid, handle);

    let id = context.vfs.fh_to_id(&handle);
    // fail if unable to convert file handle
    if let Err(stat) = id {
        make_success_reply(xid).pack(output)?;
        stat.pack(output)?;
        return Ok(());
    }
    let id = id.unwrap();
    match context.vfs.getattr(id).await {
        Ok(fh) => {
            debug!(" {:?} --> {:?}", xid, fh);
            make_success_reply(xid).pack(output)?;
            nfsstat3::NFS3_OK.pack(output)?;
            fh.pack(output)?;
        }
        Err(stat) => {
            error!("getattr error {:?} --> {:?}", xid, stat);
            make_success_reply(xid).pack(output)?;
            stat.pack(output)?;
        }
    }
    Ok(())
}

pub async fn nfsproc3_lookup(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let dirops = diropargs3::unpack(input)?.0;
    debug!("nfsproc3_lookup({:?},{:?}) ", xid, dirops);

    let dirid = context.vfs.fh_to_id(&dirops.dir);
    // fail if unable to convert file handle
    if let Err(stat) = dirid {
        make_success_reply(xid).pack(output)?;
        stat.pack(output)?;
        post_op_attr::None.pack(output)?;
        return Ok(());
    }
    let dirid = dirid.unwrap();

    let dir_attr = match context.vfs.getattr(dirid).await {
        Ok(v) => post_op_attr::Some(v),
        Err(_) => post_op_attr::None,
    };
    match context.vfs.lookup(dirid, &dirops.name).await {
        Ok(fid) => {
            let obj_attr = match context.vfs.getattr(fid).await {
                Ok(v) => post_op_attr::Some(v),
                Err(_) => post_op_attr::None,
            };

            debug!("lookup success {:?} --> {:?}", xid, obj_attr);
            make_success_reply(xid).pack(output)?;
            nfsstat3::NFS3_OK.pack(output)?;
            context.vfs.id_to_fh(fid).pack(output)?;
            obj_attr.pack(output)?;
            dir_attr.pack(output)?;
        }
        Err(stat) => {
            debug!("lookup error {:?}({:?}) --> {:?}", xid, dirops.name, stat);
            make_success_reply(xid).pack(output)?;
            stat.pack(output)?;
            dir_attr.pack(output)?;
        }
    }
    Ok(())
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

const ACCESS3_READ: u32 = 0x0001;
const ACCESS3_LOOKUP: u32 = 0x0002;
const ACCESS3_MODIFY: u32 = 0x0004;
const ACCESS3_EXTEND: u32 = 0x0008;
const ACCESS3_DELETE: u32 = 0x0010;
const ACCESS3_EXECUTE: u32 = 0x0020;

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
    debug!("nfsproc3_readdirplus({:?},{:?}) ", xid, args);

    let dirid = context.vfs.fh_to_id(&args.dir);
    // fail if unable to convert file handle
    if let Err(stat) = dirid {
        make_success_reply(xid).pack(output)?;
        stat.pack(output)?;
        post_op_attr::None.pack(output)?;
        return Ok(());
    }
    let dirid = dirid.unwrap();
    let dir_attr_maybe = context.vfs.getattr(dirid).await;

    let dir_attr = match dir_attr_maybe {
        Ok(v) => post_op_attr::Some(v),
        Err(_) => post_op_attr::None,
    };

    let dirversion = if let Nfs3Option::Some(dir_attr) = &dir_attr {
        let cvf_version =
            ((dir_attr.mtime.seconds as u64) << 32) | (dir_attr.mtime.nseconds as u64);
        cookieverf3(cvf_version.to_be_bytes())
    } else {
        cookieverf3::default()
    };
    debug!(" -- Dir attr {:?}", dir_attr);
    debug!(" -- Dir version {:?}", dirversion);
    let has_version = args.cookieverf != cookieverf3::default();
    // initial call should hve empty cookie verf
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
    //  2. if we cache readdir results. i.e. we think of a readdir as two parts
    //     a. enumerating everything first
    //     b. the cookie is then used to paginate the enumeration
    //     we can run into file time synchronization issues. i.e. while one
    //     listing occurs and another file is touched, the listing may report
    //     an outdated file status.
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
    /*if args.cookieverf != cookieverf3::default() && args.cookieverf != dirversion {
        info!(" -- Dir version mismatch. Received {:?}", args.cookieverf);
        make_success_reply(xid).pack(output)?;
        nfsstat3::NFS3ERR_BAD_COOKIE.pack(output)?;
        dir_attr.pack(output)?;
        return Ok(());
    }*/
    // subtract off the final entryplus* field (which must be false) and the eof
    let max_bytes_allowed = args.maxcount as usize - 128;
    // args.dircount is bytes of just fileid, name, cookie.
    // This is hard to ballpark, so we just divide it by 16
    let estimated_max_results = args.dircount / 16;
    let max_dircount_bytes = args.dircount as usize;
    let mut ctr = 0;
    match context
        .vfs
        .readdir(dirid, args.cookie, estimated_max_results as usize)
        .await
    {
        Ok(result) => {
            // we count dir_count seperately as it is just a subset of fields
            let mut accumulated_dircount: usize = 0;
            let mut all_entries_written = true;

            // this is a wrapper around a writer that also just counts the number of bytes
            // written
            let mut counting_output = crate::write_counter::WriteCounter::new(output);

            make_success_reply(xid).pack(&mut counting_output)?;
            nfsstat3::NFS3_OK.pack(&mut counting_output)?;
            dir_attr.pack(&mut counting_output)?;
            dirversion.pack(&mut counting_output)?;
            for entry in result.entries {
                let obj_attr = entry.attr;
                let handle = post_op_fh3::Some(context.vfs.id_to_fh(entry.fileid));

                let entry = entryplus3 {
                    fileid: entry.fileid,
                    name: entry.name,
                    cookie: entry.fileid,
                    name_attributes: post_op_attr::Some(obj_attr),
                    name_handle: handle,
                };
                // write the entry into a buffer first
                let mut write_buf: Vec<u8> = Vec::new();
                let mut write_cursor = std::io::Cursor::new(&mut write_buf);
                // true flag for the entryplus3* to mark that this contains an entry
                true.pack(&mut write_cursor)?;
                entry.pack(&mut write_cursor)?;
                write_cursor.flush()?;
                let added_dircount = std::mem::size_of::<fileid3>()                   // fileid
                                    + std::mem::size_of::<u32>() + entry.name.0.len()  // name
                                    + std::mem::size_of::<cookie3>(); // cookie
                let added_output_bytes = write_buf.len();
                // check if we can write without hitting the limits
                if added_output_bytes + counting_output.bytes_written() < max_bytes_allowed
                    && added_dircount + accumulated_dircount < max_dircount_bytes
                {
                    trace!("  -- dirent {:?}", entry);
                    // commit the entry
                    ctr += 1;
                    counting_output.write_all(&write_buf)?;
                    accumulated_dircount += added_dircount;
                    trace!(
                        "  -- lengths: {:?} / {:?} {:?} / {:?}",
                        accumulated_dircount,
                        max_dircount_bytes,
                        counting_output.bytes_written(),
                        max_bytes_allowed
                    );
                } else {
                    trace!(" -- insufficient space. truncating");
                    all_entries_written = false;
                    break;
                }
            }
            // false flag for the final entryplus* linked list
            false.pack(&mut counting_output)?;
            // eof flag is only valid here if we wrote everything
            if all_entries_written {
                debug!("  -- readdir eof {:?}", result.end);
                result.end.pack(&mut counting_output)?;
            } else {
                debug!("  -- readdir eof {:?}", false);
                false.pack(&mut counting_output)?;
            }
            debug!(
                "readir {}, has_version {},  start at {}, flushing {} entries, complete {}",
                dirid, has_version, args.cookie, ctr, all_entries_written
            );
        }
        Err(stat) => {
            error!("readdir error {:?} --> {:?} ", xid, stat);
            make_success_reply(xid).pack(output)?;
            stat.pack(output)?;
            dir_attr.pack(output)?;
        }
    };
    Ok(())
}

pub async fn nfsproc3_readdir(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let args = READDIR3args::unpack(input)?.0;
    debug!("nfsproc3_readdirplus({:?},{:?}) ", xid, args);

    let dirid = context.vfs.fh_to_id(&args.dir);
    // fail if unable to convert file handle
    if let Err(stat) = dirid {
        make_success_reply(xid).pack(output)?;
        stat.pack(output)?;
        post_op_attr::None.pack(output)?;
        return Ok(());
    }
    let dirid = dirid.unwrap();
    let dir_attr_maybe = context.vfs.getattr(dirid).await;

    let dir_attr = match dir_attr_maybe {
        Ok(v) => post_op_attr::Some(v),
        Err(_) => post_op_attr::None,
    };

    let dirversion = if let Nfs3Option::Some(dir_attr) = &dir_attr {
        let cvf_version =
            ((dir_attr.mtime.seconds as u64) << 32) | (dir_attr.mtime.nseconds as u64);
        cookieverf3(cvf_version.to_be_bytes())
    } else {
        cookieverf3::default()
    };
    debug!(" -- Dir attr {:?}", dir_attr);
    debug!(" -- Dir version {:?}", dirversion);
    let has_version = args.cookieverf != cookieverf3::default();
    // subtract off the final entryplus* field (which must be false) and the eof
    let max_bytes_allowed = args.count as usize - 128;
    // args.dircount is bytes of just fileid, name, cookie.
    // This is hard to ballpark, so we just divide it by 16
    let estimated_max_results = args.count / 16;
    let mut ctr = 0;
    match context
        .vfs
        .readdir_simple(dirid, estimated_max_results as usize)
        .await
    {
        Ok(result) => {
            // we count dir_count seperately as it is just a subset of fields
            let mut accumulated_dircount: usize = 0;
            let mut all_entries_written = true;

            // this is a wrapper around a writer that also just counts the number of bytes
            // written
            let mut counting_output = crate::write_counter::WriteCounter::new(output);

            make_success_reply(xid).pack(&mut counting_output)?;
            nfsstat3::NFS3_OK.pack(&mut counting_output)?;
            dir_attr.pack(&mut counting_output)?;
            dirversion.pack(&mut counting_output)?;
            for entry in result.entries {
                let entry = entry3 {
                    fileid: entry.fileid,
                    name: entry.name,
                    cookie: entry.fileid,
                };
                // write the entry into a buffer first
                let mut write_buf: Vec<u8> = Vec::new();
                let mut write_cursor = std::io::Cursor::new(&mut write_buf);
                // true flag for the entryplus3* to mark that this contains an entry
                true.pack(&mut write_cursor)?;
                entry.pack(&mut write_cursor)?;
                write_cursor.flush()?;
                let added_dircount = std::mem::size_of::<fileid3>()                   // fileid
                                    + std::mem::size_of::<u32>() + entry.name.0.len()  // name
                                    + std::mem::size_of::<cookie3>(); // cookie
                let added_output_bytes = write_buf.len();
                // check if we can write without hitting the limits
                if added_output_bytes + counting_output.bytes_written() < max_bytes_allowed {
                    trace!("  -- dirent {:?}", entry);
                    // commit the entry
                    ctr += 1;
                    counting_output.write_all(&write_buf)?;
                    accumulated_dircount += added_dircount;
                    trace!(
                        "  -- lengths: {:?} / {:?} / {:?}",
                        accumulated_dircount,
                        counting_output.bytes_written(),
                        max_bytes_allowed
                    );
                } else {
                    trace!(" -- insufficient space. truncating");
                    all_entries_written = false;
                    break;
                }
            }
            // false flag for the final entryplus* linked list
            false.pack(&mut counting_output)?;
            // eof flag is only valid here if we wrote everything
            if all_entries_written {
                debug!("  -- readdir eof {:?}", result.end);
                result.end.pack(&mut counting_output)?;
            } else {
                debug!("  -- readdir eof {:?}", false);
                false.pack(&mut counting_output)?;
            }
            debug!(
                "readir {}, has_version {},  start at {}, flushing {} entries, complete {}",
                dirid, has_version, args.cookie, ctr, all_entries_written
            );
        }
        Err(stat) => {
            error!("readdir error {:?} --> {:?} ", xid, stat);
            make_success_reply(xid).pack(output)?;
            stat.pack(output)?;
            dir_attr.pack(output)?;
        }
    };
    Ok(())
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
    match &createhow {
        createhow3::UNCHECKED(target_attributes) => {
            debug!("create unchecked {:?}", target_attributes);
        }
        createhow3::GUARDED(target_attributes) => {
            debug!("create guarded {:?}", target_attributes);
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
        createhow3::EXCLUSIVE(_verf) => {
            debug!("create exclusive");
        }
    }

    let fid: Result<fileid3, nfsstat3>;
    let postopattr: post_op_attr;
    // fill in the fid and post op attr here
    if matches!(createhow, createhow3::EXCLUSIVE(_)) {
        // the API for exclusive is very slightly different
        // We are not returning a post op attribute
        fid = context.vfs.create_exclusive(dirid, &dirops.name).await;
        postopattr = post_op_attr::None;
    } else if let createhow3::UNCHECKED(target_attributes) = createhow {
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
    } else {
        unreachable!();
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
