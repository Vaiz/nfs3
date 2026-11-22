//! Reassembly of RPC records from their fragments (RFC 5531, section 11).

use std::io;

use bytes::BytesMut;

use super::fragment_header;
use crate::xdr_codec::Decoder;

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
/// and, once [`is_complete`](Self::is_complete), turns the record, without
/// markers, into a decoder with [`into_decoder`](Self::into_decoder).
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
    ///
    /// # Panics
    ///
    /// Panics if the previous fragment is not exactly complete or was the last.
    pub fn start_fragment(&mut self, marker: [u8; 4]) -> io::Result<()> {
        assert!(
            !self.last && self.buf.len() == self.fragment_end,
            "previous fragment is incomplete or was the last"
        );
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
    ///
    /// Append at most the returned number of bytes: a record with data past
    /// its last fragment is never complete.
    pub fn buf_mut(&mut self) -> (&mut BytesMut, usize) {
        let remaining = self.remaining();
        if self.buf.capacity() == self.buf.len() {
            self.buf
                .reserve(remaining.min(EAGER_RESERVE.max(self.buf.len())));
        }
        (&mut self.buf, remaining)
    }

    /// Returns `true` once the last fragment of the record is complete.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.last && self.buf.len() == self.fragment_end
    }

    /// Returns a decoder over the complete record.
    ///
    /// The decoder's minimum shared length is half of the record's allocation
    /// (see [`Decoder`]), so READ and WRITE payloads are shared with the record
    /// instead of copied.
    ///
    /// # Panics
    ///
    /// Panics if the record is not [complete](Self::is_complete).
    #[must_use]
    pub fn into_decoder(self) -> Decoder {
        assert!(self.is_complete(), "record is incomplete");
        Decoder::sharing(self.buf)
    }
}

#[cfg(test)]
mod tests {
    use super::{EAGER_RESERVE, RecordBuf, io};

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
        assert!(!record.is_complete());
        let err = record.start_fragment([0x80, 0, 0, 2]).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        record.start_fragment([0x80, 0, 0, 1]).unwrap();
        assert!(!record.is_complete());
        record.buf_mut().0.extend_from_slice(b"c");
        assert!(record.is_complete());
        assert_eq!(record.into_decoder().into_remaining(), b"abc"[..]);

        let mut record = RecordBuf::new(0);
        record.start_fragment([0x80, 0, 0, 0]).unwrap();
        assert!(record.is_complete());
        assert_eq!(record.into_decoder().into_remaining(), b""[..]);
    }

    #[test]
    fn data_past_the_last_fragment_is_never_complete() {
        let mut record = RecordBuf::new(usize::MAX);
        record.start_fragment([0x80, 0, 0, 1]).unwrap();
        record.buf_mut().0.extend_from_slice(b"ab");
        assert_eq!(record.remaining(), 0);
        assert!(!record.is_complete());
    }
}
