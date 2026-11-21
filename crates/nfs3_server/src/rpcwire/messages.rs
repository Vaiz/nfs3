use std::io;
use std::num::NonZeroUsize;

use nfs3_types::rpc::fragment_header;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt};

use crate::units::MEBIBYTE;

const MAX_EAGER_FRAGMENT_SIZE: usize = 16 * MEBIBYTE as usize;

/// Contains all RPC record payload bytes, without fragment markers.
#[derive(Debug)]
pub(crate) struct RpcRecord(Vec<u8>);

impl RpcRecord {
    pub(super) const fn new(payload: Vec<u8>) -> Self {
        Self(payload)
    }

    pub(crate) fn into_inner(self) -> Vec<u8> {
        self.0
    }
}

#[derive(Debug)]
pub(crate) struct RpcRecordReader<R> {
    input: R,
    max_rpc_record_size: usize,
}

impl<R: AsyncBufRead + Unpin> RpcRecordReader<R> {
    pub(crate) const fn new(input: R, max_rpc_record_size: Option<NonZeroUsize>) -> Self {
        Self {
            input,
            max_rpc_record_size: match max_rpc_record_size {
                Some(max) => max.get(),
                None => usize::MAX,
            },
        }
    }

    /// Reads an RPC record, returning `Ok(None)` only at EOF between records.
    ///
    /// A partial record or a record exceeding the configured payload limit is an error.
    pub(crate) async fn read_record(&mut self) -> io::Result<Option<RpcRecord>> {
        if self.input.fill_buf().await?.is_empty() {
            return Ok(None);
        }

        let mut payload = Vec::new();
        loop {
            let header = self.read_header().await?;
            let fragment_len = header.fragment_length() as usize;
            let total = payload.len().checked_add(fragment_len).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "RPC record size overflow")
            })?;
            if total > self.max_rpc_record_size {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "RPC record exceeds configured size limit",
                ));
            }

            self.read_full_fragment(&mut payload, fragment_len).await?;
            if header.eof() {
                return Ok(Some(RpcRecord(payload)));
            }
        }
    }

    async fn read_header(&mut self) -> io::Result<fragment_header> {
        let mut marker = [0; 4];
        self.input.read_exact(&mut marker).await?;
        Ok(fragment_header::from(marker))
    }

    async fn read_full_fragment(
        &mut self,
        payload: &mut Vec<u8>,
        fragment_len: usize,
    ) -> io::Result<()> {
        try_reserve_fragment(payload, fragment_len)?;
        let mut remaining = fragment_len;
        while remaining != 0 {
            let available = self.input.fill_buf().await?;
            if available.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "incomplete RPC fragment payload",
                ));
            }
            let count = remaining.min(available.len());
            payload.try_reserve(count).map_err(io::Error::other)?;
            payload.extend_from_slice(&available[..count]);
            self.input.consume(count);
            remaining -= count;
        }
        Ok(())
    }
}

fn try_reserve_fragment(payload: &mut Vec<u8>, fragment_len: usize) -> io::Result<()> {
    // Eager growth must at least double the buffered payload to avoid repeated copying.
    if fragment_len <= MAX_EAGER_FRAGMENT_SIZE && fragment_len >= payload.len() {
        payload
            .try_reserve_exact(fragment_len)
            .map_err(io::Error::other)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use future_assert::FutureTestExt;
    use tokio::io::BufReader;

    use super::{MAX_EAGER_FRAGMENT_SIZE, NonZeroUsize, RpcRecordReader, io, try_reserve_fragment};

    #[test]
    fn eager_reservation_stops_at_fragment_size_cutoff() {
        let mut payload = Vec::new();
        try_reserve_fragment(&mut payload, MAX_EAGER_FRAGMENT_SIZE + 1)
            .expect("fall back above the cutoff");
        assert_eq!(payload.capacity(), 0);

        try_reserve_fragment(&mut payload, MAX_EAGER_FRAGMENT_SIZE).expect("reserve at the cutoff");
        assert!(payload.capacity() >= MAX_EAGER_FRAGMENT_SIZE);
    }

    #[test]
    fn record_limit_counts_payload_across_fragments_and_resets() {
        let input = &b"\0\0\0\x02ab\x80\0\0\x01c\x80\0\0\x03def\x80\0\0\0"[..];
        let mut reader =
            RpcRecordReader::new(BufReader::with_capacity(5, input), NonZeroUsize::new(3));
        for expected in [b"abc".as_slice(), b"def", b""] {
            let record = reader
                .read_record()
                .unwrap_ready()
                .expect("valid record")
                .expect("expected a record");
            assert_eq!(record.into_inner(), expected);
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
    fn configured_limit_rejects_before_reading_fragment_payload() {
        let record = &b"\0\0\0\x02ab\x80\0\0\x02cd"[..];
        let mut reader = RpcRecordReader::new(record, NonZeroUsize::new(3));
        let err = reader
            .read_record()
            .unwrap_ready()
            .expect_err("record exceeds the configured limit");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert_eq!(reader.input, b"cd");

        assert_eq!(
            RpcRecordReader::new(record, None)
                .read_record()
                .unwrap_ready()
                .expect("uncapped record")
                .expect("expected a record")
                .into_inner(),
            b"abcd",
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
}
