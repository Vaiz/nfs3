#![allow(
    non_camel_case_types,
    clippy::large_enum_variant,
    clippy::upper_case_acronyms,
    reason = "Keep the RFC names and store XDR union arms inline"
)]

//! This module contains the definitions of the RPC protocol as defined in RFC 1057.

use std::io;

use bytes::BytesMut;
use nfs3_macros::XdrCodec;

use crate::xdr_codec::{Decoder, Encoder, Opaque, Pack, Unpack};

/// RPC header
///
/// The RPC header is a 32-bit integer that contains the length of the fragment and an EOF flag.
#[derive(Default, Copy, Clone, Debug, Eq, PartialEq, XdrCodec)]
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

/// Largest allocation made up front based on an advertised fragment length.
///
/// Covers a default-sized READ reply or WRITE call (1 MiB of data plus headers)
/// in one allocation.
const EAGER_RESERVE: usize = 1024 * 1024 + 4096;

/// Receive buffer that reassembles one RPC record (RFC 5531 record marking).
///
/// The caller does the I/O: it passes each fragment marker to
/// [`start_fragment`](Self::start_fragment), appends the fragment data to
/// [`buf_mut`](Self::buf_mut) until [`remaining`](Self::remaining) is zero,
/// and collects the record, without markers, with
/// [`take_record`](Self::take_record).
///
/// Memory follows the data actually received: about 1 MiB at most is reserved
/// from an advertised length and beyond that the buffer at most doubles, so a
/// peer cannot make the receiver allocate much more than it sends.
#[derive(Debug)]
pub struct RecordBuf {
    buf: BytesMut,
    fragment_end: usize,
    last: bool,
    max_len: usize,
}

impl RecordBuf {
    /// Creates an empty buffer for a record of at most `max_len` bytes.
    #[must_use]
    pub fn new(max_len: usize) -> Self {
        Self {
            buf: BytesMut::new(),
            fragment_end: 0,
            last: false,
            max_len,
        }
    }

    /// Starts the next fragment from its 4-byte record marker.
    ///
    /// # Errors
    ///
    /// Fails with [`io::ErrorKind::InvalidData`] if the record would exceed the
    /// size limit.
    pub fn start_fragment(&mut self, marker: [u8; 4]) -> io::Result<()> {
        debug_assert_eq!(self.remaining(), 0, "previous fragment is incomplete");
        let header = fragment_header::from(marker);
        self.fragment_end = self
            .buf
            .len()
            .checked_add(header.fragment_length() as usize)
            .filter(|end| *end <= self.max_len)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "RPC record exceeds the size limit",
                )
            })?;
        self.last = header.eof();
        Ok(())
    }

    /// Returns the number of bytes of the current fragment not received yet.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.fragment_end.saturating_sub(self.buf.len())
    }

    /// Returns the buffer to append fragment data to and the number of bytes
    /// it still expects, reserving room if the buffer is full.
    pub fn buf_mut(&mut self) -> (&mut BytesMut, usize) {
        let remaining = self.remaining();
        if self.buf.capacity() == self.buf.len() {
            self.buf
                .reserve(remaining.min(EAGER_RESERVE.max(self.buf.len())));
        }
        (&mut self.buf, remaining)
    }

    /// Returns a decoder over the record once its last fragment is complete,
    /// leaving the buffer empty for the next record.
    ///
    /// The decoder's minimum shared length is half of the record's allocation
    /// (see [`Decoder`]), so READ and WRITE payloads are shared with the record
    /// instead of copied.
    pub fn take_record(&mut self) -> Option<Decoder> {
        if !self.last || self.remaining() != 0 {
            return None;
        }
        debug_assert_eq!(self.buf.len(), self.fragment_end, "fragment overrun");
        self.fragment_end = 0;
        self.last = false;
        Some(Decoder::sharing(std::mem::take(&mut self.buf)))
    }
}

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
        let body = crate::xdr_codec::encode(auth).expect("failed to pack auth_unix");
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

#[cfg(test)]
mod tests {
    use super::{Decoder, EAGER_RESERVE, RecordBuf, io};

    #[test]
    fn eager_reservation_is_capped_and_then_grows_with_data() {
        let mut record = RecordBuf::new(usize::MAX);
        record.start_fragment([0, 0, 0, 100]).unwrap();
        let (buf, remaining) = record.buf_mut();
        assert_eq!((buf.capacity(), remaining), (100, 100));

        let mut record = RecordBuf::new(usize::MAX);
        record.start_fragment([0x7f, 0xff, 0xff, 0xff]).unwrap();
        let (buf, remaining) = record.buf_mut();
        assert_eq!(remaining, 0x7fff_ffff);
        assert_eq!(buf.capacity(), EAGER_RESERVE);
        buf.resize(EAGER_RESERVE, 0);
        assert!(record.buf_mut().0.capacity() <= 2 * EAGER_RESERVE);
    }

    #[test]
    fn fragments_are_joined_within_the_limit() {
        let mut record = RecordBuf::new(3);
        record.start_fragment([0, 0, 0, 2]).unwrap();
        record.buf_mut().0.extend_from_slice(b"ab");
        assert!(record.take_record().is_none());
        let err = record.start_fragment([0x80, 0, 0, 2]).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        record.start_fragment([0x80, 0, 0, 1]).unwrap();
        assert!(record.take_record().is_none());
        record.buf_mut().0.extend_from_slice(b"c");
        let take = |record: &mut RecordBuf| record.take_record().map(Decoder::into_remaining);
        assert_eq!(take(&mut record).unwrap(), b"abc"[..]);

        record.start_fragment([0x80, 0, 0, 0]).unwrap();
        assert_eq!(take(&mut record).unwrap(), b""[..]);
    }
}
