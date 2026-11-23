use bytes::{Buf, Bytes, BytesMut};

use super::util::get_padding;
use super::{Error, Result};

/// Reads XDR values from a complete in-memory buffer.
///
/// Read values with [`Unpack::unpack`](super::Unpack::unpack). Malformed or truncated input returns
/// an [`Error`], and lengths are checked before anything is allocated.
///
/// Each decoder has a minimum shared length. Opaque data at least that long is
/// returned as a reference-counted [`Bytes`] sharing the input buffer, without
/// copying, and keeps the whole buffer in memory. Shorter opaque data is copied
/// into its own allocation. Small [`BoundedOpaque`](super::BoundedOpaque)
/// values are copied inline instead.
///
/// - [`Decoder::copying`] disables sharing: all opaque data is copied.
/// - [`RecordBuf::into_decoder`](crate::rpc::RecordBuf::into_decoder) sets the minimum to half of
///   the record's allocation, so READ and WRITE payloads are shared, and a shared value never keeps
///   more than twice its size in memory.
///
/// ```
/// use nfs3_types::xdr_codec::{Decoder, Unpack};
///
/// let mut input = Decoder::copying(&[0, 0, 0, 7][..]);
/// assert_eq!(u32::unpack(&mut input)?, 7);
/// # Ok::<(), nfs3_types::xdr_codec::Error>(())
/// ```
#[derive(Debug, Clone)]
pub struct Decoder {
    buf: Bytes,
    /// Minimum shared length; `usize::MAX` disables sharing.
    min_shared_len: usize,
}

impl Decoder {
    /// Creates a decoder over `buf` that copies opaque data instead of sharing it.
    pub fn copying(buf: impl Into<Bytes>) -> Self {
        let buf = buf.into();
        Self {
            buf,
            min_shared_len: usize::MAX,
        }
    }

    /// Creates a decoder whose minimum shared length is half of `buf`'s
    /// capacity.
    ///
    /// `buf` must own its whole allocation (never split or advanced), so that
    /// its capacity is the memory a shared value keeps alive.
    pub(crate) fn sharing(buf: BytesMut) -> Self {
        let min_shared_len = buf.capacity().div_ceil(2);
        let buf = buf.freeze();
        Self {
            buf,
            min_shared_len,
        }
    }

    /// Returns the number of bytes not decoded yet.
    #[must_use]
    pub const fn remaining(&self) -> usize {
        self.buf.len()
    }

    /// Returns `true` if all input has been decoded.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Returns the bytes that have not been decoded yet.
    #[must_use]
    pub fn into_remaining(self) -> Bytes {
        self.buf
    }

    /// Reads `N` bytes; `N` is a multiple of 4, so there is no padding.
    pub(crate) fn read_array<const N: usize>(&mut self) -> Result<[u8; N]> {
        const { assert!(N.is_multiple_of(4)) };
        if self.buf.len() < N {
            return Err(Error::UnexpectedEof);
        }
        let mut out = [0; N];
        self.buf.copy_to_slice(&mut out);
        Ok(out)
    }

    /// Reads `len` bytes of opaque data followed by its padding.
    ///
    /// The data shares the input (reference-counted) if `len` is at least the
    /// minimum shared length and is copied otherwise. The length is validated against the
    /// remaining input before anything is allocated.
    pub(crate) fn read_opaque(&mut self, len: usize) -> Result<Bytes> {
        let share = len >= self.min_shared_len;
        self.read_padded(len, |buf| {
            if share {
                buf.split_to(len)
            } else {
                let data = Bytes::copy_from_slice(&buf[..len]);
                buf.advance(len);
                data
            }
        })
    }

    pub(super) fn read_opaque_into(&mut self, data: &mut [u8]) -> Result<()> {
        self.read_padded(data.len(), |buf| buf.copy_to_slice(data))
    }

    fn read_padded<T>(&mut self, len: usize, read: impl FnOnce(&mut Bytes) -> T) -> Result<T> {
        let padding = get_padding(len);
        if len > self.buf.len() || padding > self.buf.len() - len {
            return Err(Error::InvalidLength(len));
        }
        let data = read(&mut self.buf);
        if self.buf[..padding].iter().any(|&b| b != 0) {
            return Err(Error::NonZeroPadding);
        }
        self.buf.advance(padding);
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use bytes::BufMut;

    use super::*;
    use crate::xdr_codec::{Opaque, Unpack};

    /// Decodes a `T` that must occupy all of `buf`.
    fn decode<T: Unpack>(buf: &'static [u8]) -> Result<T> {
        let mut input = Decoder::copying(buf);
        let value = T::unpack(&mut input)?;
        if !input.is_empty() {
            return Err(Error::TrailingBytes(input.remaining()));
        }
        Ok(value)
    }

    fn byte_range(bytes: &[u8]) -> std::ops::Range<usize> {
        let start = bytes.as_ptr() as usize;
        start..start + bytes.len()
    }

    /// Encodes a 4-byte opaque followed by a 100-byte one.
    fn small_and_large(capacity: usize) -> BytesMut {
        let mut record = BytesMut::with_capacity(capacity);
        record.put_u32(4);
        record.put_slice(b"name");
        record.put_u32(100);
        record.put_bytes(0xAB, 100);
        record
    }

    #[test]
    fn length_beyond_input_is_rejected_before_allocating() {
        // Claims a 4 GiB - 1 opaque with only four bytes of input.
        let mut input = Decoder::copying(&[0xff, 0xff, 0xff, 0xff, 1, 2, 3, 4][..]);
        assert_eq!(
            Opaque::unpack(&mut input).unwrap_err(),
            Error::InvalidLength(0xffff_ffff)
        );
        // Data fits but its padding does not.
        assert_eq!(
            decode::<Opaque<'_>>(&[0, 0, 0, 1, 7][..]).unwrap_err(),
            Error::InvalidLength(1)
        );
    }

    #[test]
    fn non_zero_padding_is_rejected() {
        assert_eq!(
            decode::<Opaque<'_>>(&[0, 0, 0, 1, 7, 0, 1, 0][..]).unwrap_err(),
            Error::NonZeroPadding
        );
        assert_eq!(
            decode::<Opaque<'_>>(&[0, 0, 0, 1, 7, 0, 0, 0][..]).unwrap(),
            Opaque::borrowed(&[7])
        );
    }

    #[test]
    fn decode_requires_full_consumption() {
        assert_eq!(
            decode::<u32>(&[0, 0, 0, 1, 0][..]).unwrap_err(),
            Error::TrailingBytes(1)
        );
        assert_eq!(
            decode::<u32>(&[0, 0, 0][..]).unwrap_err(),
            Error::UnexpectedEof
        );
    }

    #[test]
    fn large_opaque_shares_the_record_and_small_one_is_copied() {
        let record = small_and_large(112);
        let mut input = Decoder::sharing(record);
        let record_range = byte_range(&input.buf);
        let small = Opaque::unpack(&mut input).unwrap();
        let large = Opaque::unpack(&mut input).unwrap();
        assert!(input.is_empty());
        assert_eq!(&*small, b"name");
        assert!(!record_range.contains(&(small.as_ptr() as usize)));
        let large_range = byte_range(&large);
        assert!(record_range.start <= large_range.start && large_range.end <= record_range.end);
    }

    #[test]
    fn opaque_is_copied_when_the_allocation_is_large_or_unknown() {
        // Spare capacity counts: the large opaque is under half of the allocation.
        let mut input = Decoder::sharing(small_and_large(1024));
        let record_range = byte_range(&input.buf);
        Opaque::unpack(&mut input).unwrap();
        let large = Opaque::unpack(&mut input).unwrap();
        assert!(!record_range.contains(&(large.as_ptr() as usize)));

        // A slice of a larger buffer: the allocation is unknown.
        let parent = small_and_large(112).freeze();
        let mut input = Decoder::copying(parent.slice(8..));
        let large = Opaque::unpack(&mut input).unwrap();
        assert!(!byte_range(&parent).contains(&(large.as_ptr() as usize)));
    }
}
