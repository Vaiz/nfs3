//! Allocation-free, OS-thread-local logical RPC observations.
//!
//! Not physical copy totals or task attribution. Reset only with no outstanding
//! instrumented work.
//! Cancelled losing socket-select futures and initiated socket reads are NOT
//! instrumented. Marker scratch initialization is counted separately.
use std::cell::Cell;
use std::io;

pub use crate::xdr_codec::opaque_diagnostics::Count;

/// Existing awaited IO, encode, initialization and explicit test-copy sites.
#[derive(Clone, Copy, Debug)]
#[repr(usize)]
pub enum Site {
    /// Client record encoding, including marker.
    ClientEncode,
    /// Client record send.
    ClientSend,
    /// Client reply marker read.
    ClientMarker,
    /// Client reply body read.
    ClientBody,
    /// Client reply body initialization.
    ClientZero,
    /// Client four-byte marker scratch initialization.
    ClientMarkerZero,
    /// Server socket scratch initialization (128 KiB).
    ServerScratch,
    /// Socket-to-internal-duplex forwarding.
    Forward,
    /// Server fragment marker read.
    ServerMarker,
    /// Server fragment body read.
    ServerBody,
    /// Server resize initialization.
    ServerZero,
    /// Server four-byte marker scratch initialization.
    ServerMarkerZero,
    /// Reply encoding, excluding marker.
    ServerEncode,
    /// Reply marker send.
    ServerSendMarker,
    /// Reply body send.
    ServerSendBody,
    /// `ReadFs` explicit `to_vec` copy.
    ReadCopy,
    /// `WriteFs` consumed data observation, not a copy.
    WriteConsumed,
    /// Harness transcript copy, excluded from product claims.
    TestCopy,
}

const SITES: usize = Site::TestCopy as usize + 1;

/// Selected socket outcomes only: no attempt or cancellation claims.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SelectedSocketReads {
    /// Successful nonempty reads.
    pub ok_calls: u64,
    /// Successful nonempty bytes.
    pub ok_bytes: u64,
    /// Selected Ok(0) outcomes.
    pub eof: u64,
    /// Selected `WouldBlock` errors.
    pub would_block: u64,
    /// Other selected errors.
    pub other_errors: u64,
    /// Saturating overflow loss.
    pub loss: u64,
}

/// Capacity changes are observations, NOT reallocations or actual copy bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Growth {
    /// Successful resizes, including capacity-preserving resizes.
    pub observations: u64,
    /// Resizes for which capacity increased.
    pub capacity_increases: u64,
    /// Sum of old logical lengths at capacity increases: relocation upper bound.
    pub old_logical_upper_bound: u64,
}

/// Synchronous local guard observations; normal end must precede guard drop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Lifecycle {
    /// Bodies entered.
    pub starts: u64,
    /// Bodies reaching normal exit.
    pub normal_ends: u64,
    /// Guards dropped, including cancellation/unwinding.
    pub drops: u64,
}

/// Fixed counters; no callbacks, queues, heap allocations or task-local storage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ledger {
    /// Counters indexed by Site.
    pub counts: [Count; SITES],
    /// Socket outcomes are deliberately separate from Count.
    pub selected: SelectedSocketReads,
    /// Fragment Vec growth observations.
    pub growth: Growth,
    /// Socket, reader and handler local guards, respectively.
    pub lifecycle: [Lifecycle; 3],
    /// Server marker `UnexpectedEof` results, including possible partial markers.
    pub marker_eof: u64,
    /// Other server marker errors.
    pub marker_errors: u64,
    /// Observed `WriteFs` bytes not equal to the fixture pattern 0xAA.
    pub bad_write_bytes: u64,
    /// Overflow loss outside selected socket outcomes.
    pub loss: u64,
}

thread_local! {
    static LEDGER: Cell<Ledger> = Cell::new(Ledger::default());
}

fn update(f: impl FnOnce(&mut Ledger)) {
    LEDGER.with(|cell| {
        let mut ledger = cell.get();
        f(&mut ledger);
        cell.set(ledger);
    });
}

const fn add(value: &mut u64, increment: u64, loss: &mut u64) {
    if let Some(sum) = value.checked_add(increment) {
        *value = sum;
    } else {
        *value = u64::MAX;
        *loss = loss.saturating_add(1);
    }
}

/// Return and clear this thread's RPC ledger (not the separate opaque ledger).
#[must_use]
pub fn take() -> Ledger {
    LEDGER.with(Cell::take)
}

/// Inspect without resetting beneath pending work; collect only after quiescence.
#[must_use]
pub fn snapshot() -> Ledger {
    LEDGER.with(Cell::get)
}

/// Observe an existing operation's attempt or successful completion.
pub fn record(site: Site, bytes: usize, completed: bool) {
    update(|ledger| {
        let count = &mut ledger.counts[site as usize];
        let (calls, total) = if completed {
            (&mut count.completions, &mut count.completed_bytes)
        } else {
            (&mut count.attempts, &mut count.attempted_bytes)
        };
        add(calls, 1, &mut ledger.loss);
        add(total, bytes as u64, &mut ledger.loss);
    });
}

/// Observe a completed existing initialization or synchronous copy.
pub fn completed(site: Site, bytes: usize) {
    record(site, bytes, false);
    record(site, bytes, true);
}

/// Observe by reference at the beginning of the selected branch body.
pub fn selected(result: &io::Result<usize>) {
    update(|ledger| {
        let reads = &mut ledger.selected;
        match result {
            Ok(0) => add(&mut reads.eof, 1, &mut reads.loss),
            Ok(n) => {
                add(&mut reads.ok_calls, 1, &mut reads.loss);
                add(&mut reads.ok_bytes, *n as u64, &mut reads.loss);
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                add(&mut reads.would_block, 1, &mut reads.loss);
            }
            Err(_) => add(&mut reads.other_errors, 1, &mut reads.loss),
        }
    });
}

/// Observe lengths/capacities only after the existing resize succeeds.
pub fn growth(old_len: usize, old_capacity: usize, new_capacity: usize) {
    update(|ledger| {
        add(&mut ledger.growth.observations, 1, &mut ledger.loss);
        if new_capacity > old_capacity {
            add(&mut ledger.growth.capacity_increases, 1, &mut ledger.loss);
            add(
                &mut ledger.growth.old_logical_upper_bound,
                old_len as u64,
                &mut ledger.loss,
            );
        }
    });
}

/// Observe the unchanged marker read result before propagating it.
pub fn marker_result<T>(result: &io::Result<T>) {
    if let Err(e) = result {
        update(|ledger| {
            let field = if e.kind() == io::ErrorKind::UnexpectedEof {
                &mut ledger.marker_eof
            } else {
                &mut ledger.marker_errors
            };
            add(field, 1, &mut ledger.loss);
        });
    }
}

/// Validate sink input without allocating or changing the backend result.
pub fn consumed(data: &[u8]) {
    completed(Site::WriteConsumed, data.len());
    let bad = data.iter().filter(|&&byte| byte != 0xAA).count();
    update(|ledger| add(&mut ledger.bad_write_bytes, bad as u64, &mut ledger.loss));
}

/// Local synchronous lifecycle guard; never retains or joins product tasks.
pub struct Guard(usize);

impl Guard {
    /// Start socket (0), reader (1), or handler (2) body observation.
    #[must_use]
    pub fn start(index: usize) -> Self {
        update(|ledger| add(&mut ledger.lifecycle[index].starts, 1, &mut ledger.loss));
        Self(index)
    }

    /// Record the body's normal end without changing drop timing.
    pub fn normal_end(&self) {
        update(|ledger| {
            add(
                &mut ledger.lifecycle[self.0].normal_ends,
                1,
                &mut ledger.loss,
            );
        });
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        update(|ledger| add(&mut ledger.lifecycle[self.0].drops, 1, &mut ledger.loss));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcomes_growth_and_partial_operations() {
        let _ = take();
        selected(&Ok(7));
        selected(&Ok(0));
        selected(&Err(io::ErrorKind::WouldBlock.into()));
        selected(&Err(io::ErrorKind::BrokenPipe.into()));
        record(Site::Forward, 9, false);
        growth(5, 16, 16);
        growth(9, 16, 32);
        marker_result::<()>(&Err(io::ErrorKind::UnexpectedEof.into()));
        consumed(&[0xAA, 0, 0xAA]);
        let ledger = take();
        assert_eq!(
            ledger.selected,
            SelectedSocketReads {
                ok_calls: 1,
                ok_bytes: 7,
                eof: 1,
                would_block: 1,
                other_errors: 1,
                loss: 0,
            }
        );
        assert_eq!(
            ledger.growth,
            Growth {
                observations: 2,
                capacity_increases: 1,
                old_logical_upper_bound: 9,
            }
        );
        assert_eq!(ledger.counts[Site::Forward as usize].completions, 0);
        assert_eq!(ledger.marker_eof, 1);
        assert_eq!(ledger.bad_write_bytes, 1);
        assert_eq!(take(), Ledger::default());
    }

    #[test]
    fn overflow_and_cancelled_guard_are_visible() {
        let _ = take();
        LEDGER.with(|cell| {
            let mut ledger = Ledger::default();
            ledger.selected.ok_bytes = u64::MAX;
            ledger.counts[Site::Forward as usize].attempts = u64::MAX;
            ledger.growth.old_logical_upper_bound = u64::MAX;
            cell.set(ledger);
        });
        selected(&Ok(1));
        record(Site::Forward, 1, false);
        growth(1, 1, 2);
        drop(Guard::start(1));
        let ledger = take();
        assert_eq!(ledger.selected.loss, 1);
        assert_eq!(ledger.loss, 2);
        assert_eq!(
            ledger.lifecycle[1],
            Lifecycle {
                starts: 1,
                normal_ends: 0,
                drops: 1
            }
        );
    }
}
