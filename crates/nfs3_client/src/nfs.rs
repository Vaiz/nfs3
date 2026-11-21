use nfs3_types::nfs3::{
    ACCESS3args, ACCESS3res, COMMIT3args, COMMIT3res, CREATE3args, CREATE3res, FSINFO3args,
    FSINFO3res, FSSTAT3args, FSSTAT3res, GETATTR3args, GETATTR3res, LINK3args, LINK3res,
    LOOKUP3args, LOOKUP3res, MKDIR3args, MKDIR3res, MKNOD3args, MKNOD3res, NFS_PROGRAM, Nfs3Result,
    PATHCONF3args, PATHCONF3res, PROGRAM, READ3args, READ3res, READDIR3args, READDIR3res,
    READDIRPLUS3args, READDIRPLUS3res, READLINK3args, READLINK3res, REMOVE3args, REMOVE3res,
    RENAME3args, RENAME3res, RMDIR3args, RMDIR3res, SETATTR3args, SETATTR3res, SYMLINK3args,
    SYMLINK3res, VERSION, WRITE3args, WRITE3res, nfs_fh3, stable_how,
};
use nfs3_types::xdr_codec::{Decode, Encode, Payload, Void};

use crate::io::{AsyncRead, AsyncWrite};
use crate::rpc::RpcClient;
use crate::{RpcError, RpcErrorKind};

/// Client for the `NFSv3` service
#[derive(Debug)]
pub struct Nfs3Client<IO> {
    rpc: RpcClient<IO>,
    max_bulk_bytes: u32,
}

impl<IO> Nfs3Client<IO> {
    /// Validate the maximum bulk reply, nesting, verifier and descriptor envelopes.
    /// Policy is explicit; actual requests are checked before IO.
    pub fn new(rpc: RpcClient<IO>, max_bulk_bytes: u32) -> Result<Self, RpcErrorKind> {
        validate_limits(rpc.limits(), max_bulk_bytes)?;
        Ok(Self {
            rpc,
            max_bulk_bytes,
        })
    }

    #[must_use]
    pub const fn rpc(&self) -> &RpcClient<IO> {
        &self.rpc
    }
}

pub fn validate_limits(
    limits: crate::rpc::RpcLimits,
    max_bulk_bytes: u32,
) -> Result<(), RpcErrorKind> {
    let limits = limits.validate()?;
    let bulk = max_bulk_bytes as usize;
    let padded = bulk
        .checked_add(3)
        .map(|n| n & !3)
        .ok_or(RpcErrorKind::Configuration)?;
    let record_blocks = limits
        .record
        .max_record_bytes
        .div_ceil(limits.record.block_bytes);
    let segments = if bulk == 0 {
        0
    } else {
        bulk.div_ceil(limits.record.block_bytes)
            .checked_add(1)
            .ok_or(RpcErrorKind::Configuration)?
            .min(record_blocks)
    };
    let backing = segments
        .checked_mul(
            limits
                .record
                .block_bytes
                .min(limits.record.max_record_bytes),
        )
        .ok_or(RpcErrorKind::Configuration)?
        .min(limits.record.max_record_bytes);
    // READ: 24 RPC + 400 auth + 4 status + 88 post_op_attr + 12 data fields.
    let read_wire = padded.checked_add(528).ok_or(RpcErrorKind::Configuration)?;
    // WRITE: RPC/verifier + status + pre/post attributes + count/stability/verifier.
    let write_wire = 424_usize
        .checked_add(4 + 28 + 88 + 16)
        .ok_or(RpcErrorKind::Configuration)?;
    let reservation = Payload::descriptor_reservation_cost(segments)
        .and_then(|n| n.checked_add(400))
        .ok_or(RpcErrorKind::Configuration)?;
    if read_wire.max(write_wire) > limits.record.max_record_bytes
        || limits.decode.max_metadata_bytes < 400
        || limits.decode.max_depth < 7
        || limits.decode.max_reservation_bytes < reservation
        || limits.decode.payload.max_logical_bytes < bulk
        || limits.decode.payload.max_segments < segments
        || limits.decode.payload.max_backing_capacity < backing
    {
        return Err(RpcErrorKind::Configuration);
    }
    Ok(())
}

impl<IO: AsyncRead + AsyncWrite> Nfs3Client<IO> {
    pub async fn null(&mut self) -> Result<(), RpcError> {
        let _ = self
            .call::<Void, Void>(NFS_PROGRAM::NFSPROC3_NULL, &Void)
            .await?;
        Ok(())
    }

    pub async fn getattr(&mut self, args: &GETATTR3args) -> Result<GETATTR3res, RpcError> {
        self.call::<GETATTR3args, GETATTR3res>(NFS_PROGRAM::NFSPROC3_GETATTR, args)
            .await
    }

    pub async fn setattr(&mut self, args: &SETATTR3args) -> Result<SETATTR3res, RpcError> {
        self.call::<SETATTR3args, SETATTR3res>(NFS_PROGRAM::NFSPROC3_SETATTR, args)
            .await
    }

    pub async fn lookup(&mut self, args: &LOOKUP3args<'_>) -> Result<LOOKUP3res, RpcError> {
        self.call::<LOOKUP3args, LOOKUP3res>(NFS_PROGRAM::NFSPROC3_LOOKUP, args)
            .await
    }

    pub async fn access(&mut self, args: &ACCESS3args) -> Result<ACCESS3res, RpcError> {
        self.call::<ACCESS3args, ACCESS3res>(NFS_PROGRAM::NFSPROC3_ACCESS, args)
            .await
    }

    pub async fn readlink(
        &mut self,
        args: &READLINK3args,
    ) -> Result<READLINK3res<'static>, RpcError> {
        self.call::<READLINK3args, READLINK3res>(NFS_PROGRAM::NFSPROC3_READLINK, args)
            .await
    }

    /// Return shared charged backing, validating count against the request.
    pub async fn read(&mut self, args: &READ3args) -> Result<READ3res<Payload>, RpcError> {
        if args.count > self.max_bulk_bytes {
            return Err(self.rpc.argument_error());
        }
        let mut decode = self.rpc.limits().decode;
        decode.payload.max_logical_bytes =
            decode.payload.max_logical_bytes.min(args.count as usize);
        let result: READ3res<Payload> = self
            .rpc
            .call_unvalidated(
                PROGRAM,
                VERSION,
                NFS_PROGRAM::NFSPROC3_READ as u32,
                args,
                decode,
            )
            .await?;
        if let Nfs3Result::Ok(value) = &result {
            if value.data.len() != value.count as usize || value.count > args.count {
                return Err(self.rpc.semantic_error());
            }
        }
        self.rpc.validated();
        Ok(result)
    }

    /// Borrow caller-owned bulk and derive the transmitted count. A started,
    /// unusable WRITE has an uncertain outcome, not a safe retry indication.
    pub async fn write(
        &mut self,
        file: nfs_fh3,
        offset: u64,
        stable: stable_how,
        data: &Payload,
    ) -> Result<WRITE3res, RpcError> {
        let count = u32::try_from(data.len()).map_err(|_overflow| self.rpc.argument_error())?;
        if count > self.max_bulk_bytes {
            return Err(self.rpc.argument_error());
        }
        let args = WRITE3args {
            file,
            offset,
            count,
            stable,
            data,
        };
        let result: WRITE3res = self
            .rpc
            .call_unvalidated(
                PROGRAM,
                VERSION,
                NFS_PROGRAM::NFSPROC3_WRITE as u32,
                &args,
                self.rpc.limits().decode,
            )
            .await?;
        if let Nfs3Result::Ok(value) = &result {
            if value.count > count {
                return Err(self.rpc.semantic_error());
            }
        }
        self.rpc.validated();
        Ok(result)
    }

    pub async fn create(&mut self, args: &CREATE3args<'_>) -> Result<CREATE3res, RpcError> {
        self.call::<CREATE3args, CREATE3res>(NFS_PROGRAM::NFSPROC3_CREATE, args)
            .await
    }

    pub async fn mkdir(&mut self, args: &MKDIR3args<'_>) -> Result<MKDIR3res, RpcError> {
        self.call::<MKDIR3args, MKDIR3res>(NFS_PROGRAM::NFSPROC3_MKDIR, args)
            .await
    }

    pub async fn symlink(&mut self, args: &SYMLINK3args<'_>) -> Result<SYMLINK3res, RpcError> {
        self.call::<SYMLINK3args, SYMLINK3res>(NFS_PROGRAM::NFSPROC3_SYMLINK, args)
            .await
    }

    pub async fn mknod(&mut self, args: &MKNOD3args<'_>) -> Result<MKNOD3res, RpcError> {
        self.call::<MKNOD3args, MKNOD3res>(NFS_PROGRAM::NFSPROC3_MKNOD, args)
            .await
    }

    pub async fn remove(&mut self, args: &REMOVE3args<'_>) -> Result<REMOVE3res, RpcError> {
        self.call::<REMOVE3args, REMOVE3res>(NFS_PROGRAM::NFSPROC3_REMOVE, args)
            .await
    }

    pub async fn rmdir(&mut self, args: &RMDIR3args<'_>) -> Result<RMDIR3res, RpcError> {
        self.call::<RMDIR3args, RMDIR3res>(NFS_PROGRAM::NFSPROC3_RMDIR, args)
            .await
    }

    pub async fn rename(&mut self, args: &RENAME3args<'_, '_>) -> Result<RENAME3res, RpcError> {
        self.call::<RENAME3args, RENAME3res>(NFS_PROGRAM::NFSPROC3_RENAME, args)
            .await
    }

    pub async fn link(&mut self, args: &LINK3args<'_>) -> Result<LINK3res, RpcError> {
        self.call::<LINK3args, LINK3res>(NFS_PROGRAM::NFSPROC3_LINK, args)
            .await
    }

    pub async fn readdir(&mut self, args: &READDIR3args) -> Result<READDIR3res<'static>, RpcError> {
        self.call::<READDIR3args, READDIR3res>(NFS_PROGRAM::NFSPROC3_READDIR, args)
            .await
    }

    pub async fn readdirplus(
        &mut self,
        args: &READDIRPLUS3args,
    ) -> Result<READDIRPLUS3res<'static>, RpcError> {
        self.call::<READDIRPLUS3args, READDIRPLUS3res>(NFS_PROGRAM::NFSPROC3_READDIRPLUS, args)
            .await
    }

    pub async fn fsstat(&mut self, args: &FSSTAT3args) -> Result<FSSTAT3res, RpcError> {
        self.call::<FSSTAT3args, FSSTAT3res>(NFS_PROGRAM::NFSPROC3_FSSTAT, args)
            .await
    }

    pub async fn fsinfo(&mut self, args: &FSINFO3args) -> Result<FSINFO3res, RpcError> {
        self.call::<FSINFO3args, FSINFO3res>(NFS_PROGRAM::NFSPROC3_FSINFO, args)
            .await
    }

    pub async fn pathconf(&mut self, args: &PATHCONF3args) -> Result<PATHCONF3res, RpcError> {
        self.call::<PATHCONF3args, PATHCONF3res>(NFS_PROGRAM::NFSPROC3_PATHCONF, args)
            .await
    }

    pub async fn commit(&mut self, args: &COMMIT3args) -> Result<COMMIT3res, RpcError> {
        self.call::<COMMIT3args, COMMIT3res>(NFS_PROGRAM::NFSPROC3_COMMIT, args)
            .await
    }

    pub async fn call<C, R>(&mut self, proc: NFS_PROGRAM, args: &C) -> Result<R, RpcError>
    where
        R: Decode,
        C: Encode + Sync,
    {
        self.rpc
            .call::<C, R>(PROGRAM, VERSION, proc as u32, args)
            .await
    }
}
