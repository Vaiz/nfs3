use std::error::Error as StdError;
use std::fmt;

use super::{MountError, PortmapError, RpcError};

/// Error when establishing an NFS3 connection.
///
/// Returned by [`Nfs3ConnectionBuilder::mount`](crate::Nfs3ConnectionBuilder::mount).
#[derive(Debug)]
pub enum ConnectError {
    /// An I/O error occurred during network communication.
    Io(std::io::Error),
    /// Failed to serialize or deserialize an XDR-encoded message.
    Xdr(nfs3_types::xdr_codec::Error),
    /// The RPC layer reported a protocol-level error.
    Rpc(RpcError),
    /// The requested program is not registered with the portmapper.
    ProgramUnavailable,
    /// The portmapper returned a port number that does not fit in a `u16`.
    InvalidPortValue(u32),
    /// The mount server denied the request with the given status code.
    MountDenied(nfs3_types::mount::mountstat3),
}

impl fmt::Display for ConnectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => e.fmt(f),
            Self::Xdr(e) => e.fmt(f),
            Self::Rpc(e) => e.fmt(f),
            Self::ProgramUnavailable => write!(f, "Program unavailable"),
            Self::InvalidPortValue(value) => write!(f, "Invalid port value: {value}"),
            Self::MountDenied(e) => e.fmt(f),
        }
    }
}

impl StdError for ConnectError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Xdr(e) => Some(e),
            Self::Rpc(e) => Some(e),
            Self::ProgramUnavailable | Self::InvalidPortValue(_) | Self::MountDenied(_) => None,
        }
    }
}

impl From<std::io::Error> for ConnectError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<PortmapError> for ConnectError {
    fn from(e: PortmapError) -> Self {
        match e {
            PortmapError::Io(e) => Self::Io(e),
            PortmapError::Xdr(e) => Self::Xdr(e),
            PortmapError::Rpc(e) => Self::Rpc(e),
            PortmapError::ProgramUnavailable => Self::ProgramUnavailable,
            PortmapError::InvalidPortValue(v) => Self::InvalidPortValue(v),
        }
    }
}

impl From<MountError> for ConnectError {
    fn from(e: MountError) -> Self {
        match e {
            MountError::Io(e) => Self::Io(e),
            MountError::Xdr(e) => Self::Xdr(e),
            MountError::Rpc(e) => Self::Rpc(e),
            MountError::Denied(s) => Self::MountDenied(s),
        }
    }
}
