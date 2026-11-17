use std::time::{Duration, UNIX_EPOCH};

#[path = "../examples/support/mtime.rs"]
mod mtime;

use mtime::format_mtime;

#[test]
fn mtime_preserves_auto_si_fractional_precision() {
    for (nanos, expected) in [
        (0, "1970-01-01 00:00:00 UTC"),
        (120_000_000, "1970-01-01 00:00:00.120 UTC"),
        (123_450_000, "1970-01-01 00:00:00.123450 UTC"),
        (123_456_780, "1970-01-01 00:00:00.123456780 UTC"),
        (1, "1970-01-01 00:00:00.000000001 UTC"),
        (1_000, "1970-01-01 00:00:00.000001 UTC"),
        (1_000_000, "1970-01-01 00:00:00.001 UTC"),
    ] {
        assert_eq!(
            format_mtime(UNIX_EPOCH + Duration::new(0, nanos)).expect("NFS timestamp"),
            expected,
        );
    }
}

#[test]
fn mtime_preserves_nfs_range_and_nanosecond_normalization() {
    for (seconds, nanos, expected) in [
        (1_709_164_800, 0, "2024-02-29 00:00:00 UTC"),
        (u32::MAX, 0, "2106-02-07 06:28:15 UTC"),
        (u32::MAX, 999_999_999, "2106-02-07 06:28:15.999999999 UTC"),
        (u32::MAX, 1_000_000_000, "2106-02-07 06:28:16 UTC"),
        (u32::MAX, u32::MAX, "2106-02-07 06:28:19.294967295 UTC"),
    ] {
        assert_eq!(
            format_mtime(UNIX_EPOCH + Duration::new(u64::from(seconds), nanos))
                .expect("NFS timestamp"),
            expected,
        );
    }
}
