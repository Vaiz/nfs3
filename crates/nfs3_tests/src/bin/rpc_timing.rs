//! Bounded B0.6 timing driver. See `nfs3_tests::rpc_timing` for interval semantics.
use anyhow::Result;
use nfs3_tests::rpc_timing::{Config, run};

fn main() -> Result<()> {
    let config = Config::parse(&std::env::args().skip(1).collect::<Vec<_>>())?;
    let (samples, stats) = run(&config)?;
    println!(
        "B0.6 case={}; hooks={}; allocator=system; runtime=current_thread; connection=cold; \
         harness=tapped; tracing=no_subscriber; warmups={}; samples={}; validation=passed; \
         sample_capacity={}; sample_capacity_unchanged=true",
        config.case,
        cfg!(feature = "rpc-diagnostics"),
        config.warmups,
        samples.values().len(),
        samples.capacity(),
    );
    println!("statistics={stats:?}");
    println!("raw_ns={:?}", samples.values());
    println!(
        "operation_window=client_encode_through_decoded_output; \
         includes=server_first_poll_startup,transcript_copy_first_touch,\
         duplex_growth_backpressure,argument_construction,backend,scheduling,success_mapping; \
         excludes=fixture_runtime_creation,spawn_call,post_response_validation,output_drop,\
         teardown; rates=integer_floor; quantiles=nearest_rank; \
         payload=READ_WRITE_data_bytes_only; \
         operation_window_rates=not_sustained_wall_clock_throughput; \
         measured_loop_wall=excludes_warmup_includes_setup_validation_drop_teardown_gaps; \
         post_send_work=scheduling_dependent; \
         comparison=full_feature_lane_not_isolated_hook_cost; steady_state_TCP_timing=unknown; \
         allocator_recorder_overhead=unknown"
    );
    Ok(())
}
