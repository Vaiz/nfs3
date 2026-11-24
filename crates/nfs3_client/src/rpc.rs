//! RPC client implementation

use std::fmt::Debug;
use std::io;

use bytes::BufMut;
use nfs3_types::rpc::{
    RPC_VERSION_2, RecordBuf, accept_stat_data, call_body, msg_body, opaque_auth, reply_body,
    rpc_msg,
};
use nfs3_types::xdr_codec::{Decoder, Encoded, Encoder, Pack, Unpack};

use crate::RpcError;
use crate::io::{AsyncRead, AsyncWrite};

/// RPC client
pub struct RpcClient<IO> {
    io: Option<IO>,
    xid: u32,
    credential: opaque_auth<'static>,
    verifier: opaque_auth<'static>,
}

impl<IO> Debug for RpcClient<IO> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        f.debug_struct("RpcClient").finish()
    }
}

impl<IO> RpcClient<IO>
where
    IO: AsyncRead + AsyncWrite + Send,
{
    /// Create a new RPC client. XID is initialized to a random value.
    pub fn new(io: IO) -> Self {
        Self::new_with_auth(io, opaque_auth::default(), opaque_auth::default())
    }

    /// Create a new RPC client with custom credential and verifier.
    pub fn new_with_auth(
        io: IO,
        credential: opaque_auth<'static>,
        verifier: opaque_auth<'static>,
    ) -> Self {
        Self {
            io: Some(io),
            xid: fastrand::u32(..),
            credential,
            verifier,
        }
    }

    /// Call an RPC procedure
    ///
    /// # Errors and connection state
    ///
    /// On a successful return the connection is in a clean state and may be
    /// reused for the next call. When an error is returned, use
    /// [`RpcError::is_connection_reusable`] to decide whether the
    /// connection can be kept. The client drops the transport on an I/O
    /// error or an invalid reply.
    ///
    /// # Cancellation
    ///
    /// Dropping an unpolled call future is safe. Dropping it once I/O has
    /// started drops the transport: further calls fail with
    /// [`RpcError::ConnectionUnusable`] without touching the transport.
    /// The server may already have applied the operation, so reconnecting
    /// does not make it safe to blindly retry a mutation.
    #[expect(
        clippy::similar_names,
        reason = "prog and proc are fields of call_body"
    )]
    pub async fn call<C, R>(
        &mut self,
        prog: u32,
        vers: u32,
        proc: u32,
        args: &C,
    ) -> Result<R, RpcError>
    where
        R: Unpack,
        C: Pack + Send + Sync,
    {
        if self.io.is_none() {
            return Err(RpcError::ConnectionUnusable);
        }

        let call = call_body {
            rpcvers: RPC_VERSION_2,
            prog,
            vers,
            proc,
            cred: self.credential.borrow(),
            verf: self.verifier.borrow(),
        };
        let xid = self.xid;
        let msg = rpc_msg {
            xid,
            body: msg_body::CALL(call),
        };
        let mut record = Self::encode_call(&msg, args)?;
        self.xid = self.xid.wrapping_add(1);

        // The in-flight future owns the transport until a verified reply.
        let mut io = self.io.take().ok_or(RpcError::ConnectionUnusable)?;
        io.async_write_all(&mut record).await?;
        let result = Self::recv_reply(&mut io, xid).await;
        if result
            .as_ref()
            .err()
            .is_none_or(RpcError::is_connection_reusable)
        {
            self.io = Some(io);
        }
        result
    }

    fn encode_call<T: Pack>(msg: &rpc_msg<'_, '_>, args: &T) -> Result<Encoded, RpcError> {
        let total_len = msg
            .packed_size()
            .checked_add(args.packed_size())
            .ok_or(RpcError::WrongLength)?;
        if !total_len.is_multiple_of(4) {
            return Err(RpcError::WrongLength);
        }

        let mut out = Encoder::new(total_len);
        msg.pack(&mut out).map_err(RpcError::XdrEncode)?;
        args.pack(&mut out).map_err(RpcError::XdrEncode)?;
        if out.len() != total_len {
            return Err(RpcError::WrongLength);
        }
        out.finish_record().map_err(RpcError::XdrEncode)
    }

    async fn recv_reply<T>(io: &mut IO, xid: u32) -> Result<T, RpcError>
    where
        T: Unpack,
    {
        let mut input = Self::recv_record(io).await?;
        let resp_msg = rpc_msg::unpack(&mut input)?;

        if resp_msg.xid != xid {
            return Err(RpcError::UnexpectedXid);
        }

        let result = match resp_msg.body {
            msg_body::REPLY(reply_body::MSG_ACCEPTED(reply)) => match reply.reply_data {
                accept_stat_data::SUCCESS => Ok(T::unpack(&mut input)?),
                status => Err(RpcError::try_from(status)
                    .expect("accept_stat_data::SUCCESS is not a valid error")),
            },
            msg_body::REPLY(reply_body::MSG_DENIED(r)) => Err(RpcError::from(r)),
            msg_body::CALL(_) => return Err(RpcError::UnexpectedCall),
        };

        if !input.is_empty() {
            return Err(RpcError::NotFullyParsed {
                remaining: input.into_remaining(),
            });
        }
        result
    }

    /// Reads one RPC record (all fragments) into a single buffer.
    async fn recv_record(io: &mut IO) -> io::Result<Decoder> {
        let mut record = RecordBuf::new(usize::MAX);
        while !record.is_complete() {
            let mut marker = [0u8; 4];
            io.async_read_exact(&mut marker).await?;
            record.start_fragment(marker)?;
            while let Some(mut buf) = record.buf_mut() {
                // Read only what fits in the reserved capacity, so memory keeps
                // following the received data.
                let len = buf.chunk_mut().len();
                let buf = buf.get_mut();
                let expected = buf.len() + len;
                io.async_read_exact_buf(buf, len).await?;
                if buf.len() != expected {
                    return Err(io::Error::other(
                        "async_read_exact_buf appended the wrong number of bytes",
                    ));
                }
            }
        }
        Ok(record.into_decoder())
    }
}

#[cfg(test)]
mod tests {
    use std::future::poll_fn;
    use std::io;
    use std::pin::pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Waker};

    use bytes::{Buf, Bytes, BytesMut};
    use future_assert::FutureTestExt;
    use nfs3_types::nfs3::{GETATTR3res, nfsstat3};
    use nfs3_types::rpc::{
        accept_stat_data, accepted_reply, call_body, fragment_header, msg_body, opaque_auth,
        rejected_reply, reply_body, rpc_msg,
    };
    use nfs3_types::xdr_codec::{self, Encoded, Encoder, Opaque, Pack, Void};

    use super::RpcClient;
    use crate::RpcError;
    use crate::io::{AsyncRead, AsyncWrite};

    /// In-memory transport that transfers at most 7 bytes per read or write.
    #[derive(Default)]
    struct Mock {
        input: Bytes,
        output: Vec<u8>,
        stats: Arc<Mutex<IoStats>>,
        read_budget: Option<usize>,
        write_budget: Option<usize>,
        write_error: bool,
    }

    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
    struct IoStats {
        reads: usize,
        writes: usize,
        bytes_read: usize,
        bytes_written: usize,
        dropped: bool,
    }

    impl Drop for Mock {
        fn drop(&mut self) {
            self.stats.lock().unwrap().dropped = true;
        }
    }

    impl AsyncRead for Mock {
        fn async_read(&mut self, buf: &mut [u8]) -> impl Future<Output = io::Result<usize>> + Send {
            poll_fn(move |_| {
                let mut stats = self.stats.lock().unwrap();
                stats.reads += 1;
                let mut n = buf.len().min(self.input.len()).min(7);
                if let Some(budget) = &mut self.read_budget {
                    if *budget == 0 {
                        return Poll::Pending;
                    }
                    n = n.min(*budget);
                    *budget -= n;
                }
                buf[..n].copy_from_slice(&self.input.split_to(n));
                stats.bytes_read += n;
                Poll::Ready(Ok(n))
            })
        }
    }

    impl AsyncWrite for Mock {
        fn async_write(&mut self, buf: &[u8]) -> impl Future<Output = io::Result<usize>> + Send {
            poll_fn(move |_| {
                let mut stats = self.stats.lock().unwrap();
                stats.writes += 1;
                if self.write_error {
                    return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
                }
                let mut n = buf.len().min(7);
                if let Some(budget) = &mut self.write_budget {
                    if *budget == 0 {
                        return Poll::Pending;
                    }
                    n = n.min(*budget);
                    *budget -= n;
                }
                self.output.extend_from_slice(&buf[..n]);
                stats.bytes_written += n;
                Poll::Ready(Ok(n))
            })
        }
    }

    fn accepted(reply_data: accept_stat_data) -> msg_body<'static, 'static> {
        msg_body::REPLY(reply_body::MSG_ACCEPTED(accepted_reply {
            verf: opaque_auth::default(),
            reply_data,
        }))
    }

    fn encode_record(xid: u32, body: msg_body<'_, '_>, result: &impl Pack) -> Encoded {
        let mut out = Encoder::new(0);
        rpc_msg { xid, body }.pack(&mut out).unwrap();
        result.pack(&mut out).unwrap();
        out.finish_record().unwrap()
    }

    fn wire_record(xid: u32, body: msg_body<'_, '_>, result: &impl Pack) -> Bytes {
        let mut record = encode_record(xid, body, result);
        record.copy_to_bytes(record.remaining())
    }

    fn set_reply(client: &mut RpcClient<Mock>, body: msg_body<'_, '_>, result: &impl Pack) {
        client.io.as_mut().unwrap().input = wire_record(client.xid, body, result);
    }

    fn reply(xid: u32, result: &Opaque<'_>, fragments: &[usize]) -> Bytes {
        let mut record = encode_record(xid, accepted(accept_stat_data::SUCCESS), result);
        // Drop the single-fragment marker; each fragment gets its own below.
        record.advance(4);
        let record = record.copy_to_bytes(record.remaining());

        let mut wire = BytesMut::new();
        let mut rest = &*record;
        for (i, &len) in fragments.iter().enumerate() {
            let last = i + 1 == fragments.len();
            let len = if last { rest.len() } else { len };
            let header = fragment_header::new(len, last).unwrap();
            wire.extend_from_slice(&header.into_xdr_buf());
            wire.extend_from_slice(&rest[..len]);
            rest = &rest[len..];
        }
        wire.freeze()
    }

    fn assert_unusable_without_io(client: &mut RpcClient<Mock>) {
        assert!(client.io.is_none());
        let error = client
            .call::<_, Void>(1, 1, 0, &Void)
            .unwrap_ready()
            .unwrap_err();
        assert!(matches!(error, RpcError::ConnectionUnusable));
        assert!(!error.is_connection_reusable());
    }

    fn cancel_pending(future: impl Future) {
        assert!(
            pin!(future)
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
    }

    #[test]
    fn dropped_partial_write_drops_transport() {
        let mut io = Mock::default();
        io.write_budget = Some(5);
        let stats = Arc::clone(&io.stats);
        let mut client = RpcClient::new(io);
        cancel_pending(client.call::<_, Void>(1, 1, 0, &Void));
        let after_cancel = *stats.lock().unwrap();
        assert!(after_cancel.dropped);
        assert_eq!(after_cancel.bytes_written, 5);
        assert_eq!(after_cancel.reads, 0);
        assert_unusable_without_io(&mut client);
        assert_eq!(*stats.lock().unwrap(), after_cancel);
    }

    #[test]
    fn dropped_partial_read_drops_transport() {
        // Interrupt once in the record marker and once in the reply body.
        for read_budget in [2, 9] {
            let mut io = Mock::default();
            io.read_budget = Some(read_budget);
            let stats = Arc::clone(&io.stats);
            let mut client = RpcClient::new(io);
            set_reply(&mut client, accepted(accept_stat_data::SUCCESS), &Void);
            cancel_pending(client.call::<_, Void>(1, 1, 0, &Void));
            let after_cancel = *stats.lock().unwrap();
            assert!(after_cancel.dropped);
            assert_eq!(after_cancel.bytes_read, read_budget);
            assert_unusable_without_io(&mut client);
            assert_eq!(*stats.lock().unwrap(), after_cancel);
        }
    }

    #[test]
    fn unpolled_call_and_local_encoding_errors_preserve_connection() {
        struct InvalidArgs(usize);

        impl Pack for InvalidArgs {
            fn packed_size(&self) -> usize {
                self.0
            }

            fn pack(&self, _: &mut Encoder) -> xdr_codec::Result<()> {
                Err(xdr_codec::Error::ObjectTooLarge(self.0))
            }
        }

        let mut client = RpcClient::new(Mock::default());
        drop(client.call::<_, Void>(1, 1, 0, &Void));
        let error = client
            .call::<_, Void>(1, 1, 0, &InvalidArgs(1))
            .unwrap_ready()
            .unwrap_err();
        assert!(matches!(error, RpcError::WrongLength));
        assert!(error.is_connection_reusable());
        let error = client
            .call::<_, Void>(1, 1, 0, &InvalidArgs(4))
            .unwrap_ready()
            .unwrap_err();
        assert!(matches!(error, RpcError::XdrEncode(_)));
        assert!(error.is_connection_reusable());
        let io = client.io.as_ref().unwrap();
        assert_eq!(*io.stats.lock().unwrap(), IoStats::default());

        set_reply(&mut client, accepted(accept_stat_data::SUCCESS), &Void);
        client
            .call::<_, Void>(1, 1, 0, &Void)
            .unwrap_ready()
            .unwrap();
    }

    #[test]
    fn valid_error_replies_preserve_connection() {
        let mut client = RpcClient::new(Mock::default());
        set_reply(&mut client, accepted(accept_stat_data::PROC_UNAVAIL), &Void);
        let error = client
            .call::<_, Void>(1, 1, 0, &Void)
            .unwrap_ready()
            .unwrap_err();
        assert!(matches!(error, RpcError::ProcUnavail));
        assert!(error.is_connection_reusable());

        let denied = msg_body::REPLY(reply_body::MSG_DENIED(rejected_reply::RPC_MISMATCH {
            low: 2,
            high: 2,
        }));
        set_reply(&mut client, denied, &Void);
        let error = client
            .call::<_, Void>(1, 1, 0, &Void)
            .unwrap_ready()
            .unwrap_err();
        assert!(matches!(error, RpcError::RpcMismatch { low: 2, high: 2 }));
        assert!(error.is_connection_reusable());

        let result = GETATTR3res::Err((nfsstat3::NFS3ERR_NOENT, Void));
        set_reply(&mut client, accepted(accept_stat_data::SUCCESS), &result);
        let result: GETATTR3res = client.call(1, 1, 0, &Void).unwrap_ready().unwrap();
        assert!(matches!(
            result,
            GETATTR3res::Err((nfsstat3::NFS3ERR_NOENT, Void))
        ));

        set_reply(&mut client, accepted(accept_stat_data::SUCCESS), &Void);
        client
            .call::<_, Void>(1, 1, 0, &Void)
            .unwrap_ready()
            .unwrap();
    }

    #[test]
    fn trailing_reply_data_prevents_further_io_even_on_rpc_rejection() {
        for body in [
            accepted(accept_stat_data::SUCCESS),
            accepted(accept_stat_data::PROC_UNAVAIL),
            msg_body::REPLY(reply_body::MSG_DENIED(rejected_reply::RPC_MISMATCH {
                low: 2,
                high: 2,
            })),
        ] {
            let mut client = RpcClient::new(Mock::default());
            set_reply(&mut client, body, &42u32);
            let error = client
                .call::<_, Void>(1, 1, 0, &Void)
                .unwrap_ready()
                .unwrap_err();
            assert!(
                matches!(error, RpcError::NotFullyParsed { ref remaining } if remaining.len() == 4)
            );
            assert!(!error.is_connection_reusable());
            assert_unusable_without_io(&mut client);
        }
    }

    #[test]
    fn invalid_or_mismatched_reply_prevents_further_io() {
        let mut client = RpcClient::new(Mock::default());
        set_reply(&mut client, accepted(accept_stat_data::SUCCESS), &Void);
        let stats = Arc::clone(&client.io.as_ref().unwrap().stats);
        let error = client
            .call::<_, u32>(1, 1, 0, &Void)
            .unwrap_ready()
            .unwrap_err();
        assert!(matches!(
            error,
            RpcError::Xdr(xdr_codec::Error::UnexpectedEof)
        ));
        assert!(!error.is_connection_reusable());
        assert!(stats.lock().unwrap().dropped);
        assert_unusable_without_io(&mut client);

        let mut client = RpcClient::new(Mock::default());
        client.io.as_mut().unwrap().input = wire_record(
            client.xid.wrapping_add(1),
            accepted(accept_stat_data::PROC_UNAVAIL),
            &Void,
        );
        let error = client
            .call::<_, Void>(1, 1, 0, &Void)
            .unwrap_ready()
            .unwrap_err();
        assert!(matches!(error, RpcError::UnexpectedXid));
        assert!(!error.is_connection_reusable());
        assert_unusable_without_io(&mut client);

        let mut client = RpcClient::new(Mock::default());
        set_reply(
            &mut client,
            msg_body::CALL(call_body {
                rpcvers: 2,
                prog: 1,
                vers: 1,
                proc: 0,
                cred: opaque_auth::default(),
                verf: opaque_auth::default(),
            }),
            &Void,
        );
        let error = client
            .call::<_, Void>(1, 1, 0, &Void)
            .unwrap_ready()
            .unwrap_err();
        assert!(matches!(error, RpcError::UnexpectedCall));
        assert!(!error.is_connection_reusable());
        assert_unusable_without_io(&mut client);
    }

    #[test]
    fn write_failure_prevents_further_io() {
        let mut io = Mock::default();
        io.write_error = true;
        let stats = Arc::clone(&io.stats);
        let mut client = RpcClient::new(io);
        let error = client
            .call::<_, Void>(1, 1, 0, &Void)
            .unwrap_ready()
            .unwrap_err();
        assert!(matches!(error, RpcError::Io(ref e) if e.kind() == io::ErrorKind::BrokenPipe));
        assert!(!error.is_connection_reusable());
        assert!(stats.lock().unwrap().dropped);
        assert_unusable_without_io(&mut client);
    }

    #[test]
    fn fragmented_reply_is_reassembled() {
        let payload = Opaque::owned((0..=255u8).cycle().take(5000).collect());
        let mut client = RpcClient::new(Mock::default());
        client.io.as_mut().unwrap().input = reply(client.xid, &payload, &[0, 13, 2000, 0]);

        let result: Opaque<'static> = client.call(1, 1, 0, &payload).unwrap_ready().unwrap();
        assert_eq!(result, payload);
        let io = client.io.as_ref().unwrap();
        assert!(io.input.is_empty());

        // The call, with its payload in a separate chunk, was written completely.
        let output = &io.output;
        let marker = u32::from_be_bytes(output[..4].try_into().unwrap());
        assert_eq!(marker as usize, 0x8000_0000 | (output.len() - 4));
        assert!(output.ends_with(&payload[payload.len() - 100..]));
    }

    #[test]
    fn truncated_reply_is_an_io_error() {
        let mut client = RpcClient::new(Mock::default());
        let wire = reply(client.xid, &Opaque::borrowed(b"abc"), &[0]);
        client.io.as_mut().unwrap().input = wire.slice(..wire.len() - 1);
        let stats = Arc::clone(&client.io.as_ref().unwrap().stats);

        let err = client
            .call::<_, Opaque<'static>>(1, 1, 0, &Void)
            .unwrap_ready()
            .unwrap_err();
        assert!(matches!(err, RpcError::Io(ref e) if e.kind() == io::ErrorKind::UnexpectedEof));
        assert!(!err.is_connection_reusable());
        assert!(stats.lock().unwrap().dropped);
        assert_unusable_without_io(&mut client);
    }
}
