//! Opt-in, allocation-free opaque codec ledger for the current OS thread.
//!
//! These are logical IO boundary bytes and explicit initialization bytes, not
//! allocator events or proof of physical copies. Failed `read_exact`/`write_all`
//! may transfer an unknown prefix: attempted bytes are NOT completed bytes.
//! Length-prefix IO, implicit allocator work, `Cow` clones and other codecs are
//! excluded. No lifetime, retention, capacity or growth-copy inference is made.
//! Reset with [`take`] before synchronous work and collect on the same thread.
//! This is not async task attribution; unrelated work on that thread is included.

use std::cell::Cell;

/// One bounded aggregate. Initialization records an attempt and completion only
/// after the existing initialization returns; allocation failure is not captured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Count {
    /// Number of calls begun, including zero-length calls.
    pub attempts: u64,
    /// Total requested bytes, including incomplete calls.
    pub attempted_bytes: u64,
    /// Number of calls that returned successfully.
    pub completions: u64,
    /// Requested bytes of successful calls only, not partial failed transfers.
    pub completed_bytes: u64,
}

/// Six fixed counters; no event queue, heap allocation, callbacks or recursion.
/// Overflow saturates and increments `lost_updates`; any loss invalidates totals.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ledger {
    /// Payload `write_all` calls, excluding the length prefix.
    pub encode_payload: Count,
    /// Padding `write_all` calls (including zero-length calls).
    pub encode_padding: Count,
    /// Payload `read_exact` calls.
    pub decode_payload: Count,
    /// Padding `read_exact` calls (only when padding is nonempty).
    pub decode_padding: Count,
    /// Explicit `vec![0; len]` initialization.
    pub payload_zero_fill: Count,
    /// Explicit `[0; 4]` initialization, including unused padding scratch bytes.
    pub padding_zero_fill: Count,
    /// Saturating count of overflowing field updates.
    pub lost_updates: u64,
}

thread_local! {
    static LEDGER: Cell<Ledger> = Cell::new(Ledger::default());
}

/// Return and clear this thread's ledger. Does not reset any allocator recorder.
#[must_use]
pub fn take() -> Ledger {
    LEDGER.with(Cell::take)
}

#[derive(Clone, Copy)]
pub(crate) enum Site {
    EncodePayload,
    EncodePadding,
    DecodePayload,
    DecodePadding,
    PayloadZeroFill,
    PaddingZeroFill,
}

pub(crate) fn record(site: Site, bytes: usize, completed: bool) {
    LEDGER.with(|cell| {
        let mut ledger = cell.get();
        let counter = match site {
            Site::EncodePayload => &mut ledger.encode_payload,
            Site::EncodePadding => &mut ledger.encode_padding,
            Site::DecodePayload => &mut ledger.decode_payload,
            Site::DecodePadding => &mut ledger.decode_padding,
            Site::PayloadZeroFill => &mut ledger.payload_zero_fill,
            Site::PaddingZeroFill => &mut ledger.padding_zero_fill,
        };
        let (calls, total) = if completed {
            (&mut counter.completions, &mut counter.completed_bytes)
        } else {
            (&mut counter.attempts, &mut counter.attempted_bytes)
        };
        for (field, increment) in [(calls, 1), (total, bytes as u64)] {
            if let Some(value) = field.checked_add(increment) {
                *field = value;
            } else {
                *field = u64::MAX;
                ledger.lost_updates = ledger.lost_updates.saturating_add(1);
            }
        }
        cell.set(ledger);
    });
}

pub(crate) fn initialized(site: Site, bytes: usize) {
    record(site, bytes, false);
    record(site, bytes, true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overflow_is_explicit_and_resettable() {
        let _ = take();
        LEDGER.with(|cell| {
            cell.set(Ledger {
                encode_payload: Count {
                    attempts: u64::MAX,
                    attempted_bytes: u64::MAX,
                    ..Count::default()
                },
                ..Ledger::default()
            });
        });
        record(Site::EncodePayload, 1, false);
        let ledger = take();
        assert_eq!(ledger.encode_payload.attempts, u64::MAX);
        assert_eq!(ledger.encode_payload.attempted_bytes, u64::MAX);
        assert_eq!(ledger.lost_updates, 2);
        assert_eq!(take(), Ledger::default());
    }

    #[test]
    fn ledgers_are_thread_local() {
        let _ = take();
        record(Site::EncodePayload, 3, false);
        let other = std::thread::spawn(|| {
            assert_eq!(take(), Ledger::default());
            initialized(Site::PayloadZeroFill, 7);
            take()
        })
        .join()
        .expect("ledger thread");
        assert_eq!(other.payload_zero_fill.completed_bytes, 7);
        let local = take();
        assert_eq!(local.encode_payload.attempted_bytes, 3);
        assert_eq!(local.payload_zero_fill, Count::default());
    }
}
