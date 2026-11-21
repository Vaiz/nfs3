use bytes::{Buf, BufMut, Bytes, BytesMut};

use super::util::{add_padding, get_padding, zero_padding};
use super::{Error, Pack, Result};
use crate::rpc::fragment_header;

/// Shared payloads shorter than this are copied into the encoder head.
///
/// Sending a payload as its own chunk saves a copy but costs an extra
/// vectored-write slice, which is only worth it for bulk data such as READ
/// and WRITE payloads.
const MIN_SHARED_CHUNK: usize = 1024;

/// Upper bound for the head capacity reserved up front by [`Encoder::with_size_hint`].
///
/// Larger heads grow on demand, so a size hint that includes a shared payload
/// never reserves memory for it.
const MAX_INITIAL_HEAD: usize = 1024;

const MARKER_LEN: usize = 4;

/// XDR output: a contiguous head buffer plus at most one shared payload.
///
/// Fixed-size fields and small opaque values are copied into the head. The
/// first large [`Opaque`](super::Opaque) value that already owns its bytes
/// (the data of a READ reply or WRITE call) is kept as a separate chunk, so it
/// is never copied while encoding. An RPC message carries at most one such
/// payload; any further one is copied.
#[derive(Debug)]
pub struct Encoder {
    /// Starts with a slot for the record marker filled in by
    /// [`finish_record`](Self::finish_record).
    head: BytesMut,
    /// The shared payload and everything encoded after it.
    payload: Option<(Bytes, BytesMut)>,
}

impl Default for Encoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Encoder {
    /// Creates an empty encoder.
    #[must_use]
    pub fn new() -> Self {
        Self::with_head_capacity(0)
    }

    /// Creates an encoder for a value of about `size_hint` encoded bytes.
    ///
    /// At most 1 KiB is reserved up front and the head grows on demand, so a
    /// hint that includes a shared payload never reserves memory for it.
    #[must_use]
    pub fn with_size_hint(size_hint: usize) -> Self {
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
    pub fn len(&self) -> usize {
        let payload = self.payload.as_ref().map_or(0, |(data, tail)| {
            data.len() + tail.len().max(get_padding(data.len()))
        });
        self.head.len() - MARKER_LEN + payload
    }

    /// Returns `true` if nothing has been encoded yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the buffer that receives copied bytes.
    fn out(&mut self) -> &mut BytesMut {
        match &mut self.payload {
            None => &mut self.head,
            Some((data, tail)) => {
                // The payload padding stays implicit until something follows it.
                if tail.is_empty() {
                    tail.extend_from_slice(zero_padding(data.len()));
                }
                tail
            }
        }
    }

    pub(crate) fn put_slice(&mut self, data: &[u8]) {
        self.out().extend_from_slice(data);
    }

    /// Copies `data` and its padding.
    pub(crate) fn put_padded(&mut self, data: &[u8]) {
        let out = self.out();
        // Reserve exactly so a large copy does not double the buffer afterwards.
        out.reserve(add_padding(data.len()));
        out.extend_from_slice(data);
        out.extend_from_slice(zero_padding(data.len()));
    }

    /// Appends `data` and its padding, keeping a large payload as a separate chunk.
    pub(crate) fn put_padded_shared(&mut self, data: &Bytes) {
        if self.payload.is_some() || data.len() < MIN_SHARED_CHUNK {
            self.put_padded(data);
        } else {
            self.payload = Some((data.clone(), BytesMut::new()));
        }
    }

    /// Completes the encoding of a plain XDR value.
    #[must_use]
    pub fn finish(mut self) -> Encoded {
        self.head.advance(MARKER_LEN);
        self.into_encoded()
    }

    /// Completes the encoding of a single-fragment RPC record.
    ///
    /// The returned chunks start with the record marker (length and
    /// last-fragment bit). Fails if the record is too long for one fragment.
    pub fn finish_record(mut self) -> Result<Encoded> {
        let marker = record_marker(self.len())?;
        self.head[..MARKER_LEN].copy_from_slice(&marker);
        Ok(self.into_encoded())
    }

    fn into_encoded(self) -> Encoded {
        let (payload, tail) = match self.payload {
            None => (Bytes::new(), Bytes::new()),
            Some((data, tail)) if tail.is_empty() => {
                let padding = Bytes::from_static(zero_padding(data.len()));
                (data, padding)
            }
            Some((data, tail)) => (data, tail.freeze()),
        };
        Encoded {
            head: self.head,
            payload,
            tail,
        }
    }
}

fn record_marker(len: usize) -> Result<[u8; MARKER_LEN]> {
    let len = u32::try_from(len)
        .ok()
        .filter(|len| *len <= fragment_header::MASK)
        .ok_or(Error::ObjectTooLarge(len))?;
    Ok(fragment_header::new(len, true).into_xdr_buf())
}

/// Encoded bytes: a head, an optional shared payload and a tail, sent in order.
#[derive(Debug)]
pub struct Encoded {
    head: BytesMut,
    payload: Bytes,
    tail: Bytes,
}

impl Encoded {
    /// Returns the chunks to send, in order; the last two are empty when there
    /// is no shared payload.
    #[must_use]
    pub fn chunks(&self) -> [&[u8]; 3] {
        [&self.head, &self.payload, &self.tail]
    }

    /// Returns all bytes in one buffer, copying only if there is a shared payload.
    #[must_use]
    pub fn into_bytes(self) -> Bytes {
        let mut out = self.head;
        if !self.payload.is_empty() || !self.tail.is_empty() {
            // Reuses spare head capacity, e.g. the full size reserved by `encode`.
            out.reserve(self.payload.len() + self.tail.len());
            out.extend_from_slice(&self.payload);
            out.extend_from_slice(&self.tail);
        }
        out.freeze()
    }
}

/// Encodes `value` into one contiguous buffer.
pub fn encode<T: Pack + ?Sized>(value: &T) -> Result<Bytes> {
    let mut out = Encoder::with_head_capacity(value.packed_size());
    value.pack(&mut out)?;
    Ok(out.finish().into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xdr_codec::{Decoder, Opaque, Unpack};

    #[test]
    fn into_bytes_appends_the_payload_to_the_reserved_head() {
        let payload = Bytes::from(vec![0xCD; MIN_SHARED_CHUNK]);
        let value = Opaque::from_bytes(payload);
        let mut out = Encoder::with_head_capacity(value.packed_size());
        value.pack(&mut out).unwrap();
        let encoded = out.finish();
        let head = encoded.chunks()[0].as_ptr();
        let bytes = encoded.into_bytes();
        assert_eq!(bytes.as_ptr(), head);
        assert_eq!(&bytes[4..], &*value);
    }

    #[test]
    fn encoder_keeps_one_shared_payload_and_patches_marker() {
        let payload = Bytes::from(vec![0xCD; MIN_SHARED_CHUNK + 1]);
        let mut out = Encoder::with_size_hint(usize::MAX);
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

        let mut input = Decoder::new_copying(encoded.into_bytes());
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
    fn trailing_payload_padding_is_static() {
        let payload = Bytes::from(vec![0xCD; MIN_SHARED_CHUNK + 1]);
        let mut out = Encoder::new();
        Opaque::from_bytes(payload).pack(&mut out).unwrap();
        let encoded = out.finish();
        let [head, _, tail] = encoded.chunks();
        assert_eq!(
            head,
            u32::try_from(MIN_SHARED_CHUNK + 1).unwrap().to_be_bytes()
        );
        assert_eq!(tail, [0; 3]);
        assert_eq!(tail.as_ptr(), zero_padding(1).as_ptr());
    }

    #[test]
    fn small_shared_payloads_are_copied() {
        let mut out = Encoder::new();
        Opaque::from_bytes(Bytes::from_static(b"abc"))
            .pack(&mut out)
            .unwrap();
        let encoded = out.finish();
        assert_eq!(encoded.chunks(), [&b"\0\0\0\x03abc\0"[..], &[], &[]]);
    }

    #[test]
    fn record_length_limit_excludes_marker() {
        assert_eq!(record_marker(0x7fff_ffff).unwrap(), [0xff; 4]);
        assert_eq!(
            record_marker(0x8000_0000).unwrap_err(),
            Error::ObjectTooLarge(0x8000_0000)
        );
    }
}
