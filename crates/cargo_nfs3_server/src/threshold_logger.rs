use std::ops::Range;
use std::sync::atomic::{AtomicUsize, Ordering};

use tracing::info;

const BILLION: usize = 1_000_000_000;

const PREDEFINED_RANGES: &[Range<usize>] = &[
    0..10_000,
    10_000..20_000,
    20_000..50_000,
    50_000..100_000,
    100_000..200_000,
    200_000..500_000,
    500_000..1_000_000,
    1_000_000..2_000_000,
    2_000_000..5_000_000,
    5_000_000..10_000_000,
    10_000_000..20_000_000,
    20_000_000..50_000_000,
    50_000_000..100_000_000,
    100_000_000..200_000_000,
    200_000_000..500_000_000,
    500_000_000..750_000_000,
    750_000_000..1_000_000_000,
];

/// A threshold logger that tracks the size of a collection and logs when specific thresholds are
/// reached.
///
/// This type is designed to log at exponential intervals (10k, 20k, 50k, 100k, 200k, etc.)
/// to avoid spam while still providing useful monitoring information.
#[derive(Debug)]
pub struct ThresholdLogger {
    name: &'static str,
    next_threshold: AtomicUsize,
}

impl ThresholdLogger {
    /// Create a new threshold logger with the given name
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            next_threshold: AtomicUsize::new(
                PREDEFINED_RANGES
                    .first()
                    .expect("PREDEFINED_RANGES is not empty")
                    .end,
            ),
        }
    }

    /// Check if the current size should trigger a log message and log if necessary
    pub fn check_and_log(&self, current_size: usize) {
        let threshold = self.next_threshold.load(Ordering::Relaxed);

        if current_size < threshold {
            return;
        }

        let new_range = Self::calculate_threshold(current_size);

        if self
            .next_threshold
            .compare_exchange(
                threshold,
                new_range.end,
                Ordering::Relaxed,
                Ordering::Relaxed,
            )
            .is_ok()
        {
            info!(
                counter = self.name,
                count = current_size,
                formatted_count = format_number(current_size),
                "new threshold reached",
            );
        }
    }

    /// Calculate which threshold (if any) the current size has reached and the next upcoming
    /// threshold. Returns a `Range<usize>` where `start = 0` when no threshold has been reached
    /// yet.
    fn calculate_threshold(size: usize) -> Range<usize> {
        for entry in PREDEFINED_RANGES {
            if entry.contains(&size) {
                return entry.clone();
            }
        }

        billion_bounds(size)
    }
}

/// Calculate bounds for sizes in the billions range.
const fn billion_bounds(size: usize) -> Range<usize> {
    if size < BILLION {
        return 0..BILLION;
    }

    let current = (size / BILLION) * BILLION;
    let next = current.saturating_add(BILLION);

    current..next
}

/// Format a number with appropriate units (K, M, etc.)
/// Always shows values >= 1.0 with at most 3 leading digits.
/// Output is at most 7 characters (e.g., "9999999", "999.99K", "100.0M").
/// Prioritizes precision: plain numbers up to 7 chars, then uses units.
/// Omits decimal places when value divides evenly without precision loss.
#[allow(clippy::cast_precision_loss)]
fn format_number(n: usize) -> String {
    match n {
        0..=999_999 => n.to_string(),
        1_000_000..=9_999_999 => {
            if n.is_multiple_of(1_000) {
                format!("{}K", n / 1_000)
            } else {
                format!("{:.2}K", n as f64 / 1_000.0)
            }
        }
        10_000_000..=99_999_999 => {
            if n.is_multiple_of(1_000_000) {
                format!("{}M", n / 1_000_000)
            } else {
                format!("{:.2}M", n as f64 / 1_000_000.0)
            }
        }
        100_000_000..=999_999_999 => {
            if n.is_multiple_of(1_000_000) {
                format!("{}M", n / 1_000_000)
            } else {
                format!("{:.1}M", n as f64 / 1_000_000.0)
            }
        }
        1_000_000_000..=9_999_999_999 => {
            if n.is_multiple_of(1_000_000_000) {
                format!("{}B", n / 1_000_000_000)
            } else {
                format!("{:.2}B", n as f64 / 1_000_000_000.0)
            }
        }
        10_000_000_000..=99_999_999_999 => {
            if n.is_multiple_of(1_000_000_000) {
                format!("{}B", n / 1_000_000_000)
            } else {
                format!("{:.1}B", n as f64 / 1_000_000_000.0)
            }
        }
        _ => {
            let value = n / 1_000_000_000;
            format!("{value}B")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_threshold_calculation() {
        assert_eq!(ThresholdLogger::calculate_threshold(5_000).start, 0);
        assert_eq!(ThresholdLogger::calculate_threshold(10_000).start, 10_000);
        assert_eq!(ThresholdLogger::calculate_threshold(15_000).start, 10_000);
        assert_eq!(ThresholdLogger::calculate_threshold(20_000).start, 20_000);
        assert_eq!(ThresholdLogger::calculate_threshold(25_000).start, 20_000);
        assert_eq!(ThresholdLogger::calculate_threshold(50_000).start, 50_000);
        assert_eq!(ThresholdLogger::calculate_threshold(75_000).start, 50_000);
        assert_eq!(ThresholdLogger::calculate_threshold(100_000).start, 100_000);
        assert_eq!(ThresholdLogger::calculate_threshold(150_000).start, 100_000);
        assert_eq!(ThresholdLogger::calculate_threshold(200_000).start, 200_000);
        assert_eq!(
            ThresholdLogger::calculate_threshold(1_000_000).start,
            1_000_000
        );
    }

    #[test]
    fn test_next_thresholds() {
        let small = ThresholdLogger::calculate_threshold(15_000);
        assert_eq!(small.start, 10_000);
        assert_eq!(small.end, 20_000);

        let mid = ThresholdLogger::calculate_threshold(6_000_000);
        assert_eq!(mid.start, 5_000_000);
        assert_eq!(mid.end, 10_000_000);

        let large = ThresholdLogger::calculate_threshold(75_000_000);
        assert_eq!(large.start, 50_000_000);
        assert_eq!(large.end, 100_000_000);

        let billion = ThresholdLogger::calculate_threshold(1_500_000_000);
        assert_eq!(billion.start, 1_000_000_000);
        assert_eq!(billion.end, 2_000_000_000);
    }

    // Plain number range: 0-999,999
    #[test]
    fn test_formatting() {
        assert_eq!(format_number(0), "0");
        assert_eq!(format_number(1), "1");
        assert_eq!(format_number(9), "9");
        assert_eq!(format_number(100), "100");
        assert_eq!(format_number(1_000), "1000");
        assert_eq!(format_number(9_999), "9999");
        assert_eq!(format_number(10_000), "10000");
        assert_eq!(format_number(10_001), "10001");
        assert_eq!(format_number(100_000), "100000");
        assert_eq!(format_number(999_999), "999999");

        assert_eq!(format_number(1_000_000), "1000K");
        assert_eq!(format_number(1_001_000), "1001K");
        assert_eq!(format_number(1_001_001), "1001.00K");
        assert_eq!(format_number(1_500_000), "1500K");
        assert_eq!(format_number(1_234_567), "1234.57K");
        assert_eq!(format_number(2_000_000), "2000K");
        assert_eq!(format_number(5_000_000), "5000K");
        assert_eq!(format_number(9_500_000), "9500K");
        assert_eq!(format_number(9_999_000), "9999K");

        assert_eq!(format_number(10_000_000), "10M");
        assert_eq!(format_number(10_100_000), "10.10M");
        assert_eq!(format_number(10_500_000), "10.50M");
        assert_eq!(format_number(50_000_000), "50M");
        assert_eq!(format_number(50_500_000), "50.50M");
        assert_eq!(format_number(99_000_000), "99M");
        assert_eq!(format_number(99_500_000), "99.50M");
        assert_eq!(format_number(99_900_000), "99.90M");
        assert_eq!(format_number(100_000_000), "100M");
        assert_eq!(format_number(100_100_000), "100.1M");
        assert_eq!(format_number(500_000_000), "500M");
        assert_eq!(format_number(500_500_000), "500.5M");
        assert_eq!(format_number(999_000_000), "999M");
        assert_eq!(format_number(999_400_000), "999.4M");
        assert_eq!(format_number(999_900_000), "999.9M");

        assert_eq!(format_number(1_000_000_000), "1B");
        assert_eq!(format_number(1_050_000_000), "1.05B");
        assert_eq!(format_number(1_200_000_000), "1.20B");
        assert_eq!(format_number(1_500_000_000), "1.50B");
        assert_eq!(format_number(5_000_000_000), "5B");
        assert_eq!(format_number(5_100_000_000), "5.10B");
        assert_eq!(format_number(9_000_000_000), "9B");
        assert_eq!(format_number(9_500_000_000), "9.50B");
        assert_eq!(format_number(9_990_000_000), "9.99B");
        assert_eq!(format_number(10_000_000_000), "10B");
        assert_eq!(format_number(10_100_000_000), "10.1B");
        assert_eq!(format_number(50_000_000_000), "50B");
        assert_eq!(format_number(50_500_000_000), "50.5B");
        assert_eq!(format_number(99_000_000_000), "99B");
        assert_eq!(format_number(99_400_000_000), "99.4B");
        assert_eq!(format_number(99_900_000_000), "99.9B");
        assert_eq!(format_number(100_000_000_000), "100B");
        assert_eq!(format_number(1_000_000_000_000), "1000B");
        assert_eq!(format_number(5_000_000_000_000), "5000B");
    }
}
