use std::error::Error as StdError;
use std::fmt;

use super::RpcError;

/// Error from portmapper operations.
///
/// Returned by [`PortmapperClient::getport`](crate::PortmapperClient::getport).
#[derive(Debug)]
pub enum PortmapError {
    /// The RPC call failed.
    Rpc(RpcError),
    /// The requested program is not registered with the portmapper.
    ProgramUnavailable,
    /// The portmapper returned a port number that does not fit in a `u16`.
    InvalidPortValue(u32),
}

impl fmt::Display for PortmapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rpc(e) => e.fmt(f),
            Self::ProgramUnavailable => write!(f, "Program unavailable"),
            Self::InvalidPortValue(value) => write!(f, "Invalid port value: {value}"),
        }
    }
}

impl StdError for PortmapError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Rpc(e) => Some(e),
            Self::ProgramUnavailable | Self::InvalidPortValue(_) => None,
        }
    }
}

impl From<RpcError> for PortmapError {
    fn from(e: RpcError) -> Self {
        Self::Rpc(e)
    }
}

impl PortmapError {
    /// Returns `true` if the connection that produced this error is still in
    /// a clean state and may be reused.
    #[must_use]
    pub const fn is_connection_reusable(&self) -> bool {
        match self {
            Self::Rpc(e) => e.is_connection_reusable(),
            Self::ProgramUnavailable | Self::InvalidPortValue(_) => true,
        }
    }
}
