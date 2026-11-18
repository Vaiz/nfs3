//! B0 diagnostics only: build this executable with `allocator-diagnostics`.
//!
//! The global allocator is replaced only in this executable. No timing is
//! collected: normal-allocator timing must run separately. The bounded,
//! single-thread fixture checks event transport, not exact process memory.
//! Initial live allocations are absent; addresses/`allocation_id` are only
//! correlation keys, not lifetime IDs. Realloc overlap, usable bytes, process
//! peak, retention and cross-thread order remain unknown. The separate
//! `--retention` lane measures only requested bytes of four owned opaque outputs.

use std::alloc::{Layout, alloc, dealloc, handle_alloc_error, realloc};
use std::borrow::Cow;
use std::hint::black_box;

use nfs3_types::xdr_codec::opaque_diagnostics::{Count, Ledger, take};
use nfs3_types::xdr_codec::{Opaque, Pack, Unpack};
use seismograph::recorder::event::{Address, EventKind, Events};
use seismograph::recorder::{Configuration, EventBufferCapacity, Policy, without_recording};
use seismograph::snapshot::{EventBufferDisposition, SnapshotOptions};

rallocator::rallocator!();

// Minimum supported capacity; stack capture is off, but ring slots still contain
// fixed stack storage. This is a per-thread event bound, not a byte-memory limit.
const CAPACITY: usize = 64;

const RETENTION_SIZES: [u64; 4] = [4096, 65536, 262144, 1048576];
const RETENTION_LIVE: [u64; 8] = [4096, 69632, 331776, 1380352, 331776, 69632, 4096, 0];
const RETENTION_TOTAL: u64 = 1380352;

#[derive(Clone, Copy, Debug)]
struct Observation {
    address: Address,
    capacity: u64,
    read: u64,
    payload_matches: bool,
}

// Reconstruct lifetimes independently of the fixture schedule. Address is only
// a lookup key into the currently live origins; generations never get recycled.
// The fixed storage is sufficient for this lane, not a general process tracker.
fn retention_lifetimes(events: &Events) -> Result<[(u64, u64); 8], &'static str> {
    check_capture(events)?;
    if events.total_events != 8
        || events.events.first().map(|event| event.sequence.get()) != Some(1)
        || events.events.last().map(|event| event.sequence.get()) != Some(8)
    {
        return Err("retention requires the complete sequence 1 through 8");
    }
    let mut origins = [None; 4];
    let mut generation = 0u64;
    let mut live = 0u64;
    let mut checkpoints = [(0, 0); 8];
    for (event, checkpoint) in events.events.iter().zip(&mut checkpoints) {
        let allocation = event.allocation().ok_or("non-allocation event")?;
        if allocation.address.get() == 0 || !event.call_stack.is_empty() {
            return Err("invalid address or unexpected stack capture");
        }
        let address = allocation.address;
        let identity = match event.kind {
            EventKind::Allocation => {
                if origins
                    .iter()
                    .flatten()
                    .any(|&(key, _, _, _)| key == address)
                {
                    return Err("duplicate live address");
                }
                generation = generation.checked_add(1).ok_or("generation overflow")?;
                live = live
                    .checked_add(allocation.size)
                    .ok_or("scoped byte sum overflow")?;
                let slot = origins
                    .iter_mut()
                    .find(|slot| slot.is_none())
                    .ok_or("too many live outputs")?;
                *slot = Some((address, generation, allocation.size, allocation.alignment));
                generation
            }
            EventKind::Deallocation => {
                let slot = origins
                    .iter_mut()
                    .find(|slot| slot.is_some_and(|(key, _, _, _)| key == address))
                    .ok_or("unmatched, premature or double free")?;
                let (_, identity, size, alignment) = slot.take().ok_or("missing origin")?;
                if size != allocation.size || alignment != allocation.alignment {
                    return Err("free layout differs from origin");
                }
                live = live.checked_sub(size).ok_or("scoped byte sum underflow")?;
                identity
            }
            _ => return Err("unexpected event kind"),
        };
        *checkpoint = (identity, live);
    }
    if live != 0 || origins.iter().any(Option::is_some) {
        return Err("outputs remain live at stop");
    }
    Ok(checkpoints)
}

// This pure validator is shared by the executable and synthetic falsification
// tests. Hand-authored sizes/checkpoints and the logical codec ledger are
// independent checks, not values derived from the reconstructed live set.
fn check_retention(
    events: &Events,
    observations: &[Observation; 4],
    ledger: Ledger,
) -> Result<[(u64, u64); 8], &'static str> {
    let checkpoints = retention_lifetimes(events)?;
    let payload = Count {
        attempts: 4,
        attempted_bytes: RETENTION_TOTAL,
        completions: 4,
        completed_bytes: RETENTION_TOTAL,
    };
    if ledger
        != (Ledger {
            decode_payload: payload,
            payload_zero_fill: payload,
            ..Ledger::default()
        })
    {
        return Err("codec ledger differs from independent expected ledger");
    }
    for (index, event) in events.events.iter().enumerate() {
        let output = if index < 4 { index } else { 7 - index };
        let expected_kind = if index < 4 {
            EventKind::Allocation
        } else {
            EventKind::Deallocation
        };
        let allocation = event.allocation().ok_or("missing allocation payload")?;
        let observation = observations[output];
        let size = RETENTION_SIZES[output];
        if event.kind != expected_kind
            || allocation.address != observation.address
            || allocation.size != size
            || allocation.alignment != 1
            || observation.capacity != size
            || observation.read != size.checked_add(4).ok_or("wire length overflow")?
            || !observation.payload_matches
            || checkpoints[index] != ((output as u64) + 1, RETENTION_LIVE[index])
        {
            return Err("retention differs from independent fixture schedule");
        }
    }
    Ok(checkpoints)
}

fn payload_byte(output: usize, offset: usize) -> u8 {
    ((offset + output * 17) % 255 + 1) as u8
}

fn retention_main() -> Result<(), Box<dyn std::error::Error>> {
    // Independent RFC opaque length headers, not encoded by the codec under
    // test. These sizes are aligned; only the default B0.1 lane tests padding.
    let headers = [[0, 0, 16, 0], [0, 1, 0, 0], [0, 4, 0, 0], [0, 16, 0, 0]];
    let inputs: [Vec<u8>; 4] = std::array::from_fn(|output| {
        let size = usize::try_from(RETENTION_SIZES[output]).expect("fixture size fits usize");
        let mut wire = vec![0; size.checked_add(4).expect("fixed wire length")];
        wire[..4].copy_from_slice(&headers[output]);
        for (offset, byte) in wire[4..].iter_mut().enumerate() {
            *byte = payload_byte(output, offset);
        }
        wire
    });
    let mut outputs: [Option<Opaque<'static>>; 4] = std::array::from_fn(|_| None);
    let mut observations = [Observation {
        address: Address::new(0),
        capacity: 0,
        read: 0,
        payload_matches: false,
    }; 4];
    let _ = take();
    seismograph::recorder(Configuration {
        allocations: Policy::all(false),
        event_capacity_per_thread: EventBufferCapacity::new(CAPACITY),
        ..Configuration::default()
    });
    // No heap metadata, printing, encoding or copied payloads in this window.
    for (output, input) in inputs.iter().enumerate() {
        let mut wire = black_box(input.as_slice());
        let (decoded, read) = Opaque::unpack(&mut wire).expect("fixed opaque fixture");
        let decoded = black_box(decoded);
        let Cow::Owned(bytes) = &decoded.0 else {
            panic!("owned decode required");
        };
        observations[output] = black_box(Observation {
            address: Address::from_ptr(bytes.as_ptr()),
            capacity: u64::try_from(bytes.capacity()).expect("capacity fits u64"),
            read: u64::try_from(read).expect("read fits u64"),
            payload_matches: wire.is_empty()
                && bytes.len() == input.len() - 4
                && bytes
                    .iter()
                    .enumerate()
                    .all(|(offset, &byte)| byte == payload_byte(output, offset)),
        });
        outputs[output] = Some(decoded);
    }
    black_box(&outputs);
    for output in outputs.iter_mut().rev() {
        drop(black_box(output.take()));
    }
    let ledger = black_box(take());
    let snapshot = seismograph::snapshot(SnapshotOptions {
        event_buffers: EventBufferDisposition::Stop,
    })?;
    // Force inputs to remain alive across the stopped snapshot, not merely
    // across decode. Their frees must not masquerade as output frees.
    black_box(&inputs);
    without_recording(|| -> Result<(), Box<dyn std::error::Error>> {
        let decoded = seismograph::snapshot::decode(snapshot.as_bytes())?;
        let checkpoints = check_retention(&decoded.events, &observations, ledger)?;
        println!(
            "B0.2 scoped opaque retention; capacity_per_thread={CAPACITY}; stacks=false; \
             stopped=true"
        );
        println!(
            "events={} retained={} lost={} threads={}; sequence_start=1; sequence_end=8",
            decoded.events.total_events,
            decoded.events.events.len(),
            decoded.events.lost_events,
            decoded.events.threads.len(),
        );
        let names = [
            "decode_4KiB",
            "decode_64KiB",
            "decode_256KiB",
            "decode_1024KiB",
            "drop_1024KiB",
            "drop_256KiB",
            "drop_64KiB",
            "drop_4KiB",
        ];
        for (index, event) in decoded.events.events.iter().enumerate() {
            let allocation = event.allocation().ok_or("validated allocation missing")?;
            let output = if index < 4 { index } else { 7 - index };
            let (generation, live) = checkpoints[index];
            println!(
                "checkpoint={} thread={} sequence={} kind={:?} generation={} address={} \
                 requested={} capacity={} alignment={} scoped_requested_live={} retained_output={}",
                names[index],
                event.thread_id.get(),
                event.sequence.get(),
                event.kind,
                generation,
                allocation.address.get(),
                allocation.size,
                observations[output].capacity,
                allocation.alignment,
                live,
                live,
            );
        }
        println!("opaque={ledger:?}");
        println!(
            "fixture=passed; scoped_initial_live_set=empty; scoped_requested_live=0; \
             scoped_requested_peak={}; scoped_retained_output=0; final_drops=4",
            checkpoints
                .iter()
                .map(|&(_, live)| live)
                .max()
                .ok_or("no checkpoints")?,
        );
        println!(
            "process_initial_live_set=unknown; process_live=unknown; process_peak=unknown; \
             process_retention=unknown; usable_allocator_bytes=unknown; realloc_overlap=unknown; \
             physical_copy_bytes=unknown; cross_thread_attribution=unknown; \
             instrumentation_overhead=unknown; timing=not_measured"
        );
        println!(
            "codec_counts=logical_io_and_explicit_zero_fill; pool=absent; shared_slices=absent; \
             clone_accounting=absent"
        );
        Ok(())
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Argument allocations and lane selection finish before recorder init.
    let retention = {
        let mut args = std::env::args().skip(1);
        match (args.next().as_deref(), args.next()) {
            (None, None) => false,
            (Some("--retention"), None) => true,
            _ => return Err("usage: opaque_allocator_baseline [--retention]".into()),
        }
    };
    if retention {
        return retention_main();
    }
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
    use seismograph::recorder::alloc::{Allocation, AllocationId, EventThreadId, HeapId, HeapKind};
    use seismograph::recorder::event::{
        Event, EventPayload, EventSequence, EventTimestamp, ObjectId,
    };
    use seismograph::recorder::thread::{ThreadId, ThreadLog};

    use super::*;

    fn retention_fixture() -> (Events, [Observation; 4], Ledger) {
        let mut events = fixture();
        events.total_events = 8;
        events.threads[0].total_events = 8;
        let observations = std::array::from_fn(|index| Observation {
            address: Address::new(100 + index as u64),
            capacity: RETENTION_SIZES[index],
            read: RETENTION_SIZES[index] + 4,
            payload_matches: true,
        });
        events.events = (0..8)
            .map(|index| {
                let output = if index < 4 { index } else { 7 - index };
                Event {
                    thread_id: ThreadId::new(1),
                    sequence: EventSequence::new(index as u64 + 1),
                    timestamp: EventTimestamp::from_ticks(index as u64 + 1),
                    kind: if index < 4 {
                        EventKind::Allocation
                    } else {
                        EventKind::Deallocation
                    },
                    payload: EventPayload::Allocation(Allocation {
                        allocation_id: AllocationId::new(observations[output].address.get()),
                        event_thread_id: EventThreadId::new(1),
                        heap_id: HeapId::new(1),
                        heap_kind: HeapKind::General,
                        freed_after_heap_release: false,
                        address: observations[output].address,
                        size: RETENTION_SIZES[output],
                        alignment: 1,
                    }),
                    call_stack: Vec::new(),
                }
            })
            .collect();
        let payload = Count {
            attempts: 4,
            attempted_bytes: 1380352,
            completions: 4,
            completed_bytes: 1380352,
        };
        (
            events,
            observations,
            Ledger {
                decode_payload: payload,
                payload_zero_fill: payload,
                ..Ledger::default()
            },
        )
    }

    fn allocation_mut(event: &mut Event) -> &mut Allocation {
        let EventPayload::Allocation(allocation) = &mut event.payload else {
            panic!("synthetic allocation");
        };
        allocation
    }

    #[test]
    fn retention_validator_rejects_malformed_traces_and_observations() {
        let (events, observations, ledger) = retention_fixture();
        assert_eq!(
            check_retention(&events, &observations, ledger).expect("valid fixture"),
            [
                (1, 4096),
                (2, 69632),
                (3, 331776),
                (4, 1380352),
                (4, 331776),
                (3, 69632),
                (2, 4096),
                (1, 0)
            ]
        );
        let mutations: &[fn(&mut Events)] = &[
            |e| e.lost_events = 1,
            |e| e.total_events = 9,
            |e| e.threads[0].lost_events = 1,
            |e| e.threads[0].total_events = 7,
            |e| {
                e.events.pop();
            },
            |e| {
                e.events.push(e.events[0].clone());
            },
            |e| e.events[1].sequence = EventSequence::new(1),
            |e| e.events[1].sequence = EventSequence::new(3),
            |e| {
                for event in &mut e.events {
                    event.sequence = EventSequence::new(event.sequence.get() + 1);
                }
            },
            |e| e.events.swap(0, 1),
            |e| e.events[0].thread_id = ThreadId::new(2),
            |e| e.threads.push(ThreadLog::default()),
            |e| e.events[0].kind = EventKind::ArcCreate,
            |e| e.events[0].payload = EventPayload::Object(ObjectId::new(1)),
            |e| e.events[0].call_stack.push(Address::new(1)),
            |e| allocation_mut(&mut e.events[0]).address = Address::new(0),
            |e| allocation_mut(&mut e.events[1]).address = Address::new(100),
            |e| allocation_mut(&mut e.events[4]).address = Address::new(999),
            |e| allocation_mut(&mut e.events[5]).address = Address::new(103),
            |e| e.events[0].kind = EventKind::Deallocation,
            |e| allocation_mut(&mut e.events[0]).size += 1,
            |e| allocation_mut(&mut e.events[4]).size += 1,
            |e| allocation_mut(&mut e.events[0]).alignment = 8,
            |e| allocation_mut(&mut e.events[4]).alignment = 8,
            |e| {
                allocation_mut(&mut e.events[0]).size += 1;
                allocation_mut(&mut e.events[7]).size += 1;
            },
            |e| {
                allocation_mut(&mut e.events[0]).alignment = 8;
                allocation_mut(&mut e.events[7]).alignment = 8;
            },
            |e| e.events[7].kind = EventKind::Allocation,
            |e| {
                e.events.swap(4, 5);
                for (i, event) in e.events.iter_mut().enumerate() {
                    event.sequence = EventSequence::new(i as u64 + 1);
                }
            },
        ];
        for (index, mutate) in mutations.iter().enumerate() {
            let mut malformed = events.clone();
            mutate(&mut malformed);
            assert!(
                check_retention(&malformed, &observations, ledger).is_err(),
                "{index}"
            );
        }
        let mut overflow = events.clone();
        allocation_mut(&mut overflow.events[0]).size = u64::MAX;
        assert_eq!(
            check_retention(&overflow, &observations, ledger),
            Err("scoped byte sum overflow")
        );
        for index in 0..4 {
            let mut wrong = observations;
            wrong[index].capacity += 1;
            assert!(check_retention(&events, &wrong, ledger).is_err());
            wrong = observations;
            wrong[index].payload_matches = false;
            assert!(check_retention(&events, &wrong, ledger).is_err());
            wrong = observations;
            wrong[index].read -= 1;
            assert!(check_retention(&events, &wrong, ledger).is_err());
        }
        let mut lost = ledger;
        lost.lost_updates = 1;
        assert!(check_retention(&events, &observations, lost).is_err());
        let counter_mutations: &[fn(&mut Ledger)] = &[
            |l| l.decode_payload.attempts = 3,
            |l| l.decode_payload.attempted_bytes -= 1,
            |l| l.decode_payload.completions = 3,
            |l| l.decode_payload.completed_bytes -= 1,
            |l| l.payload_zero_fill.attempts = 3,
            |l| l.payload_zero_fill.attempted_bytes -= 1,
            |l| l.payload_zero_fill.completions = 3,
            |l| l.payload_zero_fill.completed_bytes -= 1,
            |l| l.encode_payload = done(1),
            |l| l.encode_padding = done(1),
            |l| l.decode_padding = done(1),
            |l| l.padding_zero_fill = done(1),
        ];
        for mutate in counter_mutations {
            let mut wrong = ledger;
            mutate(&mut wrong);
            assert!(check_retention(&events, &observations, wrong).is_err());
        }
    }

    #[test]
    fn reused_address_gets_new_generation_but_cannot_fake_retention_schedule() {
        let (mut events, observations, ledger) = retention_fixture();
        // Four sequential lifetimes at the same address: valid abstract
        // lifetimes, but not the approved four-simultaneously-live workload.
        for (index, event) in events.events.iter_mut().enumerate() {
            event.kind = if index % 2 == 0 {
                EventKind::Allocation
            } else {
                EventKind::Deallocation
            };
            let allocation = allocation_mut(event);
            allocation.address = Address::new(100);
            allocation.allocation_id = AllocationId::new(100);
            allocation.size = 4096;
        }
        assert_eq!(
            retention_lifetimes(&events).expect("valid reuse"),
            [
                (1, 4096),
                (1, 0),
                (2, 4096),
                (2, 0),
                (3, 4096),
                (3, 0),
                (4, 4096),
                (4, 0)
            ]
        );
        assert!(check_retention(&events, &observations, ledger).is_err());
    }

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
