use std::error::Error as StdError;
use std::fmt;

use super::RpcError;

/// Error from mount operations.
///
/// Returned by [`MountClient::mnt`](crate::MountClient::mnt).
#[derive(Debug)]
pub enum MountError {
    /// The RPC call failed.
    Rpc(RpcError),
    /// The mount server denied the request with the given status code.
    Denied(nfs3_types::mount::mountstat3),
}

impl fmt::Display for MountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rpc(e) => e.fmt(f),
            Self::Denied(e) => e.fmt(f),
        }
    }
}

impl StdError for MountError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Rpc(e) => Some(e),
            Self::Denied(_) => None,
        }
    }
}

impl From<RpcError> for MountError {
    fn from(e: RpcError) -> Self {
        Self::Rpc(e)
    }
}

impl MountError {
    /// Returns `true` if the connection that produced this error is still in
    /// a clean state and may be reused.
    #[must_use]
    pub const fn is_connection_reusable(&self) -> bool {
        match self {
            Self::Rpc(e) => e.is_connection_reusable(),
            Self::Denied(_) => true,
        }
    }
}
