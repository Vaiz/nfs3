#![allow(
    non_camel_case_types,
    clippy::large_enum_variant,
    clippy::upper_case_acronyms,
    reason = "Keep the RFC names and store XDR union arms inline"
)]

//! This module contains the definitions of the RPC protocol as defined in RFC 1057.

mod record_buf;
mod record_marker;

use nfs3_macros::XdrCodec;

pub use self::record_buf::RecordBuf;
pub use self::record_marker::fragment_header;
use crate::xdr_codec::{BoundedVec, Decoder, Encoder, Error, Opaque, Pack, Result, Unpack};

pub const RPC_VERSION_2: u32 = 2;

#[derive(Copy, Clone, Debug, Eq, PartialEq, XdrCodec)]
#[repr(u32)]
pub enum msg_type {
    CALL = 0,
    REPLY = 1,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, XdrCodec)]
#[repr(u32)]
pub enum reply_stat {
    MSG_ACCEPTED = 0,
    MSG_DENIED = 1,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, XdrCodec)]
#[repr(u32)]
pub enum accept_stat {
    SUCCESS = 0,
    PROG_UNAVAIL = 1,
    PROG_MISMATCH = 2,
    PROC_UNAVAIL = 3,
    GARBAGE_ARGS = 4,
    SYSTEM_ERR = 5,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, XdrCodec)]
#[repr(u32)]
pub enum reject_stat {
    RPC_MISMATCH = 0,
    AUTH_ERROR = 1,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, XdrCodec)]
#[repr(u32)]
pub enum auth_stat {
    AUTH_OK = 0,
    AUTH_BADCRED = 1,
    AUTH_REJECTEDCRED = 2,
    AUTH_BADVERF = 3,
    AUTH_REJECTEDVERF = 4,
    AUTH_TOOWEAK = 5,
    AUTH_INVALIDRESP = 6,
    AUTH_FAILED = 7,
}

impl std::fmt::Display for auth_stat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::AUTH_OK => "AUTH_OK",
            Self::AUTH_BADCRED => "AUTH_BADCRED",
            Self::AUTH_REJECTEDCRED => "AUTH_REJECTEDCRED",
            Self::AUTH_BADVERF => "AUTH_BADVERF",
            Self::AUTH_REJECTEDVERF => "AUTH_REJECTEDVERF",
            Self::AUTH_TOOWEAK => "AUTH_TOOWEAK",
            Self::AUTH_INVALIDRESP => "AUTH_INVALIDRESP",
            Self::AUTH_FAILED => "AUTH_FAILED",
        };
        write!(f, "{name}")
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, XdrCodec)]
#[repr(u32)]
pub enum auth_flavor {
    AUTH_NULL = 0,
    AUTH_UNIX = 1,
    AUTH_SHORT = 2,
    AUTH_DES = 3,
    // and more to be defined
}

#[derive(Clone, Debug, XdrCodec)]
pub struct opaque_auth<'a> {
    pub flavor: auth_flavor,
    pub body: Opaque<'a>,
}

impl Default for opaque_auth<'static> {
    fn default() -> Self {
        Self {
            flavor: auth_flavor::AUTH_NULL,
            body: Opaque::borrowed(&[]),
        }
    }
}

impl opaque_auth<'static> {
    /// Creates a new `opaque_auth` with the given flavor and body constructed from `auth_unix`.
    ///
    /// # Panics
    ///
    /// Panics if the `auth_unix` cannot be packed.
    #[must_use]
    pub fn auth_unix(auth: &auth_unix) -> Self {
        let mut out = Encoder::new(auth.packed_size());
        auth.pack(&mut out).expect("failed to pack auth_unix");
        let body = out.into_bytes();
        Self {
            flavor: auth_flavor::AUTH_UNIX,
            body: Opaque::from_bytes(body),
        }
    }

    #[must_use]
    pub fn borrow(&self) -> opaque_auth<'_> {
        opaque_auth {
            flavor: self.flavor,
            body: Opaque::borrowed(self.body.as_ref()),
        }
    }
}

const MAX_MACHINENAME_LEN: usize = 255;
const MAX_GIDS: usize = 16;

/// RFC 5531 `AUTH_SYS` parameters. Decoding enforces the machine-name and
/// supplemental-group limits.
#[derive(Clone, Debug)]
pub struct auth_unix {
    pub stamp: u32,
    pub machinename: Opaque<'static>,
    pub uid: u32,
    pub gid: u32,
    /// Supplemental group IDs, limited to 16 entries.
    pub gids: BoundedVec<u32, MAX_GIDS>,
}

impl auth_unix {
    /// Creates `AUTH_SYS` parameters from the machine name and Unix IDs.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidLength`] if `machinename` is longer than 255
    /// bytes or `gids` contains more than 16 supplemental group IDs.
    pub fn new(stamp: u32, machinename: &[u8], uid: u32, gid: u32, gids: Vec<u32>) -> Result<Self> {
        if machinename.len() > MAX_MACHINENAME_LEN {
            return Err(Error::InvalidLength(machinename.len()));
        }
        let gids = gids.try_into()?;
        Ok(Self {
            stamp,
            machinename: Opaque::owned(machinename.to_vec()),
            uid,
            gid,
            gids,
        })
    }
}

impl Pack for auth_unix {
    fn packed_size(&self) -> usize {
        self.stamp.packed_size()
            + self.machinename.packed_size()
            + self.uid.packed_size()
            + self.gid.packed_size()
            + self.gids.packed_size()
    }

    fn pack(&self, out: &mut Encoder) -> Result<()> {
        self.stamp.pack(out)?;
        self.machinename.pack(out)?;
        self.uid.pack(out)?;
        self.gid.pack(out)?;
        self.gids.pack(out)
    }
}

impl Unpack for auth_unix {
    fn unpack(input: &mut Decoder) -> Result<Self> {
        let stamp = u32::unpack(input)?;
        let name_len = u32::unpack(input)? as usize;
        if name_len > MAX_MACHINENAME_LEN {
            return Err(Error::InvalidLength(name_len));
        }
        let machinename = Opaque::from_bytes(input.read_opaque(name_len)?);
        let uid = u32::unpack(input)?;
        let gid = u32::unpack(input)?;
        let gids = BoundedVec::unpack(input)?;
        Ok(Self {
            stamp,
            machinename,
            uid,
            gid,
            gids,
        })
    }
}

impl Default for auth_unix {
    fn default() -> Self {
        Self {
            stamp: 0,
            machinename: Opaque::borrowed(b""),
            uid: 0,
            gid: 0,
            gids: BoundedVec::default(),
        }
    }
}

#[derive(Debug, XdrCodec)]
pub struct call_body<'a> {
    pub rpcvers: u32,
    pub prog: u32,
    pub vers: u32,
    pub proc: u32,
    pub cred: opaque_auth<'a>,
    pub verf: opaque_auth<'a>,
}

#[derive(Debug, XdrCodec)]
pub struct accepted_reply<'a> {
    pub verf: opaque_auth<'a>,
    pub reply_data: accept_stat_data,
}

#[derive(Debug)]
pub enum accept_stat_data {
    SUCCESS, // FIXME: Opaque
    PROG_UNAVAIL,
    PROG_MISMATCH { low: u32, high: u32 },
    PROC_UNAVAIL,
    GARBAGE_ARGS,
    SYSTEM_ERR,
}

impl Pack for accept_stat_data {
    fn packed_size(&self) -> usize {
        4 + match self {
            Self::SUCCESS
            | Self::PROG_UNAVAIL
            | Self::PROC_UNAVAIL
            | Self::GARBAGE_ARGS
            | Self::SYSTEM_ERR => 0,
            Self::PROG_MISMATCH { .. } => 8,
        }
    }

    fn pack(&self, out: &mut Encoder) -> crate::xdr_codec::Result<()> {
        match self {
            Self::SUCCESS => accept_stat::SUCCESS.pack(out),
            Self::PROG_UNAVAIL => accept_stat::PROG_UNAVAIL.pack(out),
            Self::PROG_MISMATCH { low, high } => {
                accept_stat::PROG_MISMATCH.pack(out)?;
                low.pack(out)?;
                high.pack(out)
            }
            Self::PROC_UNAVAIL => accept_stat::PROC_UNAVAIL.pack(out),
            Self::GARBAGE_ARGS => accept_stat::GARBAGE_ARGS.pack(out),
            Self::SYSTEM_ERR => accept_stat::SYSTEM_ERR.pack(out),
        }
    }
}

impl Unpack for accept_stat_data {
    fn unpack(input: &mut Decoder) -> crate::xdr_codec::Result<Self> {
        Ok(match accept_stat::unpack(input)? {
            accept_stat::SUCCESS => Self::SUCCESS,
            accept_stat::PROG_MISMATCH => Self::PROG_MISMATCH {
                low: u32::unpack(input)?,
                high: u32::unpack(input)?,
            },
            accept_stat::PROG_UNAVAIL => Self::PROG_UNAVAIL,
            accept_stat::PROC_UNAVAIL => Self::PROC_UNAVAIL,
            accept_stat::GARBAGE_ARGS => Self::GARBAGE_ARGS,
            accept_stat::SYSTEM_ERR => Self::SYSTEM_ERR,
        })
    }
}

#[derive(Debug)]
pub enum rejected_reply {
    RPC_MISMATCH { low: u32, high: u32 },
    AUTH_ERROR(auth_stat),
}

impl rejected_reply {
    #[must_use]
    pub const fn rpc_mismatch(low: u32, high: u32) -> Self {
        Self::RPC_MISMATCH { low, high }
    }
    #[must_use]
    pub const fn auth_error(auth_stat: auth_stat) -> Self {
        Self::AUTH_ERROR(auth_stat)
    }
}

impl Pack for rejected_reply {
    fn packed_size(&self) -> usize {
        4 + match self {
            Self::RPC_MISMATCH { .. } => 8,
            Self::AUTH_ERROR(_) => 4,
        }
    }

    fn pack(&self, out: &mut Encoder) -> crate::xdr_codec::Result<()> {
        match self {
            Self::RPC_MISMATCH { low, high } => {
                reject_stat::RPC_MISMATCH.pack(out)?;
                low.pack(out)?;
                high.pack(out)
            }
            Self::AUTH_ERROR(auth_stat) => {
                reject_stat::AUTH_ERROR.pack(out)?;
                auth_stat.pack(out)
            }
        }
    }
}

impl Unpack for rejected_reply {
    fn unpack(input: &mut Decoder) -> crate::xdr_codec::Result<Self> {
        Ok(match reject_stat::unpack(input)? {
            reject_stat::RPC_MISMATCH => Self::RPC_MISMATCH {
                low: u32::unpack(input)?,
                high: u32::unpack(input)?,
            },
            reject_stat::AUTH_ERROR => Self::AUTH_ERROR(auth_stat::unpack(input)?),
        })
    }
}

#[derive(Debug, XdrCodec)]
pub enum reply_body<'a> {
    #[xdr(0)]
    MSG_ACCEPTED(accepted_reply<'a>),
    #[xdr(1)]
    MSG_DENIED(rejected_reply),
}

#[derive(Debug, XdrCodec)]
pub struct rpc_msg<'a, 'b> {
    pub xid: u32,
    pub body: msg_body<'a, 'b>,
}

#[derive(Debug, XdrCodec)]
pub enum msg_body<'a, 'b> {
    #[xdr(0)]
    CALL(call_body<'a>),
    #[xdr(1)]
    REPLY(reply_body<'b>),
}
