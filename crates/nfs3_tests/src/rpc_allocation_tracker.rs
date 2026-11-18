//! Bounded, pure reconstruction of the B0.4 event-visible requested-byte cohort.
//! Realloc calls, hidden overlap, usable/process/collector bytes and overhead
//! cannot be inferred from allocation/deallocation events.
use seismograph::recorder::event::{EventKind, Events};

pub const CAPACITY: usize = 4096;
pub const MARKER_SIZE: usize = 257;
pub const CHECKPOINTS: [&str; 4] = [
    "setup",
    "output_retained",
    "output_dropped",
    "runtime_teardown",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OwnedOutput {
    pub address: u64,
    pub length: u64,
    pub capacity: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Checkpoint {
    pub sequence: u64,
    pub generation: u64,
    pub live: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Report {
    pub allocations: u64,
    pub frees: u64,
    pub allocated_bytes: u64,
    pub freed_bytes: u64,
    pub largest_request: u64,
    pub event_order_peak: u64,
    pub live_at_stop: u64,
    pub checkpoints: [Checkpoint; 4],
    pub output_generation: Option<u64>,
    pub output_free_sequence: Option<u64>,
}

#[derive(Clone, Copy)]
struct Origin {
    address: u64,
    size: u64,
    alignment: u64,
    generation: u64,
    allocated: u64,
    freed: Option<u64>,
}

pub fn track(
    events: &Events,
    markers: [u64; 4],
    output: Option<OwnedOutput>,
) -> Result<Report, &'static str> {
    if events.events.len() > CAPACITY
        || events.lost_events != 0
        || events.total_events != events.events.len() as u64
    {
        return Err("event bound, loss or vector count mismatch");
    }
    let [thread] = events.threads.as_slice() else {
        return Err("requires exactly one recording thread");
    };
    if thread.lost_events != 0 || thread.total_events != events.total_events {
        return Err("thread loss or count mismatch");
    }
    if markers.contains(&0)
        || markers
            .iter()
            .enumerate()
            .any(|(i, p)| markers[..i].contains(p))
    {
        return Err("zero or duplicate marker address");
    }
    let mut origins = [None::<Origin>; CAPACITY];
    let mut used = 0usize;
    let mut live = 0u64;
    let mut report = Report {
        allocations: 0,
        frees: 0,
        allocated_bytes: 0,
        freed_bytes: 0,
        largest_request: 0,
        event_order_peak: 0,
        live_at_stop: 0,
        checkpoints: [Checkpoint::default(); 4],
        output_generation: None,
        output_free_sequence: None,
    };
    for (index, event) in events.events.iter().enumerate() {
        let sequence = index as u64 + 1;
        if event.sequence.get() != sequence || event.thread_id != thread.thread_id {
            return Err("sequence must start at one, contiguous on one thread");
        }
        let allocation = event.allocation().ok_or("missing allocation payload")?;
        let address = allocation.address.get();
        let size = allocation.size;
        let alignment = allocation.alignment;
        let layout = usize::try_from(size)
            .ok()
            .zip(usize::try_from(alignment).ok())
            .and_then(|(size, align)| std::alloc::Layout::from_size_align(size, align).ok());
        if address == 0
            || size == 0
            || layout.is_none()
            || address % alignment != 0
            || !event.call_stack.is_empty()
        {
            return Err("invalid layout/address or unexpected stack capture");
        }
        match event.kind {
            EventKind::Allocation => {
                if origins[..used]
                    .iter()
                    .flatten()
                    .any(|o| o.address == address && o.freed.is_none())
                {
                    return Err("duplicate live address");
                }
                report.allocations = report
                    .allocations
                    .checked_add(1)
                    .ok_or("generation overflow")?;
                report.allocated_bytes = report
                    .allocated_bytes
                    .checked_add(size)
                    .ok_or("allocation byte overflow")?;
                live = live.checked_add(size).ok_or("live byte overflow")?;
                report.largest_request = report.largest_request.max(size);
                report.event_order_peak = report.event_order_peak.max(live);
                let slot = origins.get_mut(used).ok_or("origin bound exceeded")?;
                *slot = Some(Origin {
                    address,
                    size,
                    alignment,
                    generation: report.allocations,
                    allocated: sequence,
                    freed: None,
                });
                used = used.checked_add(1).ok_or("origin index overflow")?;
                for (i, marker) in markers.iter().enumerate() {
                    if address == *marker && size == MARKER_SIZE as u64 && alignment == 1 {
                        if report.checkpoints[i].generation != 0 {
                            return Err("reused marker generation");
                        }
                        report.checkpoints[i] = Checkpoint {
                            sequence,
                            generation: report.allocations,
                            live,
                        };
                    }
                }
            }
            EventKind::Deallocation => {
                let origin = origins[..used]
                    .iter_mut()
                    .flatten()
                    .find(|o| o.address == address && o.freed.is_none())
                    .ok_or("unmatched-origin or double free")?;
                if origin.size != size || origin.alignment != alignment {
                    return Err("free layout differs from origin");
                }
                origin.freed = Some(sequence);
                report.frees = report.frees.checked_add(1).ok_or("free count overflow")?;
                report.freed_bytes = report
                    .freed_bytes
                    .checked_add(size)
                    .ok_or("free byte overflow")?;
                live = live.checked_sub(size).ok_or("live byte underflow")?;
            }
            _ => return Err("unexpected event kind"),
        }
    }
    // The generation is allocated afresh for every allocation, including address
    // reuse and realloc-shaped pairs. No inference of actual realloc calls.
    for (index, checkpoint) in report.checkpoints.iter().enumerate() {
        if checkpoint.generation == 0
            || (index > 0 && checkpoint.generation <= report.checkpoints[index - 1].generation)
        {
            return Err("missing or reordered marker");
        }
        let origin = origins[..used]
            .iter()
            .flatten()
            .find(|o| o.generation == checkpoint.generation)
            .ok_or("missing marker origin")?;
        if origin.freed.is_some() {
            return Err("marker freed before Stop");
        }
    }
    if let Some(output) = output {
        if output.address == 0
            || output.capacity == 0
            || output.length == 0
            || output.length > output.capacity
            || markers.contains(&output.address)
        {
            return Err("borrowed/empty/invalid owned output");
        }
        let retained = report.checkpoints[1].sequence;
        let dropped = report.checkpoints[2].sequence;
        let origin = origins[..used]
            .iter()
            .flatten()
            .find(|o| {
                o.address == output.address
                    && o.allocated < retained
                    && o.freed.is_none_or(|free| free > retained)
            })
            .ok_or("output has no live generation at retention")?;
        if origin.size != output.capacity || origin.alignment != 1 {
            return Err("output capacity/layout mismatch");
        }
        let free = origin.freed.ok_or("output free missing")?;
        if free <= retained || free >= dropped {
            return Err("output free outside retained/drop boundaries");
        }
        report.output_generation = Some(origin.generation);
        report.output_free_sequence = Some(free);
    }
    report.live_at_stop = live;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use seismograph::recorder::alloc::{Allocation, AllocationId, EventThreadId, HeapId, HeapKind};
    use seismograph::recorder::event::{
        Address, Event, EventPayload, EventSequence, EventTimestamp,
    };
    use seismograph::recorder::thread::{ThreadId, ThreadLog};

    use super::*;

    fn trace(spec: &[(bool, u64, u64)]) -> Events {
        Events {
            total_events: spec.len() as u64,
            threads: vec![ThreadLog {
                thread_id: ThreadId::new(1),
                total_events: spec.len() as u64,
                ..ThreadLog::default()
            }],
            events: spec
                .iter()
                .enumerate()
                .map(|(i, &(alloc, ptr, size))| Event {
                    thread_id: ThreadId::new(1),
                    sequence: EventSequence::new(i as u64 + 1),
                    timestamp: EventTimestamp::from_ticks(i as u64 + 1),
                    kind: if alloc {
                        EventKind::Allocation
                    } else {
                        EventKind::Deallocation
                    },
                    payload: EventPayload::Allocation(Allocation {
                        allocation_id: AllocationId::new(ptr),
                        event_thread_id: EventThreadId::new(1),
                        heap_id: HeapId::new(1),
                        heap_kind: HeapKind::General,
                        freed_after_heap_release: false,
                        address: Address::new(ptr),
                        size,
                        alignment: 1,
                    }),
                    call_stack: Vec::new(),
                })
                .collect(),
            ..Events::default()
        }
    }
    const MARKERS: [u64; 4] = [100, 200, 300, 400];
    const OUTPUT: OwnedOutput = OwnedOutput {
        address: 16,
        length: 16,
        capacity: 16,
    };
    fn spec() -> Vec<(bool, u64, u64)> {
        vec![
            (true, 16, 16),
            (false, 16, 16), // same-size input handle, not output
            (true, 100, 257),
            (true, 16, 16),
            (true, 200, 257),
            (false, 16, 16),
            (true, 300, 257),
            (true, 400, 257),
        ]
    }
    #[test]
    fn lookup_same_size_handles_and_realloc_shaped_pair_have_fresh_generations() {
        let r = track(&trace(&spec()), MARKERS, Some(OUTPUT)).unwrap();
        assert_eq!(r.output_generation, Some(3));
        assert_eq!(r.output_free_sequence, Some(6));
        assert_eq!((r.allocations, r.frees), (6, 2));
        assert_eq!((r.allocated_bytes, r.freed_bytes), (1060, 32));
        assert_eq!(r.event_order_peak, 1028);
        assert_eq!(r.checkpoints.map(|c| c.live), [257, 530, 771, 1028]);
        // This could be free+malloc, in-place or moving realloc. The report
        // deliberately has no actual-realloc-count or overlap/usable-byte field.
        assert_eq!(r.live_at_stop, 1028);
    }
    #[test]
    fn rejects_marker_wrong_order_freed_reused_pointer_and_identity() {
        for variant in 0..5 {
            let mut s = spec();
            match variant {
                0 => s.swap(2, 4),
                1 => s.push((false, 100, 257)),
                2 => {
                    s.push((false, 100, 257));
                    s.push((true, 100, 257));
                }
                3 => s[2].2 = 256,
                4 => s[6].1 = 100,
                _ => unreachable!(),
            }
            assert!(
                track(&trace(&s), MARKERS, Some(OUTPUT)).is_err(),
                "{variant}"
            );
        }
        assert!(track(&trace(&spec()), [100, 100, 300, 400], Some(OUTPUT)).is_err());
    }
    #[test]
    fn rejects_output_premature_missing_late_double_free_and_stale_handle() {
        for variant in 0..6 {
            let mut s = spec();
            match variant {
                0 => s.swap(4, 5), // premature
                1 => {
                    s.remove(5);
                }
                2 => s.swap(5, 6), // late
                3 => s.insert(6, (false, 16, 16)),
                4 => s[3].1 = 17, // old handle generation cannot stand in for output
                5 => s[5].2 = 17,
                _ => unreachable!(),
            }
            assert!(
                track(&trace(&s), MARKERS, Some(OUTPUT)).is_err(),
                "{variant}"
            );
        }
        for bad in [
            OwnedOutput {
                capacity: 0,
                ..OUTPUT
            },
            OwnedOutput {
                capacity: 17,
                ..OUTPUT
            },
            OwnedOutput {
                length: 17,
                ..OUTPUT
            },
        ] {
            assert!(track(&trace(&spec()), MARKERS, Some(bad)).is_err());
        }
    }
    #[test]
    fn rejects_sequence_start_two_thread_loss_counts_layout_and_bounds() {
        for variant in 0..12 {
            let mut e = trace(&spec());
            match variant {
                0 => {
                    for event in &mut e.events {
                        event.sequence = EventSequence::new(event.sequence.get() + 1);
                    }
                }
                1 => e.events[1].sequence = EventSequence::new(1),
                2 => e.events[0].thread_id = ThreadId::new(2),
                3 => e.threads.push(ThreadLog::default()),
                4 => e.lost_events = 1,
                5 => e.threads[0].lost_events = 1,
                6 => e.total_events += 1,
                7 => e.threads[0].total_events += 1,
                8 => e.events[0].kind = EventKind::ArcCreate,
                9 => {
                    let EventPayload::Allocation(a) = &mut e.events[0].payload else {
                        unreachable!()
                    };
                    a.alignment = 3;
                }
                10 => e.events[0].kind = EventKind::Deallocation,
                11 => e.events.reverse(),
                _ => unreachable!(),
            }
            assert!(track(&e, MARKERS, Some(OUTPUT)).is_err(), "{variant}");
        }
        let mut s = spec();
        while s.len() < CAPACITY {
            s.push((true, 1000 + s.len() as u64, 1));
        }
        assert!(track(&trace(&s), MARKERS, Some(OUTPUT)).is_ok());
        s.push((true, 99999, 1));
        assert!(track(&trace(&s), MARKERS, Some(OUTPUT)).is_err());
    }
    #[test]
    fn checked_event_byte_overflow_and_duplicate_live_address_fail() {
        let huge = isize::MAX as u64;
        let mut s = spec();
        s.extend([
            (true, 1000, huge),
            (false, 1000, huge),
            (true, 1000, huge),
            (false, 1000, huge),
            (true, 1000, huge),
        ]);
        assert_eq!(
            track(&trace(&s), MARKERS, Some(OUTPUT)),
            Err("allocation byte overflow")
        );
        let mut s = spec();
        s.insert(4, (true, 16, 16));
        assert_eq!(
            track(&trace(&s), MARKERS, Some(OUTPUT)),
            Err("duplicate live address")
        );
    }
    #[test]
    fn no_owned_output_claim_and_layout_and_observation_identity() {
        assert_eq!(
            track(&trace(&spec()), MARKERS, None)
                .unwrap()
                .output_generation,
            None
        );
        for variant in 0..6 {
            let mut e = trace(&spec());
            let EventPayload::Allocation(a) = &mut e.events[5].payload else {
                unreachable!()
            };
            match variant {
                0 => a.alignment = 2,
                1 => a.address = Address::new(0),
                2 => a.size = 0,
                3 => a.size = u64::MAX,
                4 => a.alignment = 0,
                5 => a.address = Address::new(999),
                _ => unreachable!(),
            }
            assert!(track(&e, MARKERS, Some(OUTPUT)).is_err(), "{variant}");
        }
        let mut s = spec();
        s[3] = (true, 17, 16);
        s[5] = (false, 17, 16);
        // A live same-sized allocation is not evidence for the observed address.
        assert!(track(&trace(&s), MARKERS, Some(OUTPUT)).is_err());
    }
}
