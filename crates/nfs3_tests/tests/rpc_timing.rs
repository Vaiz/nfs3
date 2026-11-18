use std::time::{Duration, Instant};

use nfs3_tests::rpc_timing::{Config, Samples, TimingObserver, nanoseconds, rate, statistics};

#[test]
fn boundaries_and_durations_fail_closed() {
    let now = Instant::now();
    for sequence in ["", "s", "e", "es", "sse", "see", "ses"] {
        let mut observer = TimingObserver::default();
        for boundary in sequence.chars() {
            if boundary == 's' {
                observer.start_at(now);
            } else {
                observer.end_at(now + Duration::from_nanos(7));
            }
        }
        assert!(observer.finish(Ok(())).is_err(), "{sequence}");
    }
    for (start, end, success) in [
        (now, now + Duration::from_nanos(7), true),
        (now, now, false),
        (now + Duration::from_nanos(7), now, false),
    ] {
        let mut observer = TimingObserver::default();
        observer.start_at(start);
        observer.end_at(end);
        assert_eq!(observer.finish(Ok(())).is_ok(), success);
    }
    let mut observer = TimingObserver::default();
    observer.start_at(now);
    observer.end_at(now + Duration::from_nanos(7));
    assert!(
        observer
            .finish(Err(anyhow::anyhow!("golden failure")))
            .is_err()
    );
    assert!(nanoseconds(Duration::ZERO).is_err());
    assert!(nanoseconds(Duration::from_nanos(u64::MAX)).is_ok());
    assert!(nanoseconds(Duration::from_secs(u64::MAX)).is_err());
}

#[test]
fn finite_cli_bounds() {
    let parse =
        |args: &[&str]| Config::parse(&args.iter().map(|s| (*s).into()).collect::<Vec<_>>());
    let defaults = parse(&["null"]).unwrap();
    assert_eq!((defaults.warmups, defaults.samples), (32, 512));
    for args in [
        vec!["null", "--warmup", "0", "--samples", "1"],
        vec!["read-1024", "--samples", "512", "--warmup", "32"],
    ] {
        assert!(parse(&args).is_ok());
    }
    for args in [
        vec![],
        vec!["unknown"],
        vec!["null", "--samples", "0"],
        vec!["null", "--samples", "513"],
        vec!["null", "--warmup", "33"],
        vec!["null", "--warmup", "-1"],
        vec!["null", "--samples", "184467440737095516160"],
        vec!["null", "--samples", "NaN"],
        vec!["null", "--samples"],
        vec!["null", "--other", "1"],
        vec!["null", "--samples", "1", "--samples", "2"],
        vec!["null", "--warmup", "0", "--warmup", "1"],
        vec!["null", "extra"],
    ] {
        assert!(parse(&args).is_err(), "{args:?}");
    }
}

#[test]
fn exact_nearest_rank_floor_rates_and_overflow() {
    let raw: Vec<u64> = (1..=512).rev().collect();
    let stats = statistics(&raw, Some(4096), 1_000_000_001).unwrap();
    assert_eq!((stats.p50_ns, stats.p95_ns, stats.p99_ns), (256, 487, 507));
    assert_eq!(stats.sum_ns, 131_328);
    assert_eq!(stats.operation_window_ops_per_s, 3_898_635);
    assert_eq!(
        stats.operation_window_payload_bytes_per_s,
        Some(15_968_810_916)
    );
    assert_eq!(stats.measured_loop_wall_ops_per_s, 511);
    assert_eq!(raw[0], 512);
    let singleton = statistics(&[7], None, 100).unwrap();
    assert_eq!(
        (singleton.p50_ns, singleton.p95_ns, singleton.p99_ns),
        (7, 7, 7)
    );
    assert_eq!(singleton.operation_window_payload_bytes_per_s, None);
    assert_eq!(statistics(&[2, 1, 2], None, 10).unwrap().p50_ns, 2);
    assert_eq!(rate(1, 3).unwrap(), 333_333_333);
    assert!(rate(u128::MAX, 1).is_err());
    assert!(rate(1, 0).is_err());
    assert!(statistics(&[], None, 1).is_err());
    assert!(statistics(&[0], None, 1).is_err());
    assert!(statistics(&[1; 513], None, 1).is_err());
    assert!(statistics(&[1], None, 0).is_err());
    assert!(statistics(&[1, 1], Some(u128::MAX), 1).is_err());
    assert!(statistics(&[1], Some(u128::MAX), 1).is_err());
    assert_eq!(
        statistics(&[u64::MAX; 512], None, 1).unwrap().sum_ns,
        u128::from(u64::MAX) * 512
    );
}

fn staged(ns: u64) -> TimingObserver {
    let now = Instant::now();
    let mut observer = TimingObserver::default();
    observer.start_at(now);
    observer.end_at(now + Duration::from_nanos(ns));
    observer
}

#[test]
fn commit_only_validated_samples_without_growth() {
    assert!(Samples::new(0).is_err());
    assert!(Samples::new(513).is_err());
    let mut samples = Samples::new(2).unwrap();
    let capacity = samples.capacity();
    assert!(
        samples
            .commit(staged(7), Err(anyhow::anyhow!("ledger failure")))
            .is_err()
    );
    assert!(samples.commit(TimingObserver::default(), Ok(())).is_err());
    assert!(samples.commit(staged(0), Ok(())).is_err());
    assert!(samples.values().is_empty());
    samples.commit(staged(9), Ok(())).unwrap();
    samples.commit(staged(3), Ok(())).unwrap();
    assert!(samples.commit(staged(1), Ok(())).is_err());
    assert_eq!(samples.values(), [9, 3]);
    assert_eq!(samples.capacity(), capacity);
}

fn subprocess(args: &[&str]) -> std::process::Output {
    use std::process::{Command, Stdio};
    let mut child = Command::new(env!("CARGO_BIN_EXE_rpc_timing"))
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("external timing subprocess watchdog expired: {args:?}");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn invalid_subprocess_has_no_successful_partial_output() {
    for args in [
        vec!["null", "--samples", "0"],
        vec!["null", "--warmup", "33"],
        vec!["bad"],
    ] {
        let output = subprocess(&args);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
}

#[cfg(not(feature = "allocator-diagnostics"))]
#[test]
fn every_case_short_counts_external_watchdog() {
    for case in nfs3_tests::rpc_baseline::CASES {
        for warmups in ["0", "1"] {
            let output = subprocess(&[case, "--warmup", warmups, "--samples", "2"]);
            assert!(output.status.success(), "{case}: {:?}", output);
            assert!(output.stderr.is_empty());
            let stdout = String::from_utf8(output.stdout).unwrap();
            assert!(stdout.contains(&format!("hooks={}", cfg!(feature = "rpc-diagnostics"))));
            assert!(stdout.contains("samples=2; validation=passed"));
            let raw = stdout
                .lines()
                .find(|line| line.starts_with("raw_ns="))
                .unwrap();
            let values: Vec<u64> = raw
                .trim_start_matches("raw_ns=[")
                .trim_end_matches(']')
                .split(", ")
                .map(|s| s.parse().unwrap())
                .collect();
            assert_eq!(values.len(), 2);
            assert!(values.iter().all(|&ns| ns > 0));
            assert_eq!(stdout.matches("B0.6 ").count(), 1);
            assert!(!stdout.contains("B0.3 "));
            assert_eq!(
                stdout.contains("operation_window_payload_bytes_per_s: Some("),
                case.starts_with("read-") || case.starts_with("write-")
            );
        }
    }
}

#[cfg(feature = "allocator-diagnostics")]
#[test]
fn recorder_feature_cannot_be_a_timing_lane() {
    let output = subprocess(&["null", "--warmup", "0", "--samples", "1"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}
