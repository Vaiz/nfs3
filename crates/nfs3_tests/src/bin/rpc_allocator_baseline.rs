//! One B0.4 allocation cohort per process. Requires an external watchdog.
use std::hint::black_box;

use anyhow::{Result, ensure};
use nfs3_tests::rpc_allocation_tracker::{CAPACITY, CHECKPOINTS, MARKER_SIZE, OwnedOutput, track};
use nfs3_tests::rpc_baseline::{Case, Observer, OwnedAllocation, run_observed};
use seismograph::recorder::{Configuration, EventBufferCapacity, Policy};
use seismograph::snapshot::{EventBufferDisposition, SnapshotOptions};

rallocator::rallocator!();

#[derive(Default)]
struct Markers {
    owners: [Option<Box<[u8; MARKER_SIZE]>>; 4],
    addresses: [u64; 4],
    output: Option<OwnedOutput>,
}

impl Markers {
    fn mark(&mut self, index: usize) {
        let owner = Box::new([0xA5; MARKER_SIZE]);
        let address = black_box(owner.as_ptr()) as usize as u64;
        self.addresses[index] = address;
        self.owners[index] = Some(owner);
    }
}

impl Observer for Markers {
    const CAPTURE: bool = true;
    const PRINT: bool = false;
    fn setup(&mut self) {
        self.mark(0);
    }
    fn retained(&mut self, output: Option<OwnedAllocation>) {
        self.output = output.map(|o| OwnedOutput {
            address: o.address as u64,
            length: o.length as u64,
            capacity: o.capacity as u64,
        });
        self.mark(1);
    }
    fn dropped(&mut self) {
        self.mark(2);
    }
    fn teardown(&mut self) {
        self.mark(3);
    }
}

fn main() -> Result<()> {
    // Keep every pre-recording CLI owner alive through Stop, including argv[0].
    let args: Vec<_> = std::env::args().collect();
    ensure!(args.len() == 2, "supply exactly one RPC case");
    let case = Case::parse(&args[1])?;
    let mut markers = Markers::default();
    seismograph::recorder(Configuration {
        allocations: Policy::all(false),
        event_capacity_per_thread: EventBufferCapacity::new(CAPACITY),
        ..Configuration::default()
    });
    let outcome = run_observed(&args[1], &mut markers);
    let snapshot = seismograph::snapshot(SnapshotOptions {
        event_buffers: EventBufferDisposition::Stop,
    })?;
    black_box(&args);
    black_box(&markers.owners);
    // No repeated snapshots, restart, or nested suppression assumptions.
    outcome?;
    let decoded = seismograph::snapshot::decode(snapshot.as_bytes())?;
    let events = &decoded.events;
    for thread in &events.threads {
        println!(
            "recorder_thread_id={} total_events={} lost_events={}",
            thread.thread_id.get(),
            thread.total_events,
            thread.lost_events,
        );
    }
    let report = track(events, markers.addresses, markers.output).map_err(|error| {
        anyhow::anyhow!(
            "capture=failed; exact_claims=invalid; {error}; events={} vector={} loss={} threads={}",
            events.total_events,
            events.events.len(),
            events.lost_events,
            events.threads.len()
        )
    })?;
    println!(
        "B0.4 case={}; hooks=true; allocator=rallocator; runtime=current_thread; requests=1; \
         connections=1; harness=passed; golden=passed; ledger=passed; \
         lifecycle=normally_quiescent; stopped=true; stacks=false",
        args[1]
    );
    let (request, reply) = case.lengths();
    println!("wire_request={request}; wire_reply={reply}; transcript_overflow=false");
    println!(
        "events={}; vector={}; lost={}; threads={}; sequence_start=1; sequence_end={}; \
         configured_event_slots={CAPACITY}; event_headroom={}; allocations={}; frees={}; \
         allocated_requested_event_bytes={}; freed_requested_event_bytes={}; largest_request={}; \
         event_order_requested_peak={}; requested_live_at_stop={}",
        events.total_events,
        events.events.len(),
        events.lost_events,
        events.threads.len(),
        events.total_events,
        CAPACITY - events.events.len(),
        report.allocations,
        report.frees,
        report.allocated_bytes,
        report.freed_bytes,
        report.largest_request,
        report.event_order_peak,
        report.live_at_stop
    );
    for (index, checkpoint) in report.checkpoints.iter().enumerate() {
        println!(
            "checkpoint={}; address={}; sequence={}; generation={}; cohort_requested_live={}; \
             includes_own_marker=true; cumulative_marker_requested_bytes={}",
            CHECKPOINTS[index],
            markers.addresses[index],
            checkpoint.sequence,
            checkpoint.generation,
            checkpoint.live,
            (index + 1) * MARKER_SIZE,
        );
    }
    if let Some(output) = markers.output {
        println!(
            "owned_output_address={}; output_logical={}; output_capacity={}; \
             output_generation={}; output_free_sequence={}; \
             output_free_strictly_between_retained_and_dropped=true",
            output.address,
            output.length,
            output.capacity,
            report.output_generation.expect("validated generation"),
            report.output_free_sequence.expect("validated free"),
        );
    } else {
        println!("owned_output_claim=none");
    }
    println!(
        "cohort_initial_live=empty; \
         cohort_includes=runtime,fixtures,input,duplex,transcripts,capture,golden_validation,\
         harness; marker_requested_bytes={}; marker_allocation_events=4; \
         marker_free_events_before_stop=0; actual_realloc_calls=unknown; \
         same_size_realloc=unknown; hidden_realloc_overlap=unknown; \
         usable_allocator_bytes=unknown; process_initial_allocations=unknown; \
         process_peak=unknown; collector_bytes=unknown; instrumentation_overhead=unknown; \
         timing=not_measured",
        4 * MARKER_SIZE,
    );
    // Complete raw transport evidence permits independent generation/live-sum
    // reconstruction without trusting this executable's aggregate tracker.
    for event in &events.events {
        let allocation = event.allocation().expect("validated allocation event");
        println!(
            "event_thread={} event_sequence={} kind={:?} address={} requested={} alignment={}",
            event.thread_id.get(),
            event.sequence.get(),
            event.kind,
            allocation.address.get(),
            allocation.size,
            allocation.alignment,
        );
    }
    Ok(())
}
