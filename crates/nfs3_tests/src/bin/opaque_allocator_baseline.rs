//! B0.1 diagnostics only: build this executable with `allocator-diagnostics`.
//!
//! The global allocator is replaced only in this executable. No timing is
//! collected: normal-allocator timing must run separately. The bounded,
//! single-thread fixture checks event transport, not exact process memory.
//! Initial live allocations are absent; addresses/`allocation_id` are only
//! correlation keys, not lifetime IDs. Realloc overlap, usable bytes, process
//! peak, retention and cross-thread order remain unknown.

use std::alloc::{Layout, alloc, dealloc, handle_alloc_error, realloc};

use nfs3_types::xdr_codec::opaque_diagnostics::{Count, Ledger, take};
use nfs3_types::xdr_codec::{Opaque, Pack, Unpack};
use seismograph::recorder::event::{Address, EventKind, Events};
use seismograph::recorder::{Configuration, EventBufferCapacity, Policy, without_recording};
use seismograph::snapshot::{EventBufferDisposition, SnapshotOptions};

rallocator::rallocator!();

// Minimum supported capacity; stack capture is off, but ring slots still contain
// fixed stack storage. This is a per-thread event bound, not a byte-memory limit.
const CAPACITY: usize = 64;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    seismograph::recorder(Configuration {
        allocations: Policy::all(false),
        event_capacity_per_thread: EventBufferCapacity::new(CAPACITY),
        ..Configuration::default()
    });
    let (old, new) = allocation_fixture();
    let ledger = opaque_fixture();
    let snapshot = seismograph::snapshot(SnapshotOptions {
        event_buffers: EventBufferDisposition::Stop,
    })?;
    // Keep decode/output allocations out of the capture even if configuration
    // changes later. Suppression is synchronous/thread-local, not task identity.
    without_recording(|| -> Result<(), Box<dyn std::error::Error>> {
        let decoded = seismograph::snapshot::decode(snapshot.as_bytes())?;
        check_capture(&decoded.events)?;
        check_allocation_fixture(&decoded.events, old, new)?;
        println!("B0.1 diagnostic; capacity_per_thread={CAPACITY}; stacks=false; stopped=true");
        println!(
            "events={} retained={} lost={} threads={}",
            decoded.events.total_events,
            decoded.events.events.len(),
            decoded.events.lost_events,
            decoded.events.threads.len(),
        );
        for event in &decoded.events.events {
            if let Some(allocation) = event.allocation() {
                println!(
                    "thread={} sequence={} kind={:?} address={} correlation={} size={} \
                     alignment={}",
                    event.thread_id.get(),
                    event.sequence.get(),
                    event.kind,
                    allocation.address.get(),
                    allocation.allocation_id.get(),
                    allocation.size,
                    allocation.alignment,
                );
            }
        }
        println!("opaque={ledger:?}");
        println!(
            "fixture=passed; initial_live_set=absent; lifetime_ids=absent; \
             cross_thread_order=unvalidated"
        );
        println!(
            "process_peak=unknown; retention=unknown; realloc_overlap=unknown; timing=not_measured"
        );
        Ok(())
    })
}

fn allocation_fixture() -> (Address, Address) {
    let small = Layout::from_size_align(37, 8).expect("fixed layout");
    let large = Layout::from_size_align(73, 8).expect("fixed layout");
    // SAFETY: layouts are nonzero and valid. Each successful allocation is
    // uniquely owned here; writes/reads stay within initialized bytes. On realloc
    // failure the old block remains valid and is freed before aborting. Success
    // invalidates the old pointer; only its numeric address is retained, and the
    // new block is freed once with its matching size/alignment. Nothing escapes.
    unsafe {
        let old = alloc(small);
        if old.is_null() {
            handle_alloc_error(small);
        }
        old.write_bytes(0xa5, small.size());
        let old_address = Address::from_ptr(old);
        let new = realloc(old, small, large.size());
        if new.is_null() {
            dealloc(old, small);
            handle_alloc_error(large);
        }
        let preserved = new.read() == 0xa5 && new.add(small.size() - 1).read() == 0xa5;
        let new_address = Address::from_ptr(new);
        dealloc(new, large);
        assert!(preserved, "realloc must preserve the initialized prefix");
        (old_address, new_address)
    }
}

const fn done(bytes: u64) -> Count {
    Count {
        attempts: 1,
        attempted_bytes: bytes,
        completions: 1,
        completed_bytes: bytes,
    }
}

fn opaque_fixture() -> Ledger {
    let _ = take();
    // Hand-authored wire bytes and fixed output avoid a test-transport Vec copy.
    let wire = [0, 0, 0, 1, b'a', 0, 0, 0];
    let mut encoded = [0u8; 8];
    assert_eq!(
        Opaque::borrowed(b"a")
            .pack(&mut encoded.as_mut_slice())
            .expect("pack"),
        8
    );
    assert_eq!(encoded, wire);
    let (decoded, read) = Opaque::unpack(&mut wire.as_slice()).expect("unpack");
    assert_eq!(decoded.as_ref(), b"a");
    assert_eq!(read, 8);
    let ledger = take();
    assert_eq!(
        ledger,
        Ledger {
            encode_payload: done(1),
            encode_padding: done(3),
            decode_payload: done(1),
            decode_padding: done(3),
            payload_zero_fill: done(1),
            padding_zero_fill: done(4),
            lost_updates: 0,
        }
    );
    drop(decoded);
    assert_eq!(take(), Ledger::default(), "drop is not a codec copy event");
    ledger
}

// A zero overwritten-prefix counter alone misses e.g. ring-slot lock timeouts.
// Check vector counts and per-thread sequences too. Deliberately refuse to
// reconstruct cross-thread ordering or infer an initial process live set.
fn check_capture(events: &Events) -> Result<(), &'static str> {
    if events.lost_events != 0 || events.total_events != events.events.len() as u64 {
        return Err("lost or missing events");
    }
    let [thread] = events.threads.as_slice() else {
        return Err("fixture requires exactly one recording thread");
    };
    if thread.lost_events != 0 || thread.total_events != events.total_events {
        return Err("thread totals do not match capture");
    }
    if events
        .events
        .iter()
        .any(|event| event.thread_id != thread.thread_id)
    {
        return Err("event belongs to an unexpected thread");
    }
    if events
        .events
        .windows(2)
        .any(|pair| pair[0].sequence.get().checked_add(1) != Some(pair[1].sequence.get()))
    {
        return Err("missing, duplicated or out-of-order per-thread sequence");
    }
    Ok(())
}

fn check_allocation_fixture(
    events: &Events,
    old: Address,
    new: Address,
) -> Result<(), &'static str> {
    // Only the first four events belong to the explicit fixture. Even an
    // in-place realloc emits free+alloc with the same address: not two lifetimes
    // inferred from an address, and not proof of old+new allocation overlap.
    let expected = [
        (EventKind::Allocation, old, 37),
        (EventKind::Deallocation, old, 37),
        (EventKind::Allocation, new, 73),
        (EventKind::Deallocation, new, 73),
    ];
    let Some(fixture) = events.events.get(..expected.len()) else {
        return Err("allocation fixture events missing");
    };
    for (event, (kind, address, size)) in fixture.iter().zip(expected) {
        let Some(allocation) = event.allocation() else {
            return Err("fixture event has no allocation payload");
        };
        if event.kind != kind
            || allocation.address != address
            || allocation.size != size
            || allocation.alignment != 8
            || !event.call_stack.is_empty()
        {
            return Err("allocation fixture differs from independent expected ledger");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use seismograph::recorder::event::{
        Event, EventPayload, EventSequence, EventTimestamp, ObjectId,
    };
    use seismograph::recorder::thread::{ThreadId, ThreadLog};

    use super::*;

    fn fixture() -> Events {
        let thread_id = ThreadId::new(1);
        Events {
            total_events: 2,
            threads: vec![ThreadLog {
                thread_id,
                total_events: 2,
                ..ThreadLog::default()
            }],
            events: (1..=2)
                .map(|sequence| Event {
                    thread_id,
                    sequence: EventSequence::new(sequence),
                    timestamp: EventTimestamp::from_ticks(sequence),
                    kind: EventKind::ArcCreate,
                    payload: EventPayload::Object(ObjectId::new(1)),
                    call_stack: Vec::new(),
                })
                .collect(),
            ..Events::default()
        }
    }

    #[test]
    fn completeness_rejects_loss_missing_duplicate_order_and_thread_mismatch() {
        assert!(check_capture(&fixture()).is_ok());
        let mut events = fixture();
        events.lost_events = 1;
        assert!(check_capture(&events).is_err());
        let mut events = fixture();
        events.events.pop();
        assert!(check_capture(&events).is_err()); // lost_events alone still zero
        let mut events = fixture();
        events.events[1].sequence = EventSequence::new(1);
        assert!(check_capture(&events).is_err());
        let mut events = fixture();
        events.events[1].sequence = EventSequence::new(3);
        assert!(check_capture(&events).is_err());
        let mut events = fixture();
        events.events.reverse();
        assert!(check_capture(&events).is_err());
        let mut events = fixture();
        events.threads[0].total_events = 3;
        assert!(check_capture(&events).is_err());
        let mut events = fixture();
        events.threads[0].lost_events = 1;
        assert!(check_capture(&events).is_err());
        let mut events = fixture();
        events.events[0].thread_id = ThreadId::new(2);
        assert!(check_capture(&events).is_err());
        let mut events = fixture();
        events.threads.push(ThreadLog::default());
        assert!(check_capture(&events).is_err());
    }
}
