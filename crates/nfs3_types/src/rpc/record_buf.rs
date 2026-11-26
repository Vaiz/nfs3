//! Reassembly of RPC records from their fragments (RFC 5531, section 11).

use std::io;

use bytes::buf::Limit;
use bytes::{BufMut, BytesMut};

use super::fragment_header;
use crate::xdr_codec::Decoder;

/// Largest allocation made up front based on an advertised fragment length.
///
/// Covers a default-sized READ reply or WRITE call (1 MiB of data plus headers)
/// in one allocation.
const EAGER_RESERVE: usize = 1024 * 1024 + 4096;

/// Bounds framing work even when fragments carry no payload.
///
/// [Linux's `SVC_TCP_MAX_FRAGS`](https://github.com/torvalds/linux/blob/47439bb1635e8dd9245e876c2046b66a9b6f9400/net/sunrpc/svcsock.c)
/// uses 256 as a per-socket-lock processing budget and resumes later. Here the
/// same value is a per-record limit: excess fragments are rejected.
const MAX_FRAGMENTS: u16 = 256;

/// Receive buffer that reassembles one RPC record (RFC 5531 record marking).
///
/// Records may contain at most 256 fragments, including empty and final
/// fragments. This is an implementation limit, not an RFC requirement.
///
/// The caller does the I/O: it passes each fragment marker to
/// [`start_fragment`](Self::start_fragment), appends the fragment data to
/// [`buf_mut`](Self::buf_mut) until it returns `None`, and, once
/// [`is_complete`](Self::is_complete), turns the record, without markers, into
/// a decoder with [`into_decoder`](Self::into_decoder).
///
/// Memory follows the data actually received: about 1 MiB at most is reserved
/// from an advertised length and beyond that the buffer at most doubles, so a
/// peer cannot make the receiver allocate much more than it sends.
#[derive(Debug)]
pub struct RecordBuf {
    buf: BytesMut,
    fragment_end: usize,
    fragments: u16,
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
            fragments: 0,
            last: false,
            max_len,
        }
    }

    /// Starts the next fragment from its 4-byte record marker.
    ///
    /// # Errors
    ///
    /// Fails with [`io::ErrorKind::InvalidData`] if the record would exceed the
    /// size limit or contain more than 256 fragments.
    ///
    /// # Panics
    ///
    /// Panics if the previous fragment is not exactly complete or was the last.
    pub fn start_fragment(&mut self, marker: [u8; 4]) -> io::Result<()> {
        assert!(
            !self.last && self.buf.len() == self.fragment_end,
            "previous fragment is incomplete or was the last"
        );
        if self.fragments >= MAX_FRAGMENTS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "RPC record exceeds the fragment count limit",
            ));
        }
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
        self.fragments += 1;
        Ok(())
    }

    /// Returns the buffer to append the current fragment's data to, limited to
    /// the bytes not received yet, or `None` once the fragment is complete.
    ///
    /// Reserves room if the buffer is full. Appending through
    /// [`Limit::get_mut`] past the limit leaves the record never complete.
    pub fn buf_mut(&mut self) -> Option<Limit<&mut BytesMut>> {
        let remaining = self
            .fragment_end
            .checked_sub(self.buf.len())
            .filter(|remaining| *remaining != 0)?;
        if self.buf.capacity() == self.buf.len() {
            self.buf
                .reserve(remaining.min(EAGER_RESERVE.max(self.buf.len())));
        }
        Some((&mut self.buf).limit(remaining))
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
    use bytes::BufMut;

    use super::{EAGER_RESERVE, RecordBuf, io};

    #[test]
    fn eager_reservation_is_capped_and_then_grows_with_data() {
        let mut record = RecordBuf::new(usize::MAX);
        record.start_fragment([0, 0, 0, 100]).unwrap();
        let buf = record.buf_mut().unwrap();
        assert_eq!((buf.get_ref().capacity(), buf.remaining_mut()), (100, 100));

        let mut record = RecordBuf::new(usize::MAX);
        record.start_fragment([0x7f, 0xff, 0xff, 0xff]).unwrap();
        let mut buf = record.buf_mut().unwrap();
        assert_eq!(buf.remaining_mut(), 0x7fff_ffff);
        assert_eq!(buf.get_ref().capacity(), EAGER_RESERVE);
        buf.put_bytes(0, EAGER_RESERVE);
        let buf = record.buf_mut().unwrap();
        assert!(buf.get_ref().capacity() <= 2 * EAGER_RESERVE);
    }

    #[test]
    fn fragments_are_joined_within_the_limit() {
        let mut record = RecordBuf::new(3);
        record.start_fragment([0, 0, 0, 2]).unwrap();
        record.buf_mut().unwrap().put_slice(b"ab");
        assert!(record.buf_mut().is_none());
        assert!(!record.is_complete());
        let err = record.start_fragment([0x80, 0, 0, 2]).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        record.start_fragment([0x80, 0, 0, 1]).unwrap();
        assert!(!record.is_complete());
        record.buf_mut().unwrap().put_slice(b"c");
        assert!(record.is_complete());
        assert_eq!(record.into_decoder().into_remaining(), b"abc"[..]);

        let mut record = RecordBuf::new(0);
        record.start_fragment([0x80, 0, 0, 0]).unwrap();
        assert!(record.buf_mut().is_none());
        assert!(record.is_complete());
        assert_eq!(record.into_decoder().into_remaining(), b""[..]);
    }

    #[test]
    fn accepts_final_fragment_at_the_count_limit() {
        for payload_len in [0, 1] {
            let mut record = RecordBuf::new(usize::MAX);
            for _ in 0..255 {
                record.start_fragment([0, 0, 0, payload_len]).unwrap();
                if let Some(mut buf) = record.buf_mut() {
                    buf.put_u8(b'a');
                }
            }
            record.start_fragment([0x80, 0, 0, 1]).unwrap();
            record.buf_mut().unwrap().put_u8(b'b');
            assert!(record.is_complete());
            let mut expected = vec![b'a'; 255 * usize::from(payload_len)];
            expected.push(b'b');
            assert_eq!(record.into_decoder().into_remaining(), expected);
        }
    }

    #[test]
    fn rejects_excessive_fragments_even_with_no_payload() {
        for payload_len in [0, 1] {
            let mut record = RecordBuf::new(usize::MAX);
            for _ in 0..256 {
                record.start_fragment([0, 0, 0, payload_len]).unwrap();
                if let Some(mut buf) = record.buf_mut() {
                    buf.put_u8(b'a');
                }
            }
            for marker in [[0; 4], [0x80, 0, 0, 0], [0, 0, 0, 1], [0x80, 0, 0, 1]] {
                let err = record.start_fragment(marker).unwrap_err();
                assert_eq!(err.kind(), io::ErrorKind::InvalidData);
                assert_eq!(
                    err.to_string(),
                    "RPC record exceeds the fragment count limit"
                );
                assert!(!record.is_complete());
                assert!(record.buf_mut().is_none());
            }
        }
    }

    #[test]
    fn data_past_the_last_fragment_is_never_complete() {
        let mut record = RecordBuf::new(usize::MAX);
        record.start_fragment([0x80, 0, 0, 1]).unwrap();
        record.buf_mut().unwrap().get_mut().extend_from_slice(b"ab");
        assert!(record.buf_mut().is_none());
        assert!(!record.is_complete());
    }
}
