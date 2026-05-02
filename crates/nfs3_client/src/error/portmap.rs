use std::error::Error as StdError;
use std::fmt;

use super::{Error, RpcError};

/// Error from portmapper operations.
///
/// Returned by [`PortmapperClient::getport`](crate::PortmapperClient::getport).
#[derive(Debug)]
pub enum PortmapError {
    Io(std::io::Error),
    Xdr(nfs3_types::xdr_codec::Error),
    Rpc(RpcError),
    ProgramUnavailable,
    InvalidPortValue(u32),
}

impl fmt::Display for PortmapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => e.fmt(f),
            Self::Xdr(e) => e.fmt(f),
            Self::Rpc(e) => e.fmt(f),
            Self::ProgramUnavailable => write!(f, "Program unavailable"),
            Self::InvalidPortValue(value) => write!(f, "Invalid port value: {value}"),
        }
    }
}

impl StdError for PortmapError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Xdr(e) => Some(e),
            Self::Rpc(e) => Some(e),
            Self::ProgramUnavailable | Self::InvalidPortValue(_) => None,
        }
    }
}

impl From<Error> for PortmapError {
    fn from(e: Error) -> Self {
        match e {
            Error::Io(e) => Self::Io(e),
            Error::Xdr(e) => Self::Xdr(e),
            Error::Rpc(e) => Self::Rpc(e),
        }
    }
}
