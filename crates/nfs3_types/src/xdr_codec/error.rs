use std::fmt;

/// XDR encoding or decoding error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The input ended before the value was complete.
    UnexpectedEof,

    /// An invalid value was encountered for an enum/bool type.
    InvalidEnumValue(u32),

    /// A length or count prefix exceeds the remaining input.
    InvalidLength(usize),

    /// XDR padding bytes were not zero.
    NonZeroPadding,

    /// The input contains bytes after the decoded value.
    TrailingBytes(usize),

    /// An error occurred while packing an object.
    ObjectTooLarge(usize),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof => write!(f, "Unexpected end of XDR input"),
            Self::InvalidEnumValue(value) => write!(f, "Invalid enum value: {value}"),
            Self::InvalidLength(len) => write!(f, "Invalid length: {len}"),
            Self::NonZeroPadding => write!(f, "Non-zero XDR padding"),
            Self::TrailingBytes(len) => write!(f, "{len} trailing bytes after XDR value"),
            Self::ObjectTooLarge(size) => write!(f, "Object too large: {size} bytes"),
        }
    }
}

impl std::error::Error for Error {}
