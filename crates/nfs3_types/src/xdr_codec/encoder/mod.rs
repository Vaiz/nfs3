mod encoded;

use bytes::{Buf, BufMut, Bytes, BytesMut};
pub use encoded::Encoded;

use super::Result;
use super::util::{add_padding, zero_padding};
use crate::rpc::fragment_header;

/// Shared payloads shorter than this are copied into the encoder head.
///
/// Sending a payload as its own chunk saves a copy but costs an extra
/// vectored-write slice, which is only worth it for bulk data such as READ
/// and WRITE payloads.
const MIN_SHARED_CHUNK: usize = 1024;

/// Upper bound for the head capacity reserved up front by [`Encoder::new`].
///
/// Larger heads grow on demand, so a size hint that includes a shared payload
/// never reserves memory for it.
const MAX_INITIAL_HEAD: usize = 1024;

const MARKER_LEN: usize = 4;

/// Writes XDR values.
///
/// Write values with [`Pack::pack`](super::Pack::pack), then call
/// [`finish_record`](Self::finish_record).
///
/// Everything is copied, except the first [`Opaque`](super::Opaque) of at least
/// 1 KiB that holds [`Bytes`], such as READ or WRITE data: the [`Encoded`]
/// record keeps a reference-counted clone of it instead.
///
/// ```
/// use bytes::Buf;
/// use nfs3_types::xdr_codec::{Encoder, Pack};
///
/// let mut out = Encoder::new(4);
/// 7u32.pack(&mut out)?;
/// let mut record = out.finish_record()?;
/// let bytes = record.copy_to_bytes(record.remaining());
/// assert_eq!(bytes, &[0x80, 0, 0, 4, 0, 0, 0, 7][..]);
/// # Ok::<(), nfs3_types::xdr_codec::Error>(())
/// ```
#[derive(Debug)]
pub struct Encoder {
    /// Starts with a slot for the record marker filled in by
    /// [`finish_record`](Self::finish_record).
    head: BytesMut,
    /// The shared payload, then its padding and everything encoded after it.
    payload: Option<(Bytes, BytesMut)>,
}

impl Encoder {
    /// Creates an encoder for a value of about `size_hint` bytes.
    ///
    /// At most 1 KiB is reserved up front.
    #[must_use]
    pub fn new(size_hint: usize) -> Self {
        Self::with_head_capacity(size_hint.min(MAX_INITIAL_HEAD))
    }

    fn with_head_capacity(capacity: usize) -> Self {
        let mut head = BytesMut::with_capacity(MARKER_LEN + capacity);
        head.put_bytes(0, MARKER_LEN);
        Self {
            head,
            payload: None,
        }
    }

    /// Returns the number of encoded bytes, excluding the record marker.
    #[must_use]
    #[expect(
        clippy::len_without_is_empty,
        reason = "callers only compare the length with the expected size"
    )]
    pub fn len(&self) -> usize {
        let payload_and_tail_len = self
            .payload
            .as_ref()
            .map_or(0, |(data, tail)| data.len() + tail.len());

        self.head.len() - MARKER_LEN + payload_and_tail_len
    }

    /// Returns the buffer that receives copied bytes.
    const fn out(&mut self) -> &mut BytesMut {
        match &mut self.payload {
            None => &mut self.head,
            Some((_, tail)) => tail,
        }
    }

    /// Copies `data`, whose size is a multiple of 4 and so needs no padding.
    pub(crate) fn put_array<const N: usize>(&mut self, data: &[u8; N]) {
        const { assert!(N.is_multiple_of(4)) };
        self.out().extend_from_slice(data);
    }

    /// Copies `data` and adds padding.
    pub(crate) fn put_padded(&mut self, data: &[u8]) {
        let out = self.out();
        // Reserve exactly so a large copy does not double the buffer afterwards.
        out.reserve(add_padding(data.len()));
        out.extend_from_slice(data);
        out.extend_from_slice(zero_padding(data.len()));
    }

    /// Appends `data` and its padding, keeping the first large payload as a
    /// separate chunk; any further one is copied.
    pub(crate) fn put_padded_shared(&mut self, data: &Bytes) {
        if self.payload.is_some() || data.len() < MIN_SHARED_CHUNK {
            self.put_padded(data);
        } else {
            let padding = BytesMut::from(zero_padding(data.len()));
            self.payload = Some((data.clone(), padding));
        }
    }

    /// Returns the encoded bytes, without a record marker, as one buffer.
    ///
    /// A shared payload is copied into it.
    #[must_use]
    pub(crate) fn into_bytes(mut self) -> Bytes {
        self.head.advance(MARKER_LEN);
        self.into_encoded().into_bytes()
    }

    /// Completes the encoding of a single-fragment RPC record.
    ///
    /// The record starts with the record marker (length and last-fragment bit).
    /// Fails if the record is too long for one fragment.
    pub fn finish_record(mut self) -> Result<Encoded> {
        let marker = fragment_header::new(self.len(), true)?.into_xdr_buf();
        self.head[..MARKER_LEN].copy_from_slice(&marker);
        Ok(self.into_encoded())
    }

    fn into_encoded(self) -> Encoded {
        let (payload, tail) = self.payload.unwrap_or_default();
        Encoded {
            head: self.head,
            payload,
            tail: tail.freeze(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xdr_codec::{Decoder, Opaque, Pack, Unpack};

    /// Like [`Encoder::into_bytes`], but keeps the chunks.
    fn plain(mut out: Encoder) -> Encoded {
        out.head.advance(MARKER_LEN);
        out.into_encoded()
    }

    #[test]
    fn into_bytes_appends_the_payload_to_the_reserved_head() {
        let payload = Bytes::from(vec![0xCD; MIN_SHARED_CHUNK]);
        let value = Opaque::from_bytes(payload);
        let mut out = Encoder::with_head_capacity(value.packed_size());
        value.pack(&mut out).unwrap();
        let encoded = plain(out);
        let head = encoded.chunks()[0].as_ptr();
        let bytes = encoded.into_bytes();
        assert_eq!(bytes.as_ptr(), head);
        assert_eq!(&bytes[4..], &*value);
    }

    #[test]
    fn encoder_keeps_one_shared_payload_and_patches_marker() {
        let payload = Bytes::from(vec![0xCD; MIN_SHARED_CHUNK + 1]);
        let mut out = Encoder::new(usize::MAX);
        assert!(out.head.capacity() <= MARKER_LEN + MAX_INITIAL_HEAD);
        7u32.pack(&mut out).unwrap();
        Opaque::from_bytes(payload.clone()).pack(&mut out).unwrap();
        // A second shared payload and anything after the first are copied.
        Opaque::from_bytes(payload.clone()).pack(&mut out).unwrap();
        Opaque::borrowed(b"tail").pack(&mut out).unwrap();
        let len = out.len();
        let padded = MIN_SHARED_CHUNK + 4;
        assert_eq!(len, 4 + 2 * (4 + padded) + 4 + 4);

        let encoded = out.finish_record().unwrap();
        let [head, shared, tail] = encoded.chunks();
        assert_eq!(head.len(), 4 + 4 + 4);
        assert_eq!(shared.as_ptr(), payload.as_ptr());
        assert_eq!(tail.len(), 3 + 4 + padded + 8);
        let marker = u32::from_be_bytes(head[..4].try_into().unwrap());
        assert_eq!(marker, 0x8000_0000 | u32::try_from(len).unwrap());

        let mut input = Decoder::copying(encoded.into_bytes());
        input.read_array::<4>().unwrap();
        assert_eq!(u32::unpack(&mut input).unwrap(), 7);
        for _ in 0..2 {
            assert_eq!(
                Opaque::unpack(&mut input).unwrap(),
                Opaque::from_bytes(payload.clone())
            );
        }
        assert_eq!(&*Opaque::unpack(&mut input).unwrap(), b"tail");
        assert!(input.is_empty());
    }

    #[test]
    fn trailing_payload_is_padded() {
        let payload = Bytes::from(vec![0xCD; MIN_SHARED_CHUNK + 1]);
        let mut out = Encoder::new(0);
        Opaque::from_bytes(payload).pack(&mut out).unwrap();
        let encoded = plain(out);
        let [head, _, tail] = encoded.chunks();
        assert_eq!(
            head,
            u32::try_from(MIN_SHARED_CHUNK + 1).unwrap().to_be_bytes()
        );
        assert_eq!(tail, [0; 3]);
    }

    #[test]
    fn small_shared_payloads_are_copied() {
        let mut out = Encoder::new(0);
        Opaque::from_bytes(Bytes::from_static(b"abc"))
            .pack(&mut out)
            .unwrap();
        let encoded = out.finish_record().unwrap();
        assert_eq!(
            encoded.chunks(),
            [&b"\x80\0\0\x08\0\0\0\x03abc\0"[..], &[], &[]]
        );
    }
}
