use std::error::Error as StdError;
use std::fmt;

use nfs3_types::rpc::{accept_stat_data, rejected_reply};

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Xdr(nfs3_types::xdr_codec::Error),
    Rpc(RpcError),
    Portmap(PortmapError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => e.fmt(f),
            Error::Xdr(e) => e.fmt(f),
            Error::Rpc(e) => e.fmt(f),
            Error::Portmap(e) => e.fmt(f),
        }
    }
}

impl StdError for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<nfs3_types::xdr_codec::Error> for Error {
    fn from(e: nfs3_types::xdr_codec::Error) -> Self {
        Self::Xdr(e)
    }
}

impl From<RpcError> for Error {
    fn from(e: RpcError) -> Self {
        Self::Rpc(e)
    }
}

impl From<rejected_reply> for Error {
    fn from(e: rejected_reply) -> Self {
        Self::Rpc(e.into())
    }
}

#[derive(Debug)]
pub enum RpcError {
    UnexpectedCall,
    Auth,
    RpcMismatch,
    WrongLength,
    UnexpectedXid,
    NotFullyParsed,
    ProgUnavail,
    ProgMismatch,
    ProcUnavail,
    GarbageArgs,
    SystemErr,
}

impl fmt::Display for RpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RpcError::UnexpectedCall => write!(f, "Unexpected CALL request"),
            RpcError::Auth => write!(f, "Authentication error"),
            RpcError::RpcMismatch => write!(f, "RPC version mismatch"),
            RpcError::WrongLength => write!(f, "Wrong length in RPC message"),
            RpcError::UnexpectedXid => write!(f, "Unexpected XID in RPC reply"),
            RpcError::NotFullyParsed => write!(f, "Not fully parsed"),
            RpcError::ProgUnavail => write!(f, "Program unavailable"),
            RpcError::ProgMismatch => write!(f, "Program mismatch"),
            RpcError::ProcUnavail => write!(f, "Procedure unavailable"),
            RpcError::GarbageArgs => write!(f, "Garbage arguments"),
            RpcError::SystemErr => write!(f, "System error"),
        }
    }
}

impl StdError for RpcError {}

impl From<rejected_reply> for RpcError {
    fn from(e: rejected_reply) -> Self {
        match e {
            rejected_reply::RPC_MISMATCH { .. } => RpcError::RpcMismatch,
            rejected_reply::AUTH_ERROR(_) => RpcError::Auth,
        }
    }
}

impl TryFrom<accept_stat_data> for RpcError {
    type Error = ();

    fn try_from(value: accept_stat_data) -> Result<Self, Self::Error> {
        match value {
            accept_stat_data::SUCCESS => Err(()),
            accept_stat_data::PROG_UNAVAIL => Ok(RpcError::ProgUnavail),
            accept_stat_data::PROG_MISMATCH { .. } => Ok(RpcError::ProgMismatch),
            accept_stat_data::PROC_UNAVAIL => Ok(RpcError::ProcUnavail),
            accept_stat_data::GARBAGE_ARGS => Ok(RpcError::GarbageArgs),
            accept_stat_data::SYSTEM_ERR => Ok(RpcError::SystemErr),
        }
    }
}

#[derive(Debug)]
pub enum PortmapError {
    ProgramUnavailable,
}

impl fmt::Display for PortmapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PortmapError::ProgramUnavailable => write!(f, "Program unavailable"),
        }
    }
}

impl StdError for PortmapError {}

impl From<PortmapError> for Error {
    fn from(e: PortmapError) -> Self {
        Self::Portmap(e)
    }
}
