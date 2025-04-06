//! Error types

use std::error::Error as StdError;
use std::fmt;

use nfs3_types::rpc::{accept_stat_data, rejected_reply};

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Xdr(nfs3_types::xdr_codec::Error),
    Rpc(RpcError),
    Portmap(PortmapError),
    MountError(nfs3_types::mount::mountstat3),
    NfsError(nfs3_types::nfs3::nfsstat3),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => e.fmt(f),
            Self::Xdr(e) => e.fmt(f),
            Self::Rpc(e) => e.fmt(f),
            Self::Portmap(e) => e.fmt(f),
            Self::MountError(e) => (*e as u32).fmt(f),
            Self::NfsError(e) => (*e as u32).fmt(f),
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
    NotFullyParsed { buf: Vec<u8>, pos: u64 },
    ProgUnavail,
    ProgMismatch,
    ProcUnavail,
    GarbageArgs,
    SystemErr,
}

impl fmt::Display for RpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedCall => write!(f, "Unexpected CALL request"),
            Self::Auth => write!(f, "Authentication error"),
            Self::RpcMismatch => write!(f, "RPC version mismatch"),
            Self::WrongLength => write!(f, "Wrong length in RPC message"),
            Self::UnexpectedXid => write!(f, "Unexpected XID in RPC reply"),
            Self::NotFullyParsed { .. } => write!(f, "Not fully parsed"),
            Self::ProgUnavail => write!(f, "Program unavailable"),
            Self::ProgMismatch => write!(f, "Program mismatch"),
            Self::ProcUnavail => write!(f, "Procedure unavailable"),
            Self::GarbageArgs => write!(f, "Garbage arguments"),
            Self::SystemErr => write!(f, "System error"),
        }
    }
}

impl StdError for RpcError {}

impl From<rejected_reply> for RpcError {
    fn from(e: rejected_reply) -> Self {
        match e {
            rejected_reply::RPC_MISMATCH { .. } => Self::RpcMismatch,
            rejected_reply::AUTH_ERROR(_) => Self::Auth,
        }
    }
}

impl TryFrom<accept_stat_data> for RpcError {
    type Error = ();

    fn try_from(value: accept_stat_data) -> Result<Self, Self::Error> {
        match value {
            accept_stat_data::SUCCESS => Err(()),
            accept_stat_data::PROG_UNAVAIL => Ok(Self::ProgUnavail),
            accept_stat_data::PROG_MISMATCH { .. } => Ok(Self::ProgMismatch),
            accept_stat_data::PROC_UNAVAIL => Ok(Self::ProcUnavail),
            accept_stat_data::GARBAGE_ARGS => Ok(Self::GarbageArgs),
            accept_stat_data::SYSTEM_ERR => Ok(Self::SystemErr),
        }
    }
}

#[derive(Debug)]
pub enum PortmapError {
    ProgramUnavailable,
    InvalidPortValue(u32),
}

impl fmt::Display for PortmapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProgramUnavailable => write!(f, "Program unavailable"),
            Self::InvalidPortValue(value) => write!(f, "Invalid port value: {value}"),
        }
    }
}

impl StdError for PortmapError {}

impl From<PortmapError> for Error {
    fn from(e: PortmapError) -> Self {
        Self::Portmap(e)
    }
}
