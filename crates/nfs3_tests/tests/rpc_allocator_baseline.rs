#![cfg(all(feature = "allocator-diagnostics", feature = "rpc-diagnostics"))]

use std::collections::BTreeMap;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn all_allocator_cases_are_isolated_and_parent_watchdog_bounded() {
    for case in nfs3_tests::rpc_baseline::CASES {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rpc_allocator_baseline"))
            .arg(case)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn isolated allocation cohort");
        let mut stdout = child.stdout.take().expect("child stdout");
        let reader = std::thread::spawn(move || {
            let mut output = String::new();
            stdout
                .read_to_string(&mut output)
                .expect("read child stdout");
            output
        });
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = child.try_wait().expect("poll child") {
                assert!(status.success(), "{case}: {status}");
                break;
            }
            if Instant::now() >= deadline {
                child.kill().expect("kill timed out child");
                child.wait().expect("reap timed out child");
                panic!("{case}: external parent watchdog expired; capture invalid");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let output = reader.join().expect("join stdout reader");
        let field = |line: &str, name: &str| -> u64 {
            line.split([';', ' '])
                .filter_map(|field| field.split_once('='))
                .find_map(|(key, value)| (key == name).then_some(value))
                .expect("reported field")
                .parse()
                .expect("numeric field")
        };
        let global = output
            .lines()
            .find(|line| line.starts_with("events="))
            .expect("global recorder metadata");
        let mut threads = BTreeMap::new();
        let mut raw_counts = BTreeMap::<u64, u64>::new();
        for line in output.lines() {
            if line.starts_with("recorder_thread_id=") {
                let id = field(line, "recorder_thread_id");
                let total = field(line, "total_events");
                let lost = field(line, "lost_events");
                assert!(
                    threads.insert(id, (total, lost)).is_none(),
                    "{case}: duplicate thread"
                );
            } else if line.starts_with("event_thread=") {
                *raw_counts.entry(field(line, "event_thread")).or_default() += 1;
            }
        }
        assert_eq!(threads.len() as u64, field(global, "threads"), "{case}");
        assert_eq!(threads.len(), 1, "{case}");
        assert_eq!(raw_counts.len(), threads.len(), "{case}");
        for (id, (total, lost)) in &threads {
            assert_eq!(*lost, 0, "{case}: thread {id} lost events");
            assert_eq!(raw_counts.get(id), Some(total), "{case}: thread {id}");
        }
        assert_eq!(
            threads.values().map(|(total, _)| total).sum::<u64>(),
            field(global, "events"),
            "{case}"
        );
        assert_eq!(
            threads.values().map(|(_, lost)| lost).sum::<u64>(),
            field(global, "lost"),
            "{case}"
        );
        assert_eq!(
            raw_counts.values().sum::<u64>(),
            field(global, "vector"),
            "{case}"
        );
    }
}
