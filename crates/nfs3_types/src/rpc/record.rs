//! Record marking (RFC 5531, section 11): the 4-byte fragment markers that
//! delimit RPC records on stream transports.

use std::io;

use bytes::BytesMut;
use nfs3_macros::XdrCodec;

use crate::xdr_codec::{self, Decoder};

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

    /// Creates the header of a `length`-byte fragment; `eof` marks the last
    /// fragment of a record.
    ///
    /// Fails if `length` exceeds [`MASK`](Self::MASK).
    pub fn new(length: usize, eof: bool) -> xdr_codec::Result<Self> {
        let mut header = u32::try_from(length)
            .ok()
            .filter(|length| *length <= Self::MASK)
            .ok_or(xdr_codec::Error::ObjectTooLarge(length))?;
        if eof {
            header |= Self::EOF_FLAG;
        }
        Ok(Self { header })
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

#[cfg(test)]
mod tests {
    use super::{EAGER_RESERVE, RecordBuf, fragment_header, io};
    use crate::xdr_codec::{Decoder, Error};

    #[test]
    fn fragment_length_is_limited_to_31_bits() {
        let header = fragment_header::new(0x7fff_ffff, true).unwrap();
        assert_eq!(header.into_xdr_buf(), [0xff; 4]);
        assert_eq!(
            fragment_header::new(0x8000_0000, true).unwrap_err(),
            Error::ObjectTooLarge(0x8000_0000)
        );
    }

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
