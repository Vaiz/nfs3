use std::fmt;

/// The caller-selected payload bound that was exceeded.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PayloadLimit {
    /// Logical bytes visible in a view.
    LogicalBytes,
    /// Sum of backing Vec capacities, once per nonempty descriptor.
    BackingCapacity,
    /// Number of nonempty descriptors.
    Segments,
}

/// Value-only errors for the additive payload/cursor API.
///
/// Errors never retain input storage. Fallible Vec reservations are reported, but
/// Arc control allocations remain infallible: this API does not guarantee OOM recovery.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PayloadError {
    /// A caller-selected bound was exceeded.
    LimitExceeded {
        /// Bound being checked.
        kind: PayloadLimit,
        /// Caller-selected maximum.
        limit: usize,
        /// Observed requirement.
        actual: usize,
    },
    /// A range or initialized prefix is outside the input.
    InvalidRange {
        /// Requested start.
        start: usize,
        /// Requested end.
        end: usize,
        /// Available length.
        len: usize,
    },
    /// A size or offset cannot be represented on this host.
    Overflow,
    /// A fallible Vec reservation failed.
    Allocation,
    /// There are not enough record bytes.
    Truncated {
        /// Position at which bytes were required.
        offset: usize,
        /// Required byte count.
        needed: usize,
        /// Available byte count.
        remaining: usize,
    },
    /// An XDR boolean was neither zero nor one.
    InvalidBool {
        /// Boolean's wire offset.
        offset: usize,
        /// Invalid wire value.
        value: u32,
    },
    /// XDR padding contained a nonzero byte.
    NonzeroPadding {
        /// Offending byte's wire offset.
        offset: usize,
        /// Offending byte.
        value: u8,
    },
    /// Caller-provided output cannot hold the opaque value.
    BufferTooSmall {
        /// Opaque byte count.
        needed: usize,
        /// Output slice length.
        available: usize,
    },
    /// Exact record consumption was required.
    Trailing {
        /// First unconsumed offset.
        offset: usize,
        /// Unconsumed byte count.
        remaining: usize,
    },
}

impl fmt::Display for PayloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::LimitExceeded {
                kind,
                limit,
                actual,
            } => {
                write!(f, "{kind:?} limit {limit} exceeded by requirement {actual}")
            }
            Self::InvalidRange { start, end, len } => {
                write!(f, "invalid range {start}..{end} for length {len}")
            }
            Self::Overflow => f.write_str("payload size or offset overflow"),
            Self::Allocation => f.write_str("payload Vec reservation failed"),
            Self::Truncated {
                offset,
                needed,
                remaining,
            } => {
                write!(f, "at {offset}: need {needed} bytes, have {remaining}")
            }
            Self::InvalidBool { offset, value } => write!(f, "invalid boolean {value} at {offset}"),
            Self::NonzeroPadding { offset, value } => {
                write!(f, "nonzero padding {value} at {offset}")
            }
            Self::BufferTooSmall { needed, available } => {
                write!(f, "output needs {needed} bytes, has {available}")
            }
            Self::Trailing { offset, remaining } => {
                write!(f, "{remaining} trailing record bytes at {offset}")
            }
        }
    }
}

impl std::error::Error for PayloadError {}
