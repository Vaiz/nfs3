use std::collections::VecDeque;
use std::future::poll_fn;
use std::io;
use std::pin::pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use bytes::{Buf, Bytes};
use future_assert::FutureTestExt;
use nfs3_client::RpcError;
use nfs3_client::io::{AsyncRead, AsyncWrite};
use nfs3_client::rpc::RpcClient;
use nfs3_types::nfs3::{GETATTR3res, nfsstat3};
use nfs3_types::rpc::{
    accept_stat_data, accepted_reply, fragment_header, msg_body, opaque_auth, rejected_reply,
    reply_body, rpc_msg,
};
use nfs3_types::xdr_codec::{self, Encoder, Pack, Void};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Progress {
    read: usize,
    written: usize,
    dropped: bool,
}

type Reply = Box<dyn FnOnce(u32) -> Bytes + Send>;

#[derive(Default)]
struct Transport {
    progress: Arc<Mutex<Progress>>,
    read_limit: Option<usize>,
    write_limit: Option<usize>,
    replies: VecDeque<Reply>,
    input: Bytes,
    request: Vec<u8>,
}

impl Drop for Transport {
    fn drop(&mut self) {
        self.progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .dropped = true;
    }
}

impl AsyncWrite for Transport {
    fn async_write(&mut self, buf: &[u8]) -> impl Future<Output = io::Result<usize>> + Send {
        poll_fn(move |_| {
            let mut progress = self.progress.lock().expect("lock write progress");
            let n = buf.len().min(7).min(
                self.write_limit
                    .map_or(usize::MAX, |limit| limit - progress.written),
            );
            if n == 0 {
                return Poll::Pending;
            }
            self.request.extend_from_slice(&buf[..n]);
            progress.written += n;
            Poll::Ready(Ok(n))
        })
    }
}

impl AsyncRead for Transport {
    fn async_read(&mut self, buf: &mut [u8]) -> impl Future<Output = io::Result<usize>> + Send {
        poll_fn(move |_| {
            let mut progress = self.progress.lock().expect("lock read progress");
            let budget = self
                .read_limit
                .map_or(usize::MAX, |limit| limit - progress.read);
            if budget == 0 {
                return Poll::Pending;
            }
            if self.input.is_empty()
                && !self.request.is_empty()
                && let Some(reply) = self.replies.pop_front()
            {
                let marker =
                    u32::from_be_bytes(self.request[..4].try_into().expect("record marker"));
                assert_eq!(marker as usize, 0x8000_0000 | (self.request.len() - 4));
                let xid = u32::from_be_bytes(self.request[4..8].try_into().expect("request XID"));
                self.input = reply(xid);
                self.request.clear();
            }
            let n = buf.len().min(self.input.len()).min(7).min(budget);
            buf[..n].copy_from_slice(&self.input.split_to(n));
            progress.read += n;
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

const fn denied() -> msg_body<'static, 'static> {
    msg_body::REPLY(reply_body::MSG_DENIED(rejected_reply::RPC_MISMATCH {
        low: 2,
        high: 2,
    }))
}

fn record(xid: u32, body: msg_body<'_, '_>, result: &impl Pack) -> Bytes {
    let mut enc = Encoder::new(0);
    rpc_msg { xid, body }
        .pack(&mut enc)
        .expect("encode reply header");
    result.pack(&mut enc).expect("encode reply result");
    let mut wire = enc.finish_record().expect("finish reply record");
    wire.copy_to_bytes(wire.remaining())
}

fn success(xid: u32) -> Bytes {
    record(xid, accepted(accept_stat_data::SUCCESS), &Void)
}

fn assert_unusable(client: &mut RpcClient<Transport>, progress: &Mutex<Progress>) {
    let before = *progress.lock().expect("read transport progress");
    assert!(before.dropped);
    let err = client
        .call::<_, Void>(1, 1, 0, &Void)
        .unwrap_ready()
        .expect_err("discarded transport must not be reused");
    assert!(matches!(err, RpcError::ConnectionUnusable));
    assert!(!err.is_connection_reusable());
    assert_eq!(*progress.lock().expect("read transport progress"), before);
}

fn cancel_pending(call: impl Future) {
    assert!(
        pin!(call)
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
}

#[test]
fn excessive_reply_fragments_drop_transport() {
    let mut transport = Transport::default();
    transport.replies.push_back(Box::new(|xid| {
        let mut reply = vec![0; 256 * 4];
        reply.extend_from_slice(&success(xid));
        reply.into()
    }));
    let progress = Arc::clone(&transport.progress);
    let mut client = RpcClient::new(transport);
    let err = client
        .call::<_, Void>(1, 1, 0, &Void)
        .unwrap_ready()
        .expect_err("reply exceeds the fragment count limit");
    let RpcError::Io(err) = err else {
        panic!("expected a fragment-limit I/O error, got {err:?}");
    };
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert_eq!(
        err.to_string(),
        "RPC record exceeds the fragment count limit"
    );
    assert_eq!(
        progress.lock().expect("read transport progress").read,
        257 * 4
    );
    assert_unusable(&mut client, &progress);
}

#[test]
fn cancelling_partial_io_drops_transport() {
    for (write_limit, read_limit) in [(Some(5), None), (None, Some(2)), (None, Some(9))] {
        let mut transport = Transport::default();
        transport.write_limit = write_limit;
        transport.read_limit = read_limit;
        transport.replies.push_back(Box::new(success));
        let progress = Arc::clone(&transport.progress);
        let mut client = RpcClient::new(transport);
        cancel_pending(client.call::<_, Void>(1, 1, 0, &Void));
        let after = *progress.lock().expect("read partial I/O progress");
        assert_eq!(after.read, read_limit.unwrap_or(0));
        if let Some(n) = write_limit {
            assert_eq!(after.written, n);
        }
        assert_unusable(&mut client, &progress);
    }
}

#[test]
fn unpolled_call_and_local_encoding_failures_preserve_transport() {
    struct InvalidArgs(usize);

    impl Pack for InvalidArgs {
        fn packed_size(&self) -> usize {
            self.0
        }

        fn pack(&self, _: &mut Encoder) -> xdr_codec::Result<()> {
            Err(xdr_codec::Error::ObjectTooLarge(self.0))
        }
    }

    let mut transport = Transport::default();
    transport.replies.push_back(Box::new(success));
    let progress = Arc::clone(&transport.progress);
    let mut client = RpcClient::new(transport);
    drop(client.call::<_, Void>(1, 1, 0, &Void));
    let err = client
        .call::<_, Void>(1, 1, 0, &InvalidArgs(1))
        .unwrap_ready()
        .expect_err("unaligned size must fail");
    assert!(matches!(err, RpcError::WrongLength));
    assert!(err.is_connection_reusable());
    let err = client
        .call::<_, Void>(1, 1, 0, &InvalidArgs(4))
        .unwrap_ready()
        .expect_err("arguments must fail to pack");
    assert!(matches!(err, RpcError::XdrEncode(_)));
    assert!(err.is_connection_reusable());
    assert_eq!(
        *progress.lock().expect("read transport progress"),
        Progress::default()
    );
    client
        .call::<_, Void>(1, 1, 0, &Void)
        .unwrap_ready()
        .expect("local failures leave the connection reusable");
}

#[test]
fn complete_matching_error_replies_preserve_transport() {
    let replies: VecDeque<Reply> = VecDeque::from([
        Box::new(|xid| record(xid, accepted(accept_stat_data::PROC_UNAVAIL), &Void)) as Reply,
        Box::new(|xid| record(xid, denied(), &Void)),
        Box::new(|xid| {
            let nfs_error = GETATTR3res::Err((nfsstat3::NFS3ERR_NOENT, Void));
            record(xid, accepted(accept_stat_data::SUCCESS), &nfs_error)
        }),
        Box::new(|xid| {
            // Split the RPC header across fragments, then deliver short reads.
            let wire = success(xid);
            let (head, tail) = wire[4..].split_at(7);
            let mut reply = Vec::new();
            for (fragment, last) in [(head, false), (tail, true)] {
                reply.extend(
                    fragment_header::new(fragment.len(), last)
                        .expect("valid fragment length")
                        .into_xdr_buf(),
                );
                reply.extend_from_slice(fragment);
            }
            reply.into()
        }),
    ]);
    let mut transport = Transport::default();
    transport.replies.extend(replies);
    let mut client = RpcClient::new(transport);

    let err = client
        .call::<_, Void>(1, 1, 0, &Void)
        .unwrap_ready()
        .expect_err("accepted RPC rejection");
    assert!(matches!(err, RpcError::ProcUnavail));
    assert!(err.is_connection_reusable());
    let err = client
        .call::<_, Void>(1, 1, 0, &Void)
        .unwrap_ready()
        .expect_err("denied RPC reply");
    assert!(matches!(err, RpcError::RpcMismatch { low: 2, high: 2 }));
    assert!(err.is_connection_reusable());
    let result: GETATTR3res = client
        .call(1, 1, 0, &Void)
        .unwrap_ready()
        .expect("decode NFS error");
    assert!(matches!(
        result,
        GETATTR3res::Err((nfsstat3::NFS3ERR_NOENT, Void))
    ));
    client
        .call::<_, Void>(1, 1, 0, &Void)
        .unwrap_ready()
        .expect("reuse after complete errors and reassemble fragmented reply");
}

#[test]
fn invalid_replies_drop_transport() {
    type Case = (Reply, fn(RpcError) -> bool);
    let cases: [Case; 5] = [
        (
            Box::new(|xid| {
                record(
                    xid.wrapping_add(1),
                    accepted(accept_stat_data::PROC_UNAVAIL),
                    &Void,
                )
            }),
            |err| matches!(err, RpcError::UnexpectedXid),
        ),
        (
            Box::new(|xid| record(xid, accepted(accept_stat_data::PROC_UNAVAIL), &42u32)),
            |err| matches!(err, RpcError::NotFullyParsed { .. }),
        ),
        (Box::new(|xid| record(xid, denied(), &42u32)), |err| {
            matches!(err, RpcError::NotFullyParsed { .. })
        }),
        (
            Box::new(|xid| success(xid).slice(..8)),
            |err| matches!(err, RpcError::Io(e) if e.kind() == io::ErrorKind::UnexpectedEof),
        ),
        (
            Box::new(|xid| {
                let mut wire = success(xid).to_vec();
                wire[8..12].copy_from_slice(&42u32.to_be_bytes());
                wire.into()
            }),
            |err| {
                matches!(
                    err,
                    RpcError::XdrDecode(xdr_codec::Error::InvalidEnumValue(42))
                )
            },
        ),
    ];
    for (reply, expected) in cases {
        let mut transport = Transport::default();
        transport.replies.push_back(reply);
        let progress = Arc::clone(&transport.progress);
        let mut client = RpcClient::new(transport);
        let err = client
            .call::<_, Void>(1, 1, 0, &Void)
            .unwrap_ready()
            .expect_err("invalid reply must fail the call");
        assert!(!err.is_connection_reusable());
        assert!(expected(err));
        assert_unusable(&mut client, &progress);
    }
}
