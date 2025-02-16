use std::error::Error as StdError;
use std::fmt;

use nfs3_types::rpc::rejected_reply;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Xdr(nfs3_types::xdr_codec::Error),
    Rpc(RpcError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => e.fmt(f),
            Error::Xdr(e) => e.fmt(f),
            Error::Rpc(e) => e.fmt(f),
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
}

impl fmt::Display for RpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RpcError::UnexpectedCall => write!(f, "Unexpected CALL request"),
            RpcError::Auth => write!(f, "Authentication error"),
            RpcError::RpcMismatch => write!(f, "RPC version mismatch"),
            RpcError::WrongLength => write!(f, "Wrong length in RPC message"),
            RpcError::UnexpectedXid => write!(f, "Unexpected XID in RPC reply"),
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
