//! Owned bulk IO capability.
//!
//! Implementations must move requests into the actual
//! work item, including blocking jobs, rather than retaining only borrowed data.
//! Arbitrary persistent backend allocations are outside the listener domain.

use std::sync::Arc;

use nfs3_types::nfs3::{fattr3, nfsstat3, stable_how};
use nfs3_types::xdr_codec::{BackingReservation, Payload, PayloadLimits, ReceiveBuffer};
use tokio::sync::OwnedSemaphorePermit;

use super::{NfsFileSystem, NfsReadFileSystem};

pub(crate) struct Lease {
    // Drop entitlement before publishing the slot as free. Drain uses the
    // operation semaphore as the completion barrier.
    pub(crate) _control: BackingReservation,
    pub(crate) _slot: OwnedSemaphorePermit,
}

/// Private shared ownership prevents an outer future from reclaiming live work.
pub(crate) type WorkLease = Arc<Lease>;

pub struct OwnedRead<H> {
    pub handle: H,
    pub offset: u64,
    buffer: ReceiveBuffer,
    limits: PayloadLimits,
    lease: WorkLease,
}

impl<H> OwnedRead<H> {
    pub(crate) const fn new(
        handle: H,
        offset: u64,
        buffer: ReceiveBuffer,
        limits: PayloadLimits,
        lease: WorkLease,
    ) -> Self {
        Self {
            handle,
            offset,
            buffer,
            limits,
            lease,
        }
    }
    /// Initialized writable storage, allocated before backend execution.
    pub fn buffer(&mut self) -> &mut [u8] {
        self.buffer.as_mut_slice()
    }
    /// Publish only a checked prefix of the supplied backing. No replacement Vec.
    pub fn complete(self, count: usize, eof: bool) -> Result<OwnedReadResult, nfsstat3> {
        let payload = self
            .buffer
            .freeze(count, self.limits)
            .map_err(|_| nfsstat3::NFS3ERR_SERVERFAULT)?;
        Ok(OwnedReadResult {
            payload,
            eof,
            _lease: self.lease,
        })
    }
}

pub struct OwnedReadResult {
    pub(crate) payload: Payload,
    pub(crate) eof: bool,
    pub(crate) _lease: WorkLease,
}

impl OwnedReadResult {
    #[must_use]
    pub const fn payload(&self) -> &Payload {
        &self.payload
    }
    #[must_use]
    pub const fn eof(&self) -> bool {
        self.eof
    }
}

pub struct OwnedWrite<H> {
    pub handle: H,
    pub offset: u64,
    pub stable: stable_how,
    pub data: Payload,
    pub(crate) _lease: WorkLease,
}

/// No blanket copying adapter from legacy Vec/slice IO is supplied.
pub trait NfsOwnedRead: NfsReadFileSystem {
    fn read_owned(
        &self,
        request: OwnedRead<Self::Handle>,
    ) -> impl Future<Output = Result<OwnedReadResult, nfsstat3>> + Send;
}

pub trait NfsOwnedIo: NfsFileSystem + NfsOwnedRead {
    fn write_owned(
        &self,
        request: OwnedWrite<Self::Handle>,
    ) -> impl Future<Output = Result<(fattr3, stable_how), nfsstat3>> + Send;
}
