#![allow(
    non_camel_case_types,
    clippy::large_enum_variant,
    clippy::upper_case_acronyms
)]

//! This module contains the definitions of the RPC protocol as defined in RFC 1057.

use nfs3_macros::XdrCodec;

use crate::xdr_codec::{Opaque, Pack, PackedSize, Read, Result, Unpack, Write};

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

#[derive(Copy, Clone, Debug, Eq, PartialEq, XdrCodec)]
#[repr(u32)]
pub enum auth_flavor {
    AUTH_NULL = 0,
    AUTH_UNIX = 1,
    AUTH_SHORT = 2,
    AUTH_DES = 3,
    // and more to be defined
}

#[derive(Debug, XdrCodec)]
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
            machinename: Opaque::owned(vec![]),
            uid: 0,
            gid: 0,
            gids: vec![],
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

impl<Out> Pack<Out> for accept_stat_data
where
    Out: Write,
{
    fn pack(&self, w: &mut Out) -> Result<usize> {
        let len = match self {
            accept_stat_data::SUCCESS => accept_stat::SUCCESS.pack(w)?,
            accept_stat_data::PROG_UNAVAIL => accept_stat::PROG_UNAVAIL.pack(w)?,
            accept_stat_data::PROG_MISMATCH { low, high } => {
                accept_stat::PROG_MISMATCH.pack(w)? + low.pack(w)? + high.pack(w)?
            }
            accept_stat_data::PROC_UNAVAIL => accept_stat::PROC_UNAVAIL.pack(w)?,
            accept_stat_data::GARBAGE_ARGS => accept_stat::GARBAGE_ARGS.pack(w)?,
            accept_stat_data::SYSTEM_ERR => accept_stat::SYSTEM_ERR.pack(w)?,
        };
        Ok(len)
    }
}

impl PackedSize for accept_stat_data {
    const PACKED_SIZE: Option<usize> = None;

    fn count_packed_size(&self) -> usize {
        4 + match self {
            accept_stat_data::SUCCESS => 0,
            accept_stat_data::PROG_UNAVAIL => 0,
            accept_stat_data::PROG_MISMATCH { .. } => 8,
            accept_stat_data::PROC_UNAVAIL => 0,
            accept_stat_data::GARBAGE_ARGS => 0,
            accept_stat_data::SYSTEM_ERR => 0,
        }
    }
}

impl<In> Unpack<In> for accept_stat_data
where
    In: Read,
{
    fn unpack(r: &mut In) -> Result<(Self, usize)> {
        let (accept_stat, len) = accept_stat::unpack(r)?;
        let (body, body_len) = match accept_stat {
            accept_stat::SUCCESS => (accept_stat_data::SUCCESS, 0),
            accept_stat::PROG_MISMATCH => {
                let (low, low_len) = u32::unpack(r)?;
                let (high, high_len) = u32::unpack(r)?;
                (
                    accept_stat_data::PROG_MISMATCH { low, high },
                    low_len + high_len,
                )
            }
            accept_stat::PROG_UNAVAIL => (accept_stat_data::PROG_UNAVAIL, 0),
            accept_stat::PROC_UNAVAIL => (accept_stat_data::PROC_UNAVAIL, 0),
            accept_stat::GARBAGE_ARGS => (accept_stat_data::GARBAGE_ARGS, 0),
            accept_stat::SYSTEM_ERR => (accept_stat_data::SYSTEM_ERR, 0),
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
    pub fn rpc_mismatch(low: u32, high: u32) -> Self {
        rejected_reply::RPC_MISMATCH { low, high }
    }
    pub fn auth_error(auth_stat: auth_stat) -> Self {
        rejected_reply::AUTH_ERROR(auth_stat)
    }
}

impl<Out> Pack<Out> for rejected_reply
where
    Out: Write,
{
    fn pack(&self, w: &mut Out) -> Result<usize> {
        let len = match self {
            rejected_reply::RPC_MISMATCH { low, high } => {
                reject_stat::RPC_MISMATCH.pack(w)? + low.pack(w)? + high.pack(w)?
            }
            rejected_reply::AUTH_ERROR(auth_stat) => {
                reject_stat::AUTH_ERROR.pack(w)? + auth_stat.pack(w)?
            }
        };
        Ok(len)
    }
}

impl PackedSize for rejected_reply {
    const PACKED_SIZE: Option<usize> = None;

    fn count_packed_size(&self) -> usize {
        4 + match self {
            rejected_reply::RPC_MISMATCH { .. } => 8,
            rejected_reply::AUTH_ERROR(_) => 4,
        }
    }
}

impl<In> Unpack<In> for rejected_reply
where
    In: Read,
{
    fn unpack(r: &mut In) -> Result<(Self, usize)> {
        let (reject_stat, len) = reject_stat::unpack(r)?;
        let (body, body_len) = match reject_stat {
            reject_stat::RPC_MISMATCH => {
                let (low, low_len) = u32::unpack(r)?;
                let (high, high_len) = u32::unpack(r)?;
                (
                    rejected_reply::RPC_MISMATCH { low, high },
                    low_len + high_len,
                )
            }
            reject_stat::AUTH_ERROR => {
                let (auth_stat, auth_stat_len) = auth_stat::unpack(r)?;
                (rejected_reply::AUTH_ERROR(auth_stat), auth_stat_len)
            }
        };
        Ok((body, len + body_len))
    }
}

#[derive(Debug)]
pub enum reply_body<'a> {
    MSG_ACCEPTED(accepted_reply<'a>),
    MSG_DENIED(rejected_reply),
}

impl<Out> Pack<Out> for reply_body<'_>
where
    Out: Write,
{
    fn pack(&self, w: &mut Out) -> Result<usize> {
        let len = match self {
            reply_body::MSG_ACCEPTED(accepted_reply) => {
                reply_stat::MSG_ACCEPTED.pack(w)? + accepted_reply.pack(w)?
            }
            reply_body::MSG_DENIED(rejected_reply) => {
                reply_stat::MSG_DENIED.pack(w)? + rejected_reply.pack(w)?
            }
        };
        Ok(len)
    }
}

impl PackedSize for reply_body<'_> {
    const PACKED_SIZE: Option<usize> = None;

    fn count_packed_size(&self) -> usize {
        4 + match self {
            reply_body::MSG_ACCEPTED(accepted_reply) => accepted_reply.packed_size(),
            reply_body::MSG_DENIED(rejected_reply) => rejected_reply.packed_size(),
        }
    }
}

impl<In> Unpack<In> for reply_body<'_>
where
    In: Read,
{
    fn unpack(r: &mut In) -> Result<(Self, usize)> {
        let (reply_stat, len) = reply_stat::unpack(r)?;
        let (body, body_len) = match reply_stat {
            reply_stat::MSG_ACCEPTED => {
                let (body, body_len) = accepted_reply::unpack(r)?;
                (reply_body::MSG_ACCEPTED(body), body_len)
            }
            reply_stat::MSG_DENIED => {
                let (body, body_len) = rejected_reply::unpack(r)?;
                (reply_body::MSG_DENIED(body), body_len)
            }
        };
        Ok((body, len + body_len))
    }
}

#[derive(Debug, XdrCodec)]
pub struct rpc_msg<'a, 'b> {
    pub xid: u32,
    pub body: msg_body<'a, 'b>,
}

#[derive(Debug)]
pub enum msg_body<'a, 'b> {
    CALL(call_body<'a>),
    REPLY(reply_body<'b>),
}

impl<Out> Pack<Out> for msg_body<'_, '_>
where
    Out: Write,
{
    fn pack(&self, w: &mut Out) -> Result<usize> {
        let len = match self {
            msg_body::CALL(call_body) => msg_type::CALL.pack(w)? + call_body.pack(w)?,
            msg_body::REPLY(reply_body) => msg_type::REPLY.pack(w)? + reply_body.pack(w)?,
        };
        Ok(len)
    }
}

impl PackedSize for msg_body<'_, '_> {
    const PACKED_SIZE: Option<usize> = None;

    fn count_packed_size(&self) -> usize {
        4 + match self {
            msg_body::CALL(call_body) => call_body.packed_size(),
            msg_body::REPLY(reply_body) => reply_body.packed_size(),
        }
    }
}

impl<In> Unpack<In> for msg_body<'_, '_>
where
    In: Read,
{
    fn unpack(r: &mut In) -> Result<(Self, usize)> {
        let (msg_type, len) = msg_type::unpack(r)?;
        let (body, body_len) = match msg_type {
            msg_type::CALL => {
                let (body, body_len) = call_body::unpack(r)?;
                (msg_body::CALL(body), body_len)
            }
            msg_type::REPLY => {
                let (body, body_len) = reply_body::unpack(r)?;
                (msg_body::REPLY(body), body_len)
            }
        };
        Ok((body, len + body_len))
    }
}
