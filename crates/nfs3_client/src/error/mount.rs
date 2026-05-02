use std::error::Error as StdError;
use std::fmt;

use super::{Error, RpcError};

/// Error from mount operations.
///
/// Returned by [`MountClient::mnt`](crate::MountClient::mnt).
#[derive(Debug)]
pub enum MountError {
    Io(std::io::Error),
    Xdr(nfs3_types::xdr_codec::Error),
    Rpc(RpcError),
    Status(nfs3_types::mount::mountstat3),
}

impl fmt::Display for MountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => e.fmt(f),
            Self::Xdr(e) => e.fmt(f),
            Self::Rpc(e) => e.fmt(f),
            Self::Status(e) => e.fmt(f),
        }
    }
}

impl StdError for MountError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Xdr(e) => Some(e),
            Self::Rpc(e) => Some(e),
            Self::Status(_) => None,
        }
    }
}

impl From<Error> for MountError {
    fn from(e: Error) -> Self {
        match e {
            Error::Io(e) => Self::Io(e),
            Error::Xdr(e) => Self::Xdr(e),
            Error::Rpc(e) => Self::Rpc(e),
        }
    }
}
