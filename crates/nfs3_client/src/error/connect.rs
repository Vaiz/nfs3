use std::error::Error as StdError;
use std::fmt;

use super::{MountError, PortmapError};

/// Error when establishing an NFS3 connection.
///
/// Returned by [`Nfs3ConnectionBuilder::mount`](crate::Nfs3ConnectionBuilder::mount).
#[derive(Debug)]
pub enum ConnectError {
    Io(std::io::Error),
    Portmap(PortmapError),
    Mount(MountError),
}

impl fmt::Display for ConnectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => e.fmt(f),
            Self::Portmap(e) => e.fmt(f),
            Self::Mount(e) => e.fmt(f),
        }
    }
}

impl StdError for ConnectError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Portmap(e) => Some(e),
            Self::Mount(e) => Some(e),
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
        Self::Portmap(e)
    }
}

impl From<MountError> for ConnectError {
    fn from(e: MountError) -> Self {
        Self::Mount(e)
    }
}
