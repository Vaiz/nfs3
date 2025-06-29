use std::fmt;

#[derive(Debug)]
pub enum Error {
    /// An error occurred while reading or writing data.
    Io(std::io::Error),

    /// An invalid value was encountered for an enum/bool type.
    InvalidEnumValue(u32),

    /// An error occurred while unpacking an object.
    InvalidLength(usize),

    /// An error occurred while packing an object.
    ObjectTooLarge(usize),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::InvalidEnumValue(value) => write!(f, "Invalid enum value: {value}"),
            Self::InvalidLength(len) => write!(f, "Invalid length: {len}"),
            Self::ObjectTooLarge(size) => write!(f, "Object too large: {size} bytes"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}
