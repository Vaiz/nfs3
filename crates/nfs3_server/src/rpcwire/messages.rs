use std::io::{self, Cursor};
use std::num::NonZeroUsize;

use anyhow::bail;
use nfs3_types::rpc::{
    accept_stat_data, accepted_reply, auth_stat, call_body, fragment_header, msg_body, opaque_auth,
    rejected_reply, reply_body, rpc_msg,
};
use nfs3_types::xdr_codec::{Pack, Unpack, Void};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt};

/// Contains all RPC record payload bytes, without fragment markers.
#[derive(Debug)]
pub(crate) struct RpcRecord(Vec<u8>);

impl RpcRecord {
    pub(crate) fn into_inner(self) -> Vec<u8> {
        self.0
    }
}

#[derive(Debug)]
pub(crate) struct RpcRecordReader<R> {
    input: R,
    max_rpc_record_size: Option<NonZeroUsize>,
}

impl<R: AsyncBufRead + Unpin> RpcRecordReader<R> {
    pub(crate) const fn new(input: R, max_rpc_record_size: Option<NonZeroUsize>) -> Self {
        Self {
            input,
            max_rpc_record_size,
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
        let mut marker = [0; 4];
        loop {
            self.input.read_exact(&mut marker).await?;
            let header = fragment_header::from(marker);
            let fragment_len = header.fragment_length() as usize;
            let total = payload.len().checked_add(fragment_len).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "RPC record size overflow")
            })?;
            if self
                .max_rpc_record_size
                .is_some_and(|max| total > max.get())
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "RPC record exceeds configured size limit",
                ));
            }

            self.append_fragment(&mut payload, fragment_len).await?;
            if header.eof() {
                return Ok(Some(RpcRecord(payload)));
            }
        }
    }

    async fn append_fragment(
        &mut self,
        payload: &mut Vec<u8>,
        mut remaining: usize,
    ) -> io::Result<()> {
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

/// A decoded RPC call header and its still-encoded procedure arguments.
pub(crate) struct RpcCall {
    xid: u32,
    body: call_body<'static>,
    data: Option<Vec<u8>>,
    message_start: usize, // offset of the start of the message in the data buffer
}

impl TryFrom<RpcRecord> for RpcCall {
    type Error = anyhow::Error;

    fn try_from(packed: RpcRecord) -> Result<Self, Self::Error> {
        let packed = packed.0;
        let (rpc, pos) = match rpc_msg::unpack(&mut Cursor::new(&packed)) {
            Ok(ok) => ok,
            Err(err) => {
                bail!("Failed to unpack RPC message: {err}");
            }
        };

        let xid = rpc.xid;
        let body = match rpc.body {
            msg_body::CALL(call) => call,
            msg_body::REPLY(_) => {
                bail!("Expected a CALL message, got REPLY. XID: {xid}");
            }
        };

        Ok(Self {
            xid,
            body,
            data: Some(packed),
            message_start: pos,
        })
    }
}

impl RpcCall {
    pub(crate) const fn xid(&self) -> u32 {
        self.xid
    }
    pub(crate) const fn body(&self) -> &call_body<'static> {
        &self.body
    }

    pub(crate) const fn take_data(&mut self) -> Cursor<Vec<u8>> {
        let data = self.data.take().expect("Data already taken");
        let mut cursor = Cursor::new(data);
        cursor.set_position(self.message_start as u64);
        cursor
    }

    pub(crate) fn into_success_reply<T>(self, message: &T) -> anyhow::Result<RpcCallOutcome>
    where
        T: Pack + 'static,
    {
        let rpc = rpc_msg {
            xid: self.xid,
            body: msg_body::REPLY(reply_body::MSG_ACCEPTED(accepted_reply {
                verf: opaque_auth::default(),
                reply_data: accept_stat_data::SUCCESS,
            })),
        };

        pack(&rpc, message).map(RpcCallOutcome::Reply)
    }

    pub(crate) fn into_rpc_mismatch(self) -> anyhow::Result<RpcCallOutcome> {
        use nfs3_types::rpc::RPC_VERSION_2;

        let reply =
            reply_body::MSG_DENIED(rejected_reply::rpc_mismatch(RPC_VERSION_2, RPC_VERSION_2));

        let rpc = rpc_msg {
            xid: self.xid,
            body: msg_body::REPLY(reply),
        };
        pack(&rpc, &Void).map(RpcCallOutcome::Reply)
    }

    pub(crate) fn into_auth_error(self, stat: auth_stat) -> anyhow::Result<RpcCallOutcome> {
        let rpc = rpc_msg {
            xid: self.xid,
            body: msg_body::REPLY(reply_body::MSG_DENIED(rejected_reply::auth_error(stat))),
        };
        pack(&rpc, &Void).map(RpcCallOutcome::Reply)
    }

    pub(crate) fn into_error_reply(self, err: accept_stat_data) -> anyhow::Result<RpcCallOutcome> {
        let rpc = rpc_msg {
            xid: self.xid,
            body: msg_body::REPLY(reply_body::MSG_ACCEPTED(accepted_reply {
                verf: opaque_auth::default(),
                reply_data: err,
            })),
        };
        pack(&rpc, &Void).map(RpcCallOutcome::Reply)
    }
}

pub(crate) enum RpcCallOutcome {
    Reply(RpcRecord),
    NoReply,
}

fn pack<T>(rpc: &rpc_msg, message: &T) -> anyhow::Result<RpcRecord>
where
    T: Pack + 'static,
{
    let size = rpc
        .packed_size()
        .checked_add(message.packed_size())
        .expect("Failed to calculate size");

    let mut cursor = Cursor::new(Vec::with_capacity(size));
    rpc.pack(&mut cursor)?;
    message.pack(&mut cursor)?;
    Ok(RpcRecord(cursor.into_inner()))
}

#[cfg(test)]
mod tests {
    use future_assert::FutureTestExt;
    use tokio::io::BufReader;

    use super::{NonZeroUsize, RpcRecordReader, io};

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
