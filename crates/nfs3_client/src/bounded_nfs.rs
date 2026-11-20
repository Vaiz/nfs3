//! Minimal NFS READ/WRITE policy on the structural bounded RPC client.

use nfs3_types::nfs3::{
    self, NFS_PROGRAM, Nfs3Result, READ3args, READ3resPayload, WRITE3res, Write3Args, nfs_fh3,
    stable_how,
};
use nfs3_types::xdr_codec::{Decode, Encode, Payload};

use crate::bounded::{BoundedError, BoundedErrorKind, BoundedRpcClient};
use crate::io::{AsyncRead, AsyncWrite};

/// NFS wrapper with explicit maximum bulk length; generic RPC feasibility is
/// separate. No automatic retry or uncertain-WRITE recovery is provided.
pub struct BoundedNfsClient<IO> {
    rpc: BoundedRpcClient<IO>,
    max_bulk_bytes: u32,
}

impl<IO> BoundedNfsClient<IO> {
    /// Validate maximum READ/WRITE reply wire size, nesting, verifier/descriptor
    /// reservation work and unaligned bulk retention before IO. Generic RPC may
    /// use smaller caller-chosen limits. Actual WRITE encoding is checked before
    /// IO. Values are not defaults.
    pub fn new(rpc: BoundedRpcClient<IO>, max_bulk_bytes: u32) -> Result<Self, BoundedErrorKind> {
        let limits = rpc.limits();
        let bulk = max_bulk_bytes as usize;
        let padded = bulk
            .checked_add(3)
            .map(|n| n & !3)
            .ok_or(BoundedErrorKind::Configuration)?;
        let record_blocks = limits.max_record_bytes.div_ceil(limits.block_bytes);
        let segments = if bulk == 0 {
            0
        } else {
            bulk.div_ceil(limits.block_bytes)
                .checked_add(1)
                .ok_or(BoundedErrorKind::Configuration)?
                .min(record_blocks)
        };
        let backing = segments
            .checked_mul(limits.block_bytes.min(limits.max_record_bytes))
            .ok_or(BoundedErrorKind::Configuration)?
            .min(limits.max_record_bytes);
        // READ: 24 RPC + 400 auth + 4 status + 88 post_op_attr + 12 data fields.
        let read_wire = padded
            .checked_add(528)
            .ok_or(BoundedErrorKind::Configuration)?;
        // WRITE: 24 RPC + 400 auth + 4 status + (28 pre + 88 post) wcc
        // + 4 count + 4 stability + 8 verifier. Failure omits the final 16.
        let write_wire = 424_usize
            .checked_add(4)
            .and_then(|n| n.checked_add(28 + 88))
            .and_then(|n| n.checked_add(16))
            .ok_or(BoundedErrorKind::Configuration)?;
        let reservation = Payload::descriptor_reservation_cost(segments)
            .and_then(|n| n.checked_add(400))
            .ok_or(BoundedErrorKind::Configuration)?;
        // Deepest WRITE path: result -> ok/fail -> wcc -> option -> fattr
        // -> time/specdata -> u32. READ needs 6; the RPC header also needs 7
        // (rpc_msg -> msg_body -> reply_body -> accepted -> auth -> flavor -> u32).
        if read_wire.max(write_wire) > limits.max_record_bytes
            || limits.decode.max_metadata_bytes < 400
            || limits.decode.max_depth < 7
            || limits.decode.max_reservation_bytes < reservation
            || limits.decode.payload.max_logical_bytes < bulk
            || limits.decode.payload.max_segments < segments
            || limits.decode.payload.max_backing_capacity < backing
        {
            return Err(BoundedErrorKind::Configuration);
        }
        Ok(Self {
            rpc,
            max_bulk_bytes,
        })
    }

    #[must_use]
    pub const fn rpc(&self) -> &BoundedRpcClient<IO> {
        &self.rpc
    }
}

impl<IO: AsyncRead + AsyncWrite> BoundedNfsClient<IO> {
    /// Other procedures use the generic structural codec path, not duplicated APIs.
    pub async fn call<C: Encode + Sync, R: Decode>(
        &mut self,
        procedure: NFS_PROGRAM,
        args: &C,
    ) -> Result<R, BoundedError> {
        self.rpc
            .call(nfs3::PROGRAM, nfs3::VERSION, procedure as u32, args)
            .await
    }

    /// READ returns shared Payload backing. Count/data mismatches poison the
    /// connection as local response-validation policy, even at a record boundary.
    pub async fn read(&mut self, args: &READ3args) -> Result<READ3resPayload, BoundedError> {
        if args.count > self.max_bulk_bytes {
            return Err(self.rpc.argument_error());
        }
        let mut decode = self.rpc.limits().decode;
        decode.payload.max_logical_bytes =
            decode.payload.max_logical_bytes.min(args.count as usize);
        let result: READ3resPayload = self
            .rpc
            .call_unvalidated(
                nfs3::PROGRAM,
                nfs3::VERSION,
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

    /// Derive transmitted count from Payload length. A started unusable WRITE
    /// has an uncertain outcome; the caller must not infer that retry is safe.
    pub async fn write(
        &mut self,
        file: nfs_fh3,
        offset: u64,
        stable: stable_how,
        data: Payload,
    ) -> Result<WRITE3res, BoundedError> {
        let count = u32::try_from(data.len()).map_err(|_| self.rpc.argument_error())?;
        if count > self.max_bulk_bytes {
            return Err(self.rpc.argument_error());
        }
        let args = Write3Args {
            file,
            offset,
            count,
            stable,
            data,
        };
        let result: WRITE3res = self
            .rpc
            .call_unvalidated(
                nfs3::PROGRAM,
                nfs3::VERSION,
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
}
