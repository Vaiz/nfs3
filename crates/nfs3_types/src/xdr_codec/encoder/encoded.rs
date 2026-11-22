use std::io::IoSlice;

use bytes::{Buf, Bytes, BytesMut};

/// An encoded RPC record, returned by [`Encoder::finish_record`](super::Encoder::finish_record).
///
/// Read it through [`Buf`]. Write it with a vectored write, such as tokio's
/// `write_all_buf`, to send a shared payload without copying it.
#[derive(Debug)]
pub struct Encoded {
    /// The bytes before the shared payload.
    pub(super) head: BytesMut,
    /// The shared payload, or empty.
    pub(super) payload: Bytes,
    /// The bytes after the shared payload.
    pub(super) tail: Bytes,
}

impl Encoded {
    pub(super) fn chunks(&self) -> [&[u8]; 3] {
        [&self.head, &self.payload, &self.tail]
    }

    /// Joins the chunks, copying only if there is a shared payload.
    pub(super) fn into_bytes(self) -> Bytes {
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

impl Buf for Encoded {
    fn remaining(&self) -> usize {
        self.head.len() + self.payload.len() + self.tail.len()
    }

    fn chunk(&self) -> &[u8] {
        self.chunks()
            .into_iter()
            .find(|chunk| !chunk.is_empty())
            .unwrap_or_default()
    }

    fn advance(&mut self, mut cnt: usize) {
        let n = cnt.min(self.head.len());
        self.head.advance(n);
        cnt -= n;
        let n = cnt.min(self.payload.len());
        self.payload.advance(n);
        self.tail.advance(cnt - n);
    }

    fn chunks_vectored<'a>(&'a self, dst: &mut [IoSlice<'a>]) -> usize {
        let chunks = self.chunks().into_iter().filter(|chunk| !chunk.is_empty());
        let mut n = 0;
        for (slot, chunk) in dst.iter_mut().zip(chunks) {
            *slot = IoSlice::new(chunk);
            n += 1;
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buf_reads_across_chunks() {
        let mut encoded = Encoded {
            head: BytesMut::from(&b"0123456789"[..]),
            payload: Bytes::from(vec![0xCD; 11]),
            tail: Bytes::from_static(b"abcde"),
        };
        let expected = encoded.chunks().concat();

        // Short writes of 7 bytes end mid-chunk and span chunk boundaries.
        let mut written = Vec::<u8>::new();
        while encoded.has_remaining() {
            assert_eq!(encoded.remaining(), expected.len() - written.len());
            let mut slices = [IoSlice::new(&[]); 3];
            let n = encoded.chunks_vectored(&mut slices);
            assert_eq!(&*slices[0], encoded.chunk());
            let start = written.len();
            written.extend(slices[..n].iter().flat_map(|s| s.iter()).take(7));
            encoded.advance(written.len() - start);
        }
        assert_eq!(written, expected);
        assert!(encoded.chunk().is_empty());
    }
}
