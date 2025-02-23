#![allow(
    non_camel_case_types,
    clippy::large_enum_variant,
    clippy::upper_case_acronyms
)]

//! This module contains the definitions of the MOUNT3 protocol as defined in RFC 1813.

use std::io::{Read, Write};

use crate::xdr_codec::{Error, List, Opaque, Pack, Unpack, XdrCodec};

pub const PROGRAM: u32 = 100005;
pub const VERSION: u32 = 3;
pub const MNTPATHLEN: usize = 1024;
pub const MNTNAMLEN: usize = 255;
pub const FHSIZE3: usize = 64;

#[derive(Debug, XdrCodec)]
pub struct fhandle3<'a>(pub Opaque<'a>);
#[derive(Debug, XdrCodec)]
pub struct dirpath<'a>(pub Opaque<'a>);
#[derive(Debug, XdrCodec)]
pub struct name<'a>(pub Opaque<'a>);

#[derive(Copy, Clone, Debug, XdrCodec)]
#[repr(u32)]
pub enum mountstat3 {
    MNT3_OK = 0,
    MNT3ERR_PERM = 1,
    MNT3ERR_NOENT = 2,
    MNT3ERR_IO = 5,
    MNT3ERR_ACCES = 13,
    MNT3ERR_NOTDIR = 20,
    MNT3ERR_INVAL = 22,
    MNT3ERR_NAMETOOLONG = 63,
    MNT3ERR_NOTSUPP = 10004,
    MNT3ERR_SERVERFAULT = 10006,
}

// #[derive(Debug, XdrCodec)]
// pub enum rpc_auth_flavor {
// AUTH_NULL = 0,
// AUTH_UNIX = 1,
// AUTH_SHORT = 2,
// AUTH_DES = 3,
// AUTH_KRB = 4,
// AUTH_GSS = 6,
// AUTH_MAXFLAVOR = 8,
// AUTH_GSS_KRB5 = 390003,
// AUTH_GSS_KRB5I = 390004,
// AUTH_GSS_KRB5P = 390005,
// AUTH_GSS_LKEY = 390006,
// AUTH_GSS_LKEYI = 390007,
// AUTH_GSS_LKEYP = 390008,
// AUTH_GSS_SPKM = 390009,
// AUTH_GSS_SPKMI = 390010,
// AUTH_GSS_SPKMP = 390011,
// }

#[derive(Debug, XdrCodec)]
pub struct mountres3_ok<'a> {
    pub fhandle: fhandle3<'a>,
    pub auth_flavors: Vec<u32>,
}

#[derive(Debug)]
pub enum mountres3<'a> {
    Ok(mountres3_ok<'a>),
    Err(mountstat3),
}

impl<Out> Pack<Out> for mountres3<'_>
where
    Out: Write,
{
    fn pack(&self, output: &mut Out) -> Result<usize, Error> {
        let len = match self {
            Self::Ok(ok) => {
                let mut len = mountstat3::MNT3_OK.pack(output)?;
                len += ok.pack(output)?;
                len
            }
            Self::Err(err) => err.pack(output)?,
        };
        Ok(len)
    }
}

impl<In> Unpack<In> for mountres3<'_>
where
    In: Read,
{
    fn unpack(input: &mut In) -> Result<(Self, usize), Error> {
        let (stat, len) = mountstat3::unpack(input)?;
        let (res, res_len) = match stat {
            mountstat3::MNT3_OK => {
                let (ok, ok_len) = mountres3_ok::unpack(input)?;
                (Self::Ok(ok), ok_len)
            }
            _ => (Self::Err(stat), 0),
        };
        Ok((res, len + res_len))
    }
}

#[derive(Debug, XdrCodec)]
pub struct mountbody<'a, 'b> {
    pub ml_hostname: name<'a>,
    pub ml_directory: dirpath<'b>,
}

pub type mountlist<'a, 'b> = List<mountbody<'a, 'b>>;

#[derive(Debug, XdrCodec)]
pub struct export_node<'a, 'b> {
    pub ex_dir: dirpath<'a>,
    pub ex_groups: List<name<'b>>,
}

pub type exports<'a, 'b> = List<export_node<'a, 'b>>;

#[derive(Copy, Clone, Debug, XdrCodec)]
#[repr(u32)]
pub enum MOUNT_PROGRAM {
    MOUNTPROC3_NULL = 0,
    MOUNTPROC3_MNT = 1,
    MOUNTPROC3_DUMP = 2,
    MOUNTPROC3_UMNT = 3,
    MOUNTPROC3_UMNTALL = 4,
    MOUNTPROC3_EXPORT = 5,
}

impl std::convert::TryFrom<u32> for MOUNT_PROGRAM {
    type Error = crate::xdr_codec::Error;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::MOUNTPROC3_NULL),
            1 => Ok(Self::MOUNTPROC3_MNT),
            2 => Ok(Self::MOUNTPROC3_DUMP),
            3 => Ok(Self::MOUNTPROC3_UMNT),
            4 => Ok(Self::MOUNTPROC3_UMNTALL),
            5 => Ok(Self::MOUNTPROC3_EXPORT),
            _ => Err(crate::xdr_codec::ErrorKind::InvalidEnum(value as i32).into()),
        }
    }
}
