use std::time::SystemTime;

use jiff::Timestamp;

pub fn format_mtime(mtime: SystemTime) -> Result<String, jiff::Error> {
    let timestamp = Timestamp::try_from(mtime)?;
    // Preserve chrono's AutoSi display using the normalized timestamp, including
    // the existing Duration::new normalization of oversized NFS nanoseconds.
    let format = match timestamp.subsec_nanosecond() {
        0 => "%F %T UTC",
        nanos if nanos % 1_000_000 == 0 => "%F %T%.3f UTC",
        nanos if nanos % 1_000 == 0 => "%F %T%.6f UTC",
        _ => "%F %T%.9f UTC",
    };
    Ok(timestamp.strftime(format).to_string())
}
