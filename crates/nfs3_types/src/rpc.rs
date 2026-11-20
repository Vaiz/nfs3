#![allow(
    non_camel_case_types,
    clippy::large_enum_variant,
    clippy::upper_case_acronyms,
    reason = "Keep the RFC names and store XDR union arms inline"
)]

//! This module contains the definitions of the RPC protocol as defined in RFC 1057.

use std::io::{Read, Write};

use nfs3_macros::{XdrCodec, XdrDecode, XdrEncode};

use crate::xdr_codec::{
    Decode, DecodeResult, Decoder, Encode, EncodeResult, Encoder, Opaque, Pack, Unpack,
};

/// RPC header
///
/// The RPC header is a 32-bit integer that contains the length of the fragment and an EOF flag.
#[derive(Default, Copy, Clone, Debug, Eq, PartialEq, XdrCodec, XdrDecode, XdrEncode)]
pub struct fragment_header {
    pub header: u32,
}

impl fragment_header {
    pub const EOF_FLAG: u32 = 0x8000_0000;
    pub const MASK: u32 = 0x7FFF_FFFF;

    /// Creates a new `fragment_header` with the given length and EOF flag.
    ///
    /// # Panics
    ///
    /// Panics if the length is greater than 2 GiB.
    #[must_use]
    pub fn new(length: u32, eof: bool) -> Self {
        assert!(length <= Self::MASK);
        let mut header = length;
        if eof {
            header |= Self::EOF_FLAG;
        }
        Self { header }
    }
    #[must_use]
    pub const fn eof(&self) -> bool {
        self.header & Self::EOF_FLAG != 0
    }
    #[must_use]
    pub const fn fragment_length(&self) -> u32 {
        self.header & Self::MASK
    }
    #[must_use]
    pub const fn into_xdr_buf(self) -> [u8; 4] {
        self.header.to_be_bytes()
    }
}

impl From<[u8; 4]> for fragment_header {
    fn from(bytes: [u8; 4]) -> Self {
        let header = u32::from_be_bytes(bytes);
        Self { header }
    }
}

pub const RPC_VERSION_2: u32 = 2;

#[derive(Copy, Clone, Debug, Eq, PartialEq, XdrCodec)]
#[repr(u32)]
#[derive(XdrDecode, XdrEncode)]
pub enum msg_type {
    CALL = 0,
    REPLY = 1,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, XdrCodec)]
#[repr(u32)]
#[derive(XdrDecode, XdrEncode)]
pub enum reply_stat {
    MSG_ACCEPTED = 0,
    MSG_DENIED = 1,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, XdrCodec)]
#[repr(u32)]
#[derive(XdrDecode, XdrEncode)]
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
#[derive(XdrDecode, XdrEncode)]
pub enum reject_stat {
    RPC_MISMATCH = 0,
    AUTH_ERROR = 1,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, XdrCodec)]
#[repr(u32)]
#[derive(XdrDecode, XdrEncode)]
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
/// Closed legacy flavor set, not every flavor allowed by extensible ONC RPC.
#[derive(XdrDecode, XdrEncode)]
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
        let mut out = Vec::with_capacity(auth.packed_size());
        auth.pack(&mut out).expect("failed to pack auth_unix");
        Self {
            flavor: auth_flavor::AUTH_UNIX,
            body: Opaque::owned(out),
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

#[derive(Clone, Debug, XdrCodec)]
pub struct auth_unix {
    pub stamp: u32,
    pub machinename: Opaque<'static>,
    pub uid: u32,
    pub gid: u32,
    pub gids: Vec<u32>,
}

impl Default for auth_unix {
    fn default() -> Self {
        Self {
            stamp: 0,
            machinename: Opaque::borrowed(b""),
            uid: 0,
            gid: 0,
            gids: vec![],
        }
    }
}

#[derive(Debug, XdrCodec, XdrDecode, XdrEncode)]
pub struct call_body<'a> {
    pub rpcvers: u32,
    pub prog: u32,
    pub vers: u32,
    pub proc: u32,
    pub cred: opaque_auth<'a>,
    pub verf: opaque_auth<'a>,
}

#[derive(Debug, XdrCodec, XdrDecode, XdrEncode)]
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

    fn pack(&self, out: &mut impl Write) -> crate::xdr_codec::Result<usize> {
        let len = match self {
            Self::SUCCESS => accept_stat::SUCCESS.pack(out)?,
            Self::PROG_UNAVAIL => accept_stat::PROG_UNAVAIL.pack(out)?,
            Self::PROG_MISMATCH { low, high } => {
                accept_stat::PROG_MISMATCH.pack(out)? + low.pack(out)? + high.pack(out)?
            }
            Self::PROC_UNAVAIL => accept_stat::PROC_UNAVAIL.pack(out)?,
            Self::GARBAGE_ARGS => accept_stat::GARBAGE_ARGS.pack(out)?,
            Self::SYSTEM_ERR => accept_stat::SYSTEM_ERR.pack(out)?,
        };
        Ok(len)
    }
}

impl Unpack for accept_stat_data {
    fn unpack(input: &mut impl Read) -> crate::xdr_codec::Result<(Self, usize)> {
        let (accept_stat, len) = accept_stat::unpack(input)?;
        let (body, body_len) = match accept_stat {
            accept_stat::SUCCESS => (Self::SUCCESS, 0),
            accept_stat::PROG_MISMATCH => {
                let (low, low_len) = u32::unpack(input)?;
                let (high, high_len) = u32::unpack(input)?;
                (Self::PROG_MISMATCH { low, high }, low_len + high_len)
            }
            accept_stat::PROG_UNAVAIL => (Self::PROG_UNAVAIL, 0),
            accept_stat::PROC_UNAVAIL => (Self::PROC_UNAVAIL, 0),
            accept_stat::GARBAGE_ARGS => (Self::GARBAGE_ARGS, 0),
            accept_stat::SYSTEM_ERR => (Self::SYSTEM_ERR, 0),
        };
        Ok((body, len + body_len))
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

    fn pack(&self, out: &mut impl Write) -> crate::xdr_codec::Result<usize> {
        let len = match self {
            Self::RPC_MISMATCH { low, high } => {
                reject_stat::RPC_MISMATCH.pack(out)? + low.pack(out)? + high.pack(out)?
            }
            Self::AUTH_ERROR(auth_stat) => {
                reject_stat::AUTH_ERROR.pack(out)? + auth_stat.pack(out)?
            }
        };
        Ok(len)
    }
}

impl Unpack for rejected_reply {
    fn unpack(input: &mut impl Read) -> crate::xdr_codec::Result<(Self, usize)> {
        let (reject_stat, len) = reject_stat::unpack(input)?;
        let (body, body_len) = match reject_stat {
            reject_stat::RPC_MISMATCH => {
                let (low, low_len) = u32::unpack(input)?;
                let (high, high_len) = u32::unpack(input)?;
                (Self::RPC_MISMATCH { low, high }, low_len + high_len)
            }
            reject_stat::AUTH_ERROR => {
                let (auth_stat, auth_stat_len) = auth_stat::unpack(input)?;
                (Self::AUTH_ERROR(auth_stat), auth_stat_len)
            }
        };
        Ok((body, len + body_len))
    }
}

#[derive(Debug, XdrCodec, XdrDecode, XdrEncode)]
pub enum reply_body<'a> {
    #[xdr(0)]
    MSG_ACCEPTED(accepted_reply<'a>),
    #[xdr(1)]
    MSG_DENIED(rejected_reply),
}

#[derive(Debug, XdrCodec, XdrDecode, XdrEncode)]
pub struct rpc_msg<'a, 'b> {
    pub xid: u32,
    pub body: msg_body<'a, 'b>,
}

#[derive(Debug, XdrCodec, XdrDecode, XdrEncode)]
pub enum msg_body<'a, 'b> {
    #[xdr(0)]
    CALL(call_body<'a>),
    #[xdr(1)]
    REPLY(reply_body<'b>),
}

impl Decode for opaque_auth<'_> {
    fn decode(d: &mut Decoder) -> DecodeResult<Self> {
        Ok(Self {
            flavor: d.decode()?,
            body: d.opaque(400)?,
        })
    }
}

impl Encode for opaque_auth<'_> {
    fn encode(&self, e: &mut Encoder) -> EncodeResult<()> {
        e.encode(&self.flavor)?;
        e.opaque(self.body.as_ref(), 400)
    }
}

impl Encode for auth_unix {
    fn encode(&self, e: &mut Encoder) -> EncodeResult<()> {
        e.encode(&self.stamp)?;
        e.opaque(self.machinename.as_ref(), 255)?;
        e.encode(&self.uid)?;
        e.encode(&self.gid)?;
        e.collection(&self.gids, 16)
    }
}

impl Encode for accept_stat_data {
    fn encode(&self, e: &mut Encoder) -> EncodeResult<()> {
        match self {
            Self::SUCCESS => e.encode(&accept_stat::SUCCESS),
            Self::PROG_UNAVAIL => e.encode(&accept_stat::PROG_UNAVAIL),
            Self::PROG_MISMATCH { low, high } => {
                e.encode(&accept_stat::PROG_MISMATCH)?;
                e.encode(low)?;
                e.encode(high)
            }
            Self::PROC_UNAVAIL => e.encode(&accept_stat::PROC_UNAVAIL),
            Self::GARBAGE_ARGS => e.encode(&accept_stat::GARBAGE_ARGS),
            Self::SYSTEM_ERR => e.encode(&accept_stat::SYSTEM_ERR),
        }
    }
}

impl Encode for rejected_reply {
    fn encode(&self, e: &mut Encoder) -> EncodeResult<()> {
        match self {
            Self::RPC_MISMATCH { low, high } => {
                e.encode(&reject_stat::RPC_MISMATCH)?;
                e.encode(low)?;
                e.encode(high)
            }
            Self::AUTH_ERROR(status) => {
                e.encode(&reject_stat::AUTH_ERROR)?;
                e.encode(status)
            }
        }
    }
}

impl Decode for auth_unix {
    fn decode(d: &mut Decoder) -> DecodeResult<Self> {
        Ok(Self {
            stamp: d.decode()?,
            machinename: d.opaque(255)?,
            uid: d.decode()?,
            gid: d.decode()?,
            gids: d.collection(16)?,
        })
    }
}

impl Decode for accept_stat_data {
    fn decode(d: &mut Decoder) -> DecodeResult<Self> {
        Ok(match d.decode::<accept_stat>()? {
            accept_stat::SUCCESS => Self::SUCCESS,
            accept_stat::PROG_UNAVAIL => Self::PROG_UNAVAIL,
            accept_stat::PROG_MISMATCH => Self::PROG_MISMATCH {
                low: d.decode()?,
                high: d.decode()?,
            },
            accept_stat::PROC_UNAVAIL => Self::PROC_UNAVAIL,
            accept_stat::GARBAGE_ARGS => Self::GARBAGE_ARGS,
            accept_stat::SYSTEM_ERR => Self::SYSTEM_ERR,
        })
    }
}

impl Decode for rejected_reply {
    fn decode(d: &mut Decoder) -> DecodeResult<Self> {
        Ok(match d.decode::<reject_stat>()? {
            reject_stat::RPC_MISMATCH => Self::RPC_MISMATCH {
                low: d.decode()?,
                high: d.decode()?,
            },
            reject_stat::AUTH_ERROR => Self::AUTH_ERROR(d.decode()?),
        })
    }
}
