use std::process::Command;

// Deliberate finite fixture policy. The executable supplies no numeric profile.
const POLICY: &[&str] = &[
    "--record-block-bytes",
    "65536",
    "--record-max-bytes",
    "2097152",
    "--record-max-fragments",
    "4096",
    "--decode-metadata-bytes",
    "2097152",
    "--decode-collection-elements",
    "65536",
    "--decode-depth",
    "64",
    "--decode-reservation-bytes",
    "16777216",
    "--encode-wire-bytes",
    "2097152",
    "--encode-metadata-bytes",
    "2097152",
    "--encode-chunks",
    "256",
    "--encode-depth",
    "64",
    "--encode-reservation-bytes",
    "16777216",
    "--read-max-bytes",
    "1048576",
    "--write-max-bytes",
    "1048576",
    "--directory-wire-bytes",
    "1048576",
    "--directory-entries",
    "10000",
    "--directory-name-bytes",
    "4096",
    "--directory-metadata-bytes",
    "2097152",
    "--max-connections",
    "16",
    "--max-operations",
    "4",
    "--backing-bytes",
    "536870912",
    "--idle-timeout-ms",
    "30000",
    "--progress-timeout-ms",
    "5000",
    "--receive-timeout-ms",
    "30000",
    "--send-timeout-ms",
    "30000",
];

#[test]
fn every_independent_policy_flag_is_required_by_actual_cli() {
    for pair in POLICY.chunks_exact(2) {
        let args: Vec<_> = POLICY
            .chunks_exact(2)
            .filter(|candidate| candidate[0] != pair[0])
            .flatten()
            .copied()
            .collect();
        let output = Command::new(env!("CARGO_BIN_EXE_cargo-nfs3-server"))
            .arg("--memfs")
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{}", pair[0]);
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains(pair[0]), "{stderr}");
        assert!(stderr.contains("required"), "{stderr}");
    }
}

#[test]
fn infeasible_policy_is_rejected_before_binding_or_backend_work() {
    for (flag, value) in [
        ("--record-block-bytes", "0"),
        ("--decode-depth", "0"),
        ("--encode-wire-bytes", "1048576"),
        ("--directory-wire-bytes", "2097152"),
        ("--read-max-bytes", "2097152"),
        ("--write-max-bytes", "2097152"),
        ("--max-connections", "0"),
        ("--max-operations", "0"),
        ("--backing-bytes", "1"),
        ("--idle-timeout-ms", "0"),
        ("--progress-timeout-ms", "0"),
        ("--receive-timeout-ms", "0"),
        ("--send-timeout-ms", "0"),
    ] {
        let mut args = POLICY.to_vec();
        let index = args.iter().position(|arg| *arg == flag).unwrap();
        args[index + 1] = value;
        let output = Command::new(env!("CARGO_BIN_EXE_cargo-nfs3-server"))
            .args(["--memfs", "--bind-ip", "invalid-address"])
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{flag}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(
            stderr.contains("invalid bounded server envelope"),
            "{flag}: {stderr}"
        );
        assert!(!stderr.contains("failed to bind"), "{stderr}");
    }
}
