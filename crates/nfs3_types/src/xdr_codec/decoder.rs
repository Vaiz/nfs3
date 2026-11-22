use bytes::{Buf, Bytes, BytesMut};

use super::util::get_padding;
use super::{Error, Result, Unpack};

/// Reads XDR values from a complete in-memory buffer.
///
/// Read values with [`Unpack::unpack`], or use [`decode`] when the buffer holds
/// exactly one value. Malformed or truncated input returns an [`Error`], and
/// lengths are checked before anything is allocated.
///
/// Each decoder has a minimum shared length. Opaque data at least that long is
/// returned as a reference into the input buffer, without copying, and keeps
/// the whole buffer in memory. Shorter opaque data is copied into its own
/// allocation.
///
/// - [`Decoder::new`] disables sharing: all opaque data is copied.
/// - [`RecordBuf::take_record`](crate::rpc::RecordBuf::take_record) sets the minimum to half of the
///   record's allocation, so READ and WRITE payloads are shared, and a shared value never keeps
///   more than twice its size in memory.
///
/// ```
/// use nfs3_types::xdr_codec::{Decoder, Unpack};
///
/// let mut input = Decoder::new(&[0, 0, 0, 7][..]);
/// assert_eq!(u32::unpack(&mut input)?, 7);
/// # Ok::<(), nfs3_types::xdr_codec::Error>(())
/// ```
#[derive(Debug, Clone)]
pub struct Decoder {
    buf: Bytes,
    input_len: usize,
    /// Minimum shared length; `usize::MAX` disables sharing.
    min_shared_len: usize,
}

impl Decoder {
    /// Creates a decoder over `buf` with sharing disabled: all opaque data is
    /// copied into its own allocation.
    pub fn new(buf: impl Into<Bytes>) -> Self {
        let buf = buf.into();
        Self {
            input_len: buf.len(),
            buf,
            min_shared_len: usize::MAX,
        }
    }

    /// Creates a decoder whose minimum shared length is half of `buf`'s
    /// capacity.
    ///
    /// `buf` must own its whole allocation (never split or advanced), so that
    /// its capacity is the memory a shared value keeps alive.
    pub(crate) fn from_owned(buf: BytesMut) -> Self {
        let min_shared_len = buf.capacity().div_ceil(2);
        let buf = buf.freeze();
        Self {
            input_len: buf.len(),
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

    /// Returns the number of bytes decoded so far.
    #[must_use]
    pub const fn position(&self) -> usize {
        self.input_len - self.buf.len()
    }

    /// Returns the bytes that have not been decoded yet.
    #[must_use]
    pub fn into_remaining(self) -> Bytes {
        self.buf
    }

    pub(crate) fn read_array<const N: usize>(&mut self) -> Result<[u8; N]> {
        if self.buf.len() < N {
            return Err(Error::UnexpectedEof);
        }
        let mut out = [0; N];
        self.buf.copy_to_slice(&mut out);
        Ok(out)
    }

    /// Skips and validates the zero padding that follows `len` data bytes.
    pub(crate) fn skip_padding(&mut self, len: usize) -> Result<()> {
        let padding = get_padding(len);
        let bytes = self.buf.get(..padding).ok_or(Error::UnexpectedEof)?;
        if bytes.iter().any(|&b| b != 0) {
            return Err(Error::NonZeroPadding);
        }
        self.buf.advance(padding);
        Ok(())
    }

    /// Reads `len` bytes of opaque data followed by its padding.
    ///
    /// The data references the input if `len` is at least the minimum shared
    /// length and is copied otherwise. The length is validated against the
    /// remaining input before anything is allocated.
    pub(crate) fn read_opaque(&mut self, len: usize) -> Result<Bytes> {
        if len > self.buf.len() || get_padding(len) > self.buf.len() - len {
            return Err(Error::InvalidLength(len));
        }
        let data = if len >= self.min_shared_len {
            self.buf.split_to(len)
        } else {
            let data = Bytes::copy_from_slice(&self.buf[..len]);
            self.buf.advance(len);
            data
        };
        self.skip_padding(len)?;
        Ok(data)
    }
}

/// Decodes a `T` that must occupy all of `buf`.
///
/// Sharing is disabled, as with [`Decoder::new`]: all opaque data is copied.
pub fn decode<T: Unpack>(buf: impl Into<Bytes>) -> Result<T> {
    let mut input = Decoder::new(buf);
    let value = T::unpack(&mut input)?;
    if !input.is_empty() {
        return Err(Error::TrailingBytes(input.remaining()));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use bytes::BufMut;

    use super::*;
    use crate::xdr_codec::Opaque;

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
        let mut input = Decoder::new(&[0xff, 0xff, 0xff, 0xff, 1, 2, 3, 4][..]);
        assert_eq!(
            Opaque::unpack(&mut input).unwrap_err(),
            Error::InvalidLength(0xffff_ffff)
        );
        // Data fits but its padding does not.
        assert_eq!(
            decode::<Opaque<'_>>(&[0, 0, 0, 1, 7][..]).unwrap_err(),
            Error::InvalidLength(1)
        );
        assert_eq!(
            decode::<Vec<u32>>(&[0, 0, 0, 2, 0, 0, 0, 1][..]).unwrap_err(),
            Error::InvalidLength(2)
        );
    }

    #[test]
    fn non_zero_padding_is_rejected() {
        assert_eq!(
            decode::<Opaque<'_>>(&[0, 0, 0, 1, 7, 0, 1, 0][..]).unwrap_err(),
            Error::NonZeroPadding
        );
        assert_eq!(
            decode::<[u8; 2]>(&[1, 2, 0, 1][..]).unwrap_err(),
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
        let mut input = Decoder::from_owned(record);
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
        let mut input = Decoder::from_owned(small_and_large(1024));
        let record_range = byte_range(&input.buf);
        Opaque::unpack(&mut input).unwrap();
        let large = Opaque::unpack(&mut input).unwrap();
        assert!(!record_range.contains(&(large.as_ptr() as usize)));

        // A slice of a larger buffer: the allocation is unknown.
        let parent = small_and_large(112).freeze();
        let mut input = Decoder::new(parent.slice(8..));
        let large = Opaque::unpack(&mut input).unwrap();
        assert!(!byte_range(&parent).contains(&(large.as_ptr() as usize)));
    }
}
