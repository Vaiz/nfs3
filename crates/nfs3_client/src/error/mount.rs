use std::error::Error as StdError;
use std::fmt;

use super::{Error, RpcError};

/// Error from mount operations.
///
/// Returned by [`MountClient::mnt`](crate::MountClient::mnt).
#[derive(Debug)]
pub enum MountError {
    /// An I/O error occurred during network communication.
    Io(std::io::Error),
    /// Failed to serialize or deserialize an XDR-encoded message.
    Xdr(nfs3_types::xdr_codec::Error),
    /// The RPC layer reported a protocol-level error.
    Rpc(RpcError),
    /// The mount server denied the request with the given status code.
    Denied(nfs3_types::mount::mountstat3),
}

impl fmt::Display for MountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => e.fmt(f),
            Self::Xdr(e) => e.fmt(f),
            Self::Rpc(e) => e.fmt(f),
            Self::Denied(e) => e.fmt(f),
        }
    }
}

impl StdError for MountError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Xdr(e) => Some(e),
            Self::Rpc(e) => Some(e),
            Self::Denied(_) => None,
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
