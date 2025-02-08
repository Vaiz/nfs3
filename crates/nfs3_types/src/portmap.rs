#![allow(
    non_camel_case_types,
    clippy::large_enum_variant,
    clippy::upper_case_acronyms
)]

//! Transcribed from RFC 1057 Appendix A

use crate::xdr_codec::{List, Opaque, XdrCodec};

pub const IPPROTO_TCP: u32 = 6;
pub const IPPROTO_UDP: u32 = 17;
pub const PROGRAM: u32 = 100000;
pub const VERSION: u32 = 2;

#[derive(Copy, Clone, Debug, XdrCodec)]
pub struct mapping {
    pub prog: u32,
    pub vers: u32,
    pub prot: u32,
    pub port: u32,
}

pub type pmaplist = List<mapping>;

#[derive(Clone, Debug, XdrCodec)]
pub struct call_args<'a> {
    pub prog: u32,
    pub vers: u32,
    pub proc: u32,
    pub args: Opaque<'a>,
}

#[derive(Clone, Debug, XdrCodec)]
pub struct call_result<'a> {
    pub port: u32,
    pub res: Opaque<'a>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, XdrCodec)]
#[repr(u32)]
pub enum PMAP_PROG {
    PMAPPROC_NULL = 0,
    PMAPPROC_SET = 1,
    PMAPPROC_UNSET = 2,
    PMAPPROC_GETPORT = 3,
    PMAPPROC_DUMP = 4,
    PMAPPROC_CALLIT = 5,
}

impl std::convert::TryFrom<u32> for PMAP_PROG {
    type Error = crate::xdr_codec::Error;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::PMAPPROC_NULL),
            1 => Ok(Self::PMAPPROC_SET),
            2 => Ok(Self::PMAPPROC_UNSET),
            3 => Ok(Self::PMAPPROC_GETPORT),
            4 => Ok(Self::PMAPPROC_DUMP),
            5 => Ok(Self::PMAPPROC_CALLIT),
            _ => Err(crate::xdr_codec::ErrorKind::InvalidEnum(value as i32).into()),
        }
    }
}
