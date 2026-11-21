use std::sync::Arc;

use nfs3_types::nfs3::{
    ACCESS3args, COMMIT3args, CREATE3args, CREATE3res, FSINFO3args, FSSTAT3args, GETATTR3args,
    LOOKUP3args, LOOKUP3res, MKDIR3args, MKDIR3res, Nfs3Result, PATHCONF3args, PROGRAM, READ3args,
    READ3res, READ3resfail, READ3resok, READDIR3args, READDIRPLUS3args, READDIRPLUS3res,
    READDIRPLUS3resok, READLINK3args, REMOVE3args, REMOVE3res, RENAME3args, RENAME3res,
    SETATTR3args, SYMLINK3args, SYMLINK3res, VERSION, WRITE3args, WRITE3res, WRITE3resfail,
    WRITE3resok, cookieverf3, dirlistplus3, entryplus3, fattr3, ftype3, nfs_fh3, nfsstat3,
    nfstime3, post_op_attr, post_op_fh3, pre_op_attr, specdata3, stable_how, wcc_attr, wcc_data,
    writeverf3,
};
use nfs3_types::rpc::{
    RPC_VERSION_2, accept_stat_data, accepted_reply, auth_flavor, auth_stat, auth_unix, call_body,
    msg_body, opaque_auth, rejected_reply, reply_body, rpc_msg,
};
use nfs3_types::xdr_codec::{
    Decode, Decoder, Encode, Encoder, Payload, PayloadLimits, ReceiveBuffer, SendPlan, Void,
};
use nfs3_types::{mount, portmap};

use super::ServerLimits;
use crate::context::RPCContext;
use crate::vfs::owned::WorkLease;
use crate::vfs::{NfsFileSystem, OwnedRead, OwnedWrite, VFSCapabilities};
use crate::{mount_handlers as m, nfs_handlers as n, portmap_handlers as p};

const AUTH_WORK_BYTES: usize = 4096;
const FALLBACK_WORK_BYTES: usize = 4096;

pub(super) enum Work {
    Error(reply_body<'static>),
    Null,
    Getattr(GETATTR3args),
    Lookup(LOOKUP3args<'static>),
    Read(READ3args),
    Write(WRITE3args<Payload>),
    Fsinfo(FSINFO3args),
    Access(ACCESS3args),
    Pathconf(PATHCONF3args),
    Fsstat(FSSTAT3args),
    Readdir(READDIR3args),
    Readdirplus(READDIRPLUS3args),
    Create(CREATE3args<'static>),
    Setattr(SETATTR3args),
    Remove(REMOVE3args<'static>),
    Rename(RENAME3args<'static, 'static>),
    Mkdir(MKDIR3args<'static>),
    Symlink(SYMLINK3args<'static>),
    Readlink(READLINK3args),
    Commit(COMMIT3args),
    Mount(mount::dirpath<'static>),
    Unmount(mount::dirpath<'static>),
    UnmountAll,
    Export,
    Getport(portmap::mapping),
}

pub(super) fn control_size() -> usize {
    // Known fixed objects plus error encoder work; runtime/allocator internals
    // remain excluded, not advertised as an exact process-memory bound.
    std::mem::size_of::<Work>()
        + std::mem::size_of::<SendPlan>()
        + std::mem::size_of::<Decoder>()
        + std::mem::size_of::<Encoder>()
        + std::mem::size_of::<crate::vfs::owned::Lease>()
        + std::mem::size_of::<auth_unix>()
        + 400
        + std::mem::size_of::<Payload>()
        + Payload::descriptor_reservation_cost(1).unwrap_or(usize::MAX)
        + nfs3_types::async_io::record_reservation_bound(1).unwrap_or(usize::MAX)
        + std::mem::size_of::<ReceiveBuffer>()
        + AUTH_WORK_BYTES
        + FALLBACK_WORK_BYTES
        + 1024
}

/// Exercise fixed maximum READ/WRITE metadata, nesting and chunk work without
/// allocating configured transfer backing. All READ outputs have one origin;
/// the one-byte case also exercises the trailing padding part.
pub(super) fn bulk_reply_limits_fit(limits: &ServerLimits) -> bool {
    let attr = fattr3 {
        type_: ftype3::NF3REG,
        mode: 0,
        nlink: 0,
        uid: 0,
        gid: 0,
        size: 0,
        used: 0,
        rdev: specdata3::default(),
        fsid: 0,
        fileid: 0,
        atime: nfstime3::default(),
        mtime: nfstime3::default(),
        ctime: nfstime3::default(),
    };
    let payload_limits = PayloadLimits {
        max_logical_bytes: 1,
        max_backing_capacity: 1,
        max_segments: 1,
    };
    let Ok(buffer) = ReceiveBuffer::try_new(1, 1) else {
        return false;
    };
    let Ok(data) = buffer.freeze(1, payload_limits) else {
        return false;
    };
    let read: READ3res<Payload> = Nfs3Result::Ok(READ3resok {
        file_attributes: post_op_attr::Some(attr.clone()),
        count: 1,
        eof: false,
        data,
    });
    if success(0, &read, limits).is_none() {
        return false;
    }
    let write = WRITE3res::Ok(WRITE3resok {
        file_wcc: wcc_data {
            before: pre_op_attr::Some(wcc_attr {
                size: 0,
                mtime: nfstime3::default(),
                ctime: nfstime3::default(),
            }),
            after: post_op_attr::Some(attr),
        },
        count: 0,
        committed: stable_how::FILE_SYNC,
        verf: writeverf3::default(),
    });
    success(0, &write, limits).is_some()
}

pub(super) fn directory_reply_limits_fit(limits: &ServerLimits) -> bool {
    let Some(bytes) = limits.directory.reply_metadata_bound() else {
        return false;
    };
    if bytes > limits.encode.max_wire_bytes
        || bytes > limits.encode.max_metadata_bytes
        || Encoder::metadata_reservation_bound(bytes)
            .is_none_or(|n| n > limits.encode.max_reservation_bytes)
    {
        return false;
    }
    // Constant-size fixture checks nesting/chunks against the real derives,
    // without iterating or allocating according to untrusted configuration.
    let attr = fattr3 {
        type_: ftype3::NF3DIR,
        mode: 0,
        nlink: 0,
        uid: 0,
        gid: 0,
        size: 0,
        used: 0,
        rdev: specdata3::default(),
        fsid: 0,
        fileid: 0,
        atime: nfstime3::default(),
        mtime: nfstime3::default(),
        ctime: nfstime3::default(),
    };
    let reply: READDIRPLUS3res<'_> = Nfs3Result::Ok(READDIRPLUS3resok {
        dir_attributes: post_op_attr::Some(attr.clone()),
        cookieverf: cookieverf3::default(),
        reply: dirlistplus3 {
            entries: nfs3_types::xdr_codec::List(vec![entryplus3 {
                fileid: 0,
                name: b"x".as_slice().into(),
                cookie: 1,
                name_attributes: post_op_attr::Some(attr),
                name_handle: post_op_fh3::Some(nfs_fh3 {
                    data: nfs3_types::xdr_codec::Opaque::borrowed(&[0; 64]),
                }),
            }]),
            eof: false,
        },
    });
    let mut probe = *limits;
    probe.encode.max_wire_bytes = 512;
    probe.encode.max_metadata_bytes = 512;
    probe.encode.max_reservation_bytes = 4096;
    success(0, &reply, &probe).is_some()
}

impl Work {
    fn names_fit(&self, max: usize) -> bool {
        match self {
            Self::Lookup(a) => a.what.name.0.len() <= max,
            Self::Create(a) => a.where_.name.0.len() <= max,
            Self::Mkdir(a) => a.where_.name.0.len() <= max,
            Self::Symlink(a) => a.where_.name.0.len() <= max,
            Self::Remove(a) => a.object.name.0.len() <= max,
            Self::Rename(a) => a.from.name.0.len() <= max && a.to.name.0.len() <= max,
            _ => true,
        }
    }
    pub(super) const fn read_count(&self) -> Option<u32> {
        if let Self::Read(args) = self {
            Some(args.count)
        } else {
            None
        }
    }
    const fn mutation(&self) -> bool {
        matches!(
            self,
            Self::Write(_)
                | Self::Create(_)
                | Self::Setattr(_)
                | Self::Remove(_)
                | Self::Rename(_)
                | Self::Mkdir(_)
                | Self::Symlink(_)
                | Self::Commit(_)
                | Self::Mount(_)
                | Self::Unmount(_)
                | Self::UnmountAll
        )
    }
}

pub(super) struct Decoded {
    pub xid: u32,
    pub auth: Option<auth_unix>,
    pub work: Work,
    pub check_authorization: bool,
}

fn accepted(stat: accept_stat_data) -> reply_body<'static> {
    reply_body::MSG_ACCEPTED(accepted_reply {
        verf: opaque_auth::default(),
        reply_data: stat,
    })
}
const fn bad_auth(stat: auth_stat) -> Work {
    Work::Error(reply_body::MSG_DENIED(rejected_reply::AUTH_ERROR(stat)))
}

fn args<D: Decode>(decoder: &mut Decoder, wrap: impl FnOnce(D) -> Work) -> Result<Work, ()> {
    decoder.decode().map(wrap).map_err(|_decode_error| ())
}

fn procedure(decoder: &mut Decoder, call: &call_body<'_>) -> Result<Work, ()> {
    macro_rules! a {
        ($ty:ty, $variant:ident) => {
            args::<$ty>(decoder, Work::$variant)
        };
    }
    macro_rules! void {
        ($variant:ident) => {
            args::<Void>(decoder, |_| Work::$variant)
        };
    }
    let version = match call.prog {
        PROGRAM => VERSION,
        mount::PROGRAM => mount::VERSION,
        portmap::PROGRAM => portmap::VERSION,
        _ => return Ok(Work::Error(accepted(accept_stat_data::PROG_UNAVAIL))),
    };
    if call.vers != version {
        return Ok(Work::Error(accepted(accept_stat_data::PROG_MISMATCH {
            low: version,
            high: version,
        })));
    }
    match (call.prog, call.proc) {
        (PROGRAM | mount::PROGRAM | portmap::PROGRAM, 0) => void!(Null),
        (PROGRAM, 1) => a!(GETATTR3args, Getattr),
        (PROGRAM, 2) => a!(SETATTR3args, Setattr),
        (PROGRAM, 3) => a!(LOOKUP3args<'static>, Lookup),
        (PROGRAM, 4) => a!(ACCESS3args, Access),
        (PROGRAM, 5) => a!(READLINK3args, Readlink),
        (PROGRAM, 6) => a!(READ3args, Read),
        (PROGRAM, 7) => a!(WRITE3args<Payload>, Write),
        (PROGRAM, 8) => a!(CREATE3args<'static>, Create),
        (PROGRAM, 9) => a!(MKDIR3args<'static>, Mkdir),
        (PROGRAM, 10) => a!(SYMLINK3args<'static>, Symlink),
        (PROGRAM, 12 | 13) => a!(REMOVE3args<'static>, Remove),
        (PROGRAM, 14) => a!(RENAME3args<'static, 'static>, Rename),
        (PROGRAM, 16) => a!(READDIR3args, Readdir),
        (PROGRAM, 17) => a!(READDIRPLUS3args, Readdirplus),
        (PROGRAM, 18) => a!(FSSTAT3args, Fsstat),
        (PROGRAM, 19) => a!(FSINFO3args, Fsinfo),
        (PROGRAM, 20) => a!(PATHCONF3args, Pathconf),
        (PROGRAM, 21) => a!(COMMIT3args, Commit),
        (mount::PROGRAM, 1) => a!(mount::dirpath<'static>, Mount),
        (mount::PROGRAM, 3) => a!(mount::dirpath<'static>, Unmount),
        (mount::PROGRAM, 4) => void!(UnmountAll),
        (mount::PROGRAM, 5) => void!(Export),
        (portmap::PROGRAM, 3) => a!(portmap::mapping, Getport),
        _ => Ok(Work::Error(accepted(accept_stat_data::PROC_UNAVAIL))),
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "Keep RPC/auth validation and record release ordered together"
)]
pub(super) fn decode(record: Payload, limits: &ServerLimits) -> Option<Decoded> {
    let mut decoder = Decoder::new(record, limits.decode_limits());
    let xid = decoder.decode::<u32>().ok()?;
    let kind = decoder.decode::<u32>().ok()?;
    if kind != 0 {
        return None;
    }
    let rpcvers = decoder.decode::<u32>().ok()?;
    if rpcvers != RPC_VERSION_2 {
        drop(decoder);
        return Some(Decoded {
            xid,
            auth: None,
            check_authorization: false,
            work: Work::Error(reply_body::MSG_DENIED(rejected_reply::RPC_MISMATCH {
                low: RPC_VERSION_2,
                high: RPC_VERSION_2,
            })),
        });
    }
    let program = decoder.decode::<u32>().ok()?;
    let version = decoder.decode::<u32>().ok()?;
    let procedure_number = decoder.decode::<u32>().ok()?;
    let Ok(cred) = decoder.decode::<opaque_auth<'static>>() else {
        drop(decoder);
        return Some(Decoded {
            xid,
            auth: None,
            check_authorization: false,
            work: bad_auth(auth_stat::AUTH_BADCRED),
        });
    };
    let Ok(verf) = decoder.decode::<opaque_auth<'static>>() else {
        drop(decoder);
        return Some(Decoded {
            xid,
            auth: None,
            check_authorization: false,
            work: bad_auth(auth_stat::AUTH_BADVERF),
        });
    };
    let auth = if cred.flavor == auth_flavor::AUTH_UNIX {
        // This is bounded copied auth metadata, not record materialization.
        let payload = Payload::try_from_vec(
            cred.body.into_owned(),
            PayloadLimits {
                max_logical_bytes: 400,
                max_backing_capacity: 400,
                max_segments: 1,
            },
        )
        .ok()?;
        // Separate RFC-bounded auth work is part of control_size(), not an
        // unchecked second copy of the configurable aggregate decode budget.
        let mut auth_decoder = Decoder::new(
            payload,
            nfs3_types::xdr_codec::DecodeLimits {
                max_metadata_bytes: 400,
                max_collection_elements: 16,
                max_depth: 4,
                max_reservation_bytes: AUTH_WORK_BYTES,
                payload: PayloadLimits {
                    max_logical_bytes: 0,
                    max_backing_capacity: 0,
                    max_segments: 0,
                },
            },
        );
        let result = auth_decoder.decode::<auth_unix>();
        if let (Ok(auth), Ok(())) = (result, auth_decoder.finish()) {
            Some(auth)
        } else {
            drop(decoder);
            return Some(Decoded {
                xid,
                auth: None,
                check_authorization: false,
                work: bad_auth(auth_stat::AUTH_BADCRED),
            });
        }
    } else {
        None
    };
    let call = call_body {
        rpcvers,
        prog: program,
        vers: version,
        proc: procedure_number,
        cred: opaque_auth::default(),
        verf,
    };
    let work = procedure(&mut decoder, &call);
    // Error mappings without a known procedure have no argument grammar. They
    // drop the record; all executable procedures require exact consumption.
    let work = match work {
        Ok(Work::Error(error)) => {
            drop(decoder);
            Work::Error(error)
        }
        work => match (work, decoder.finish()) {
            (Ok(work), Ok(())) => work,
            _ => Work::Error(accepted(accept_stat_data::GARBAGE_ARGS)),
        },
    };
    Some(Decoded {
        xid,
        auth,
        work,
        check_authorization: program != portmap::PROGRAM,
    })
}

fn encode<R: Encode>(
    xid: u32,
    body: reply_body<'_>,
    value: &R,
    limits: nfs3_types::xdr_codec::EncodeLimits,
) -> Option<SendPlan> {
    let mut encoder = Encoder::new(limits);
    encoder
        .encode(&rpc_msg {
            xid,
            body: msg_body::REPLY(body),
        })
        .ok()?;
    encoder.encode(value).ok()?;
    encoder.finish().ok()
}

fn success<R: Encode>(xid: u32, value: &R, limits: &ServerLimits) -> Option<SendPlan> {
    encode(
        xid,
        accepted(accept_stat_data::SUCCESS),
        value,
        limits.encode_limits(),
    )
}

#[expect(
    clippy::too_many_lines,
    reason = "Single procedure dispatch; no additional allocation or policy layer"
)]
pub(super) async fn execute<T: NfsFileSystem>(
    context: RPCContext<T>,
    xid: u32,
    work: Work,
    limits: &ServerLimits,
    read_buffer: Option<ReceiveBuffer>,
    lease: WorkLease,
    authorized: bool,
) -> Option<SendPlan> {
    let work = if authorized {
        work
    } else {
        bad_auth(auth_stat::AUTH_TOOWEAK)
    };
    if !work.names_fit(limits.directory.max_name_bytes) {
        // Structurally valid arguments remain an NFS result, not GARBAGE_ARGS.
        // No VFS call has started. Optional attributes are absent; RENAME still
        // carries both mandatory WCC structures in its failure body.
        macro_rules! too_long {
            ($ty:ty) => {
                success(
                    xid,
                    &<$ty>::Err((nfsstat3::NFS3ERR_NAMETOOLONG, Default::default())),
                    limits,
                )
            };
        }
        return match work {
            Work::Lookup(_) => too_long!(LOOKUP3res),
            Work::Create(_) => too_long!(CREATE3res),
            Work::Mkdir(_) => too_long!(MKDIR3res),
            Work::Symlink(_) => too_long!(SYMLINK3res),
            Work::Remove(_) => too_long!(REMOVE3res),
            Work::Rename(_) => too_long!(RENAME3res),
            _ => unreachable!("only name-bearing procedures can exceed the name limit"),
        };
    }
    let mutation = work.mutation();
    macro_rules! shared {
        ($module:ident, $handler:ident, $args:expr) => {
            success(xid, &$module::$handler(context, xid, $args).await, limits)
        };
    }
    let result = match work {
        Work::Error(error) => encode(xid, error, &Void, limits.encode_limits()),
        Work::Null => success(xid, &Void, limits),
        Work::Getattr(args) => shared!(n, nfsproc3_getattr, args),
        Work::Lookup(args) => shared!(n, nfsproc3_lookup, args),
        Work::Access(args) => shared!(n, nfsproc3_access, args),
        Work::Fsstat(args) => shared!(n, nfsproc3_fsstat, args),
        Work::Create(args) => shared!(n, nfsproc3_create, args),
        Work::Setattr(args) => shared!(n, nfsproc3_setattr, args),
        Work::Remove(args) => shared!(n, nfsproc3_remove, args),
        Work::Rename(args) => shared!(n, nfsproc3_rename, args),
        Work::Mkdir(args) => shared!(n, nfsproc3_mkdir, args),
        Work::Symlink(args) => shared!(n, nfsproc3_symlink, args),
        Work::Readlink(args) => shared!(n, nfsproc3_readlink, args),
        Work::Commit(args) => shared!(n, nfsproc3_commit, args),
        Work::Mount(args) => shared!(m, mountproc3_mnt, args),
        Work::Unmount(args) => shared!(m, mountproc3_umnt, args),
        Work::UnmountAll => shared!(m, mountproc3_umnt_all, Void),
        Work::Export => shared!(m, mountproc3_export, Void),
        Work::Getport(args) => shared!(p, pmapproc_getport, args),
        Work::Fsinfo(args) => {
            let mut result = n::nfsproc3_fsinfo(context, xid, args).await;
            if let Nfs3Result::Ok(info) = &mut result {
                info.rtmax = info.rtmax.min(limits.max_read_bytes);
                info.rtpref = info.rtpref.min(info.rtmax);
                info.rtmult = info.rtmult.min(info.rtmax);
                info.wtmax = info.wtmax.min(limits.max_write_bytes);
                info.wtpref = info.wtpref.min(info.wtmax);
                info.wtmult = info.wtmult.min(info.wtmax);
                info.dtpref = info.dtpref.min(limits.directory.max_wire_bytes);
            }
            success(xid, &result, limits)
        }
        Work::Pathconf(args) => {
            let mut result = n::nfsproc3_pathconf(context, xid, args).await;
            if let Nfs3Result::Ok(info) = &mut result {
                info.name_max = info
                    .name_max
                    .min(u32::try_from(limits.directory.max_name_bytes).unwrap_or(u32::MAX));
            }
            success(xid, &result, limits)
        }
        Work::Readdir(mut args) => {
            args.count = args.count.min(limits.directory.max_wire_bytes);
            shared!(n, nfsproc3_readdir, args)
        }
        Work::Readdirplus(mut args) => {
            args.maxcount = args.maxcount.min(limits.directory.max_wire_bytes);
            args.dircount = args.dircount.min(limits.directory.max_wire_bytes);
            shared!(n, nfsproc3_readdirplus, args)
        }
        Work::Read(args) => {
            let result = read(&context, args, read_buffer?, lease).await;
            success(xid, &result, limits)
        }
        Work::Write(args) => {
            let result = write(&context, args, limits, lease).await;
            success(xid, &result, limits)
        }
    };
    if result.is_some() || mutation {
        return result;
    }
    // Fixed error allowance was retained at admission, independent of user reply
    // codec limits. Never issue this false failure after starting a mutation.
    encode(
        xid,
        accepted(accept_stat_data::SYSTEM_ERR),
        &Void,
        nfs3_types::xdr_codec::EncodeLimits {
            max_wire_bytes: 32,
            max_metadata_bytes: 32,
            max_chunks: 4,
            max_retained_capacity: 0,
            max_depth: 8,
            max_reservation_bytes: FALLBACK_WORK_BYTES,
        },
    )
}

async fn read<T: NfsFileSystem>(
    context: &RPCContext<T>,
    args: READ3args,
    buffer: ReceiveBuffer,
    lease: WorkLease,
) -> Nfs3Result<READ3resok<Payload>, READ3resfail> {
    let id = match context.file_handle_converter.fh_from_nfs(&args.file) {
        Ok(id) => id,
        Err(stat) => return Nfs3Result::Err((stat, READ3resfail::default())),
    };
    let file_attributes = context
        .vfs
        .getattr(&id)
        .await
        .map_or(post_op_attr::None, post_op_attr::Some);
    let capacity = buffer.capacity();
    let request = OwnedRead::new(
        id,
        args.offset,
        buffer,
        PayloadLimits {
            max_logical_bytes: capacity,
            max_backing_capacity: capacity,
            max_segments: 1,
        },
        lease,
    );
    match context.vfs.read(request).await {
        Ok(result) => Nfs3Result::Ok(READ3resok {
            file_attributes,
            // Supplied READ storage is capped by the validated u32 transfer limit.
            count: u32::try_from(result.payload.len()).expect("validated READ capacity"),
            eof: result.eof,
            data: result.payload,
        }),
        Err(stat) => Nfs3Result::Err((stat, READ3resfail { file_attributes })),
    }
}

async fn write<T: NfsFileSystem>(
    context: &RPCContext<T>,
    args: WRITE3args<Payload>,
    limits: &ServerLimits,
    lease: WorkLease,
) -> WRITE3res {
    if !matches!(context.vfs.capabilities(), VFSCapabilities::ReadWrite) {
        return WRITE3res::Err((nfsstat3::NFS3ERR_ROFS, WRITE3resfail::default()));
    }
    if args.data.len() != args.count as usize || args.count > limits.max_write_bytes {
        return WRITE3res::Err((nfsstat3::NFS3ERR_INVAL, WRITE3resfail::default()));
    }
    let id = match context.file_handle_converter.fh_from_nfs(&args.file) {
        Ok(id) => id,
        Err(stat) => return WRITE3res::Err((stat, WRITE3resfail::default())),
    };
    let before = n::get_wcc_attr(context, &id)
        .await
        .map_or(pre_op_attr::None, pre_op_attr::Some);
    let request = OwnedWrite {
        handle: id,
        offset: args.offset,
        stable: args.stable,
        data: args.data,
        _lease: Arc::clone(&lease),
    };
    match context.vfs.write(request).await {
        Ok((attr, committed)) => WRITE3res::Ok(WRITE3resok {
            file_wcc: wcc_data {
                before,
                after: post_op_attr::Some(attr),
            },
            count: args.count,
            committed,
            verf: context.file_handle_converter.verf(),
        }),
        Err(stat) => WRITE3res::Err((
            stat,
            WRITE3resfail {
                file_wcc: wcc_data {
                    before,
                    after: post_op_attr::None,
                },
            },
        )),
    }
}
