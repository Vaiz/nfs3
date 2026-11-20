use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::PayloadError;

struct Domain {
    limit: usize,
    used: AtomicUsize,
}

/// Finite backing-capacity domain. Not an allocator or a process-memory cap.
///
/// Accounts unused reservations plus live mutable/frozen Vec capacity tickets.
/// Release is lock-free and infallible. Allocator usable size, Arc overhead and
/// independent application metadata are excluded.
#[derive(Clone)]
pub struct BackingBudget(Arc<Domain>);

impl BackingBudget {
    /// Create an explicit finite domain (zero permits no backing).
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self(Arc::new(Domain {
            limit,
            used: AtomicUsize::new(0),
        }))
    }

    /// Outstanding reserved and live backing bytes.
    #[must_use]
    pub fn used(&self) -> usize {
        self.0.used.load(Ordering::Acquire)
    }

    /// Atomically reserve the entire entitlement, without waiting.
    pub fn try_reserve(&self, bytes: usize) -> Result<BackingReservation, PayloadError> {
        self.0
            .used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes).filter(|sum| *sum <= self.0.limit)
            })
            .map_err(|_| PayloadError::Allocation)?;
        Ok(BackingReservation {
            ticket: Ticket {
                domain: Arc::clone(&self.0),
                bytes,
            },
        })
    }
}

/// Unused entitlement; dropping it returns only its unspent bytes.
pub struct BackingReservation {
    ticket: Ticket,
}

impl BackingReservation {
    /// Unspent entitlement.
    #[must_use]
    pub const fn remaining(&self) -> usize {
        self.ticket.bytes
    }

    pub(super) fn take(&mut self, bytes: usize) -> Result<Ticket, PayloadError> {
        self.ticket.bytes = self
            .ticket
            .bytes
            .checked_sub(bytes)
            .ok_or(PayloadError::Allocation)?;
        Ok(Ticket {
            domain: Arc::clone(&self.ticket.domain),
            bytes,
        })
    }
}

pub(super) struct Ticket {
    domain: Arc<Domain>,
    bytes: usize,
}

impl Drop for Ticket {
    fn drop(&mut self) {
        self.domain.used.fetch_sub(self.bytes, Ordering::Release);
    }
}
