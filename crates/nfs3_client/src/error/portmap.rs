use std::error::Error as StdError;
use std::fmt;

use super::Error;

/// Error from portmapper operations.
///
/// Returned by [`PortmapperClient::getport`](crate::PortmapperClient::getport).
#[derive(Debug)]
pub enum PortmapError {
    Call(Error),
    ProgramUnavailable,
    InvalidPortValue(u32),
}

impl fmt::Display for PortmapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Call(e) => e.fmt(f),
            Self::ProgramUnavailable => write!(f, "Program unavailable"),
            Self::InvalidPortValue(value) => write!(f, "Invalid port value: {value}"),
        }
    }
}

impl StdError for PortmapError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Call(e) => Some(e),
            Self::ProgramUnavailable | Self::InvalidPortValue(_) => None,
        }
    }
}

impl From<Error> for PortmapError {
    fn from(e: Error) -> Self {
        Self::Call(e)
    }
}
