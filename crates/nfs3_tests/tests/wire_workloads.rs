use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn bounded(name: &str) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wire_workloads"))
        .arg(name)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn case");
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait().expect("poll child") {
            assert!(status.success(), "{name}: {status}");
            return;
        }
        if Instant::now() >= deadline {
            child.kill().expect("kill child");
            child.wait().expect("reap child");
            panic!("{name}: external watchdog expired");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn raw_fragment_and_outer_chunk_cases() {
    for name in nfs3_tests::wire_workloads::cases() {
        if name.starts_with("null-") || name.starts_with("read-") || name.starts_with("write-") {
            bounded(&name);
        }
    }
}

#[test]
fn directory_pages_and_retained_owned_results() {
    for op in ["readdir", "plus"] {
        for count in [0, 3, 100] {
            bounded(&format!("{op}-{count}"));
        }
    }
}

#[test]
fn scheduling_tcp_and_classified_error_cases() {
    for name in nfs3_tests::wire_workloads::cases() {
        if !(name.starts_with("null-")
            || name.starts_with("read-")
            || name.starts_with("write-")
            || name.starts_with("readdir-")
            || name.starts_with("plus-"))
        {
            bounded(&name);
        }
    }
}
