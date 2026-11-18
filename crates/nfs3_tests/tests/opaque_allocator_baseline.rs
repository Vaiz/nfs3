#![cfg(feature = "allocator-diagnostics")]

use std::process::Command;

fn run(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_opaque_allocator_baseline"))
        .args(args)
        .output()
        .expect("run diagnostic in its own process");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    String::from_utf8(output.stdout).expect("diagnostic UTF-8")
}

#[test]
fn retention_is_scoped_complete_and_isolated_from_default_lane() {
    let output = run(&["--retention"]);
    assert!(output.starts_with("B0.2 scoped opaque retention;"));
    assert!(!output.contains("B0.1"));
    assert!(
        output.contains("events=8 retained=8 lost=0 threads=1; sequence_start=1; sequence_end=8")
    );
    let checkpoints: Vec<_> = output
        .lines()
        .filter(|line| line.starts_with("checkpoint="))
        .collect();
    assert_eq!(checkpoints.len(), 8);
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
    let sizes = [4096, 65536, 262144, 1048576, 1048576, 262144, 65536, 4096];
    let live = [4096, 69632, 331776, 1380352, 331776, 69632, 4096, 0];
    let generations = [1, 2, 3, 4, 4, 3, 2, 1];
    for (index, line) in checkpoints.iter().enumerate() {
        let fields: Vec<_> = line.split_whitespace().collect();
        for expected in [
            format!("checkpoint={}", names[index]),
            format!("sequence={}", index + 1),
            format!(
                "kind={}",
                if index < 4 {
                    "Allocation"
                } else {
                    "Deallocation"
                }
            ),
            format!("generation={}", generations[index]),
            format!("requested={}", sizes[index]),
            format!("capacity={}", sizes[index]),
            "alignment=1".to_owned(),
            format!("scoped_requested_live={}", live[index]),
            format!("retained_output={}", live[index]),
        ] {
            assert!(
                fields.contains(&expected.as_str()),
                "{line}: missing {expected}"
            );
        }
    }
    assert!(output.contains(
        "decode_payload: Count { attempts: 4, attempted_bytes: 1380352, completions: 4, \
         completed_bytes: 1380352 }"
    ));
    assert!(output.contains(
        "payload_zero_fill: Count { attempts: 4, attempted_bytes: 1380352, completions: 4, \
         completed_bytes: 1380352 }"
    ));
    assert!(output.contains(
        "fixture=passed; scoped_initial_live_set=empty; scoped_requested_live=0; \
         scoped_requested_peak=1380352; scoped_retained_output=0; final_drops=4"
    ));
    for unknown in [
        "process_initial_live_set",
        "process_live",
        "process_peak",
        "process_retention",
        "usable_allocator_bytes",
        "realloc_overlap",
        "physical_copy_bytes",
        "cross_thread_attribution",
        "instrumentation_overhead",
    ] {
        assert!(output.contains(&format!("{unknown}=unknown")));
    }
    assert!(output.contains("timing=not_measured"));

    // A fresh default process must retain its B0.1 contract, including padding.
    let default = run(&[]);
    assert!(default.starts_with("B0.1 diagnostic;"));
    assert!(!default.contains("B0.2"));
    assert!(!default.contains("scoped_requested"));
    assert!(default.contains(
        "fixture=passed; initial_live_set=absent; lifetime_ids=absent; \
         cross_thread_order=unvalidated"
    ));
    assert!(default.contains(
        "decode_padding: Count { attempts: 1, attempted_bytes: 3, completions: 1, \
         completed_bytes: 3 }"
    ));
    assert!(default.contains(
        "process_peak=unknown; retention=unknown; realloc_overlap=unknown; timing=not_measured"
    ));
}

#[test]
fn invalid_lane_is_rejected_before_capture() {
    for args in [&["--unknown"][..], &["--retention", "--retention"][..]] {
        let output = Command::new(env!("CARGO_BIN_EXE_opaque_allocator_baseline"))
            .args(args)
            .output()
            .expect("run diagnostic");
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("usage:"));
    }
}
