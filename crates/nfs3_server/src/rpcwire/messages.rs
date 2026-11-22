use std::io;
use std::num::NonZeroUsize;

use bytes::{Buf, BufMut, BytesMut};
use nfs3_types::rpc::RecordBuf;
use nfs3_types::xdr_codec::Decoder;
use tokio::io::{AsyncRead, AsyncReadExt};

use crate::units::KIBIBYTE;

/// Size of the read-ahead buffer used for record markers and small records.
const READ_AHEAD_SIZE: usize = 64 * KIBIBYTE as usize;

/// Reads RPC records (RFC 5531 record marking) from a byte stream.
///
/// Every record is returned as a [`Decoder`] over its own buffer, without
/// fragment markers. Small
/// records are copied out of a read-ahead buffer; the bulk of large records is
/// read directly from the input into the record buffer.
#[derive(Debug)]
pub(crate) struct RpcRecordReader<R> {
    input: R,
    read_ahead: BytesMut,
    max_rpc_record_size: usize,
}

impl<R: AsyncRead + Unpin> RpcRecordReader<R> {
    pub(crate) fn new(input: R, max_rpc_record_size: Option<NonZeroUsize>) -> Self {
        Self {
            input,
            read_ahead: BytesMut::with_capacity(READ_AHEAD_SIZE),
            max_rpc_record_size: max_rpc_record_size.map_or(usize::MAX, NonZeroUsize::get),
        }
    }

    /// Reads an RPC record, returning `Ok(None)` only at EOF between records.
    ///
    /// A partial record or a record exceeding the configured payload limit is an error.
    pub(crate) async fn read_record(&mut self) -> io::Result<Option<Decoder>> {
        if self.read_ahead.is_empty() && self.fill_read_ahead().await? == 0 {
            return Ok(None);
        }

        let mut record = RecordBuf::new(self.max_rpc_record_size);
        while !record.is_complete() {
            record.start_fragment(self.read_marker().await?)?;

            let (buf, remaining) = record.buf_mut();
            let buffered = remaining.min(self.read_ahead.len());
            buf.extend_from_slice(&self.read_ahead[..buffered]);
            self.read_ahead.advance(buffered);

            while record.remaining() != 0 {
                let (buf, remaining) = record.buf_mut();
                if self.input.read_buf(&mut buf.limit(remaining)).await? == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "incomplete RPC fragment payload",
                    ));
                }
            }
        }
        Ok(Some(record.into_decoder()))
    }

    async fn fill_read_ahead(&mut self) -> io::Result<usize> {
        self.read_ahead
            .reserve(READ_AHEAD_SIZE - self.read_ahead.len());
        self.input.read_buf(&mut self.read_ahead).await
    }

    async fn read_marker(&mut self) -> io::Result<[u8; 4]> {
        while self.read_ahead.len() < 4 {
            if self.fill_read_ahead().await? == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
        }
        let mut marker = [0; 4];
        self.read_ahead.copy_to_slice(&mut marker);
        Ok(marker)
    }
}

#[cfg(test)]
mod tests {
    use future_assert::FutureTestExt;

    use super::{NonZeroUsize, RpcRecordReader, io};

    #[test]
    fn record_limit_counts_payload_across_fragments_and_resets() {
        let input = &b"\0\0\0\x02ab\x80\0\0\x01c\x80\0\0\x03def\x80\0\0\0"[..];
        let mut reader = RpcRecordReader::new(input, NonZeroUsize::new(3));
        for expected in [b"abc".as_slice(), b"def", b""] {
            let record = reader
                .read_record()
                .unwrap_ready()
                .expect("valid record")
                .expect("expected a record");
            let record = record.into_remaining();
            assert_eq!(record, expected);
            // Records never share an allocation with the read-ahead buffer.
            if !record.is_empty() {
                record.try_into_mut().expect("record owns its allocation");
            }
        }
        assert!(
            reader
                .read_record()
                .unwrap_ready()
                .expect("clean EOF")
                .is_none()
        );
    }

    #[test]
    fn large_record_is_read_past_the_read_ahead() {
        let payload: Vec<u8> = (0..=255u8).cycle().take(3 * 1024 * 1024 + 100).collect();
        let mut input = fragment(&payload[..10], false);
        input.extend(fragment(&payload[10..], true));
        let record = RpcRecordReader::new(input.as_slice(), None)
            .read_record()
            .unwrap_ready()
            .expect("valid record")
            .expect("expected a record");
        assert_eq!(record.into_remaining(), payload);
    }

    #[test]
    fn configured_limit_rejects_before_reading_fragment_payload() {
        let record = &b"\0\0\0\x02ab\x80\0\0\x02cd"[..];
        let mut reader = RpcRecordReader::new(record, NonZeroUsize::new(3));
        let err = reader
            .read_record()
            .unwrap_ready()
            .expect_err("record exceeds the configured limit");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert_eq!(reader.read_ahead, b"cd"[..]);

        assert_eq!(
            RpcRecordReader::new(record, None)
                .read_record()
                .unwrap_ready()
                .expect("uncapped record")
                .expect("expected a record")
                .into_remaining(),
            b"abcd"[..],
        );
    }

    #[test]
    fn truncated_records_are_not_clean_eof() {
        for truncated in [&b"\x80"[..], &b"\x80\0\0\x02a"[..], &b"\0\0\0\0"[..]] {
            let err = RpcRecordReader::new(truncated, None)
                .read_record()
                .unwrap_ready()
                .expect_err("truncated record");
            assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
        }
    }

    fn fragment(data: &[u8], last: bool) -> Vec<u8> {
        let len = u32::try_from(data.len()).expect("fragment fits");
        let marker = if last { len | 0x8000_0000 } else { len };
        let mut out = marker.to_be_bytes().to_vec();
        out.extend_from_slice(data);
        out
    }
}
