use std::error::Error as StdError;
use std::fmt;

use super::Error;

/// Error from mount operations.
///
/// Returned by [`MountClient::mnt`](crate::MountClient::mnt).
#[derive(Debug)]
pub enum MountError {
    Call(Error),
    Status(nfs3_types::mount::mountstat3),
}

impl fmt::Display for MountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Call(e) => e.fmt(f),
            Self::Status(e) => e.fmt(f),
        }
    }
}

impl StdError for MountError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Call(e) => Some(e),
            Self::Status(_) => None,
        }
    }
}

impl From<Error> for MountError {
    fn from(e: Error) -> Self {
        Self::Call(e)
    }
}
