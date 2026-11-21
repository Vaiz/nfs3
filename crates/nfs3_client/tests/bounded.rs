#![expect(
    clippy::unwrap_used,
    reason = "Test fixture helpers abort on setup and RPC failures"
)]

use std::collections::VecDeque;
use std::io::{ErrorKind, IoSlice};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use nfs3_client::Nfs3Client;
use nfs3_client::io::{AsyncRead, AsyncWrite};
use nfs3_client::rpc::{RpcClient, RpcErrorKind, RpcLimits};
use nfs3_types::async_io::RecordLimits;
use nfs3_types::rpc::opaque_auth;
use nfs3_types::xdr_codec::{DecodeLimits, EncodeLimits, Payload, PayloadLimits, Void};

const fn limits() -> RpcLimits {
    RpcLimits {
        record: RecordLimits {
            block_bytes: 8,
            max_record_bytes: 2048,
            max_fragments: 4096,
        },
        backing_bytes: 2048,
        decode: DecodeLimits {
            max_metadata_bytes: 2048,
            max_collection_elements: 1024,
            max_depth: 32,
            max_reservation_bytes: 65536,
            payload: PayloadLimits {
                max_logical_bytes: 1024,
                max_backing_capacity: 2048,
                max_segments: 256,
            },
        },
        encode: EncodeLimits {
            max_wire_bytes: 2048,
            max_metadata_bytes: 2048,
            max_chunks: 512,
            max_retained_capacity: 4096,
            max_depth: 32,
            max_reservation_bytes: 65536,
        },
    }
}

#[derive(Default)]
struct State {
    sent: Vec<u8>,
    writes: usize,
    reads: usize,
    read_sizes: Vec<usize>,
    read_pointers: Vec<(usize, usize)>,
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "independent scripted IO fault switches"
)]
struct Script {
    state: Arc<Mutex<State>>,
    replies: VecDeque<Vec<u8>>,
    input: VecDeque<u8>,
    read_step: usize,
    write_step: usize,
    interrupted: bool,
    zero_write: bool,
    pending_send: bool,
    stop_write_after: Option<usize>,
    pending_receive: bool,
    fragment_bytes: Option<usize>,
    empty_fragments: usize,
    final_empty: bool,
    pending_after: Option<usize>,
    bytes_read: usize,
    fixed_xid: bool,
}

impl Script {
    fn new(replies: Vec<Vec<u8>>) -> Self {
        Self {
            state: Arc::default(),
            replies: replies.into(),
            input: VecDeque::new(),
            read_step: usize::MAX,
            write_step: usize::MAX,
            interrupted: false,
            zero_write: false,
            pending_send: false,
            stop_write_after: None,
            pending_receive: false,
            fragment_bytes: None,
            empty_fragments: 0,
            final_empty: false,
            pending_after: None,
            bytes_read: 0,
            fixed_xid: false,
        }
    }
}

impl AsyncRead for Script {
    async fn async_read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pending_receive {
            std::future::pending::<()>().await;
        }
        if self.pending_after.is_some_and(|n| self.bytes_read >= n) {
            std::future::pending::<()>().await;
        }
        if self.interrupted {
            self.interrupted = false;
            return Err(ErrorKind::Interrupted.into());
        }
        let mut state = self.state.lock().unwrap();
        state.reads += 1;
        state.read_sizes.push(buf.len());
        state.read_pointers.push((buf.as_ptr() as usize, buf.len()));
        if self.input.is_empty() {
            if let Some(mut reply) = self.replies.pop_front() {
                // Fixture XID is the only dynamic field; all other wire bytes
                // below are independent hand-authored XDR.
                let mut start = 0;
                let mut last = 0;
                while start < state.sent.len() {
                    last = start;
                    let len = u32::from_be_bytes(state.sent[start..start + 4].try_into().unwrap())
                        & 0x7fff_ffff;
                    start += 4 + len as usize;
                }
                let xid = &state.sent[last + 4..last + 8];
                // Request body is 40 bytes for the Void calls in these tests.
                if reply.len() >= 8 && !self.fixed_xid {
                    reply[4..8].copy_from_slice(xid);
                }
                if let Some(size) = self.fragment_bytes {
                    let body = reply[4..].to_vec();
                    reply.clear();
                    for _ in 0..self.empty_fragments {
                        reply.extend_from_slice(&[0; 4]);
                    }
                    for (index, chunk) in body.chunks(size).enumerate() {
                        let final_flag = if !self.final_empty && (index + 1) * size >= body.len() {
                            0x8000_0000
                        } else {
                            0
                        };
                        reply.extend_from_slice(
                            &(u32::try_from(chunk.len()).unwrap() | final_flag).to_be_bytes(),
                        );
                        reply.extend_from_slice(chunk);
                    }
                    if self.final_empty {
                        reply.extend_from_slice(&[0x80, 0, 0, 0]);
                    }
                }
                self.input.extend(reply);
            }
        }
        let n = buf.len().min(self.read_step).min(self.input.len());
        for byte in &mut buf[..n] {
            *byte = self.input.pop_front().unwrap();
        }
        self.bytes_read += n;
        Ok(n)
    }
}

impl AsyncWrite for Script {
    async fn async_write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.pending_send {
            std::future::pending::<()>().await;
        }
        if self
            .stop_write_after
            .is_some_and(|stop| self.state.lock().unwrap().sent.len() >= stop)
        {
            std::future::pending::<()>().await;
        }
        if self.zero_write {
            return Ok(0);
        }
        let n = buf.len().min(self.write_step);
        let mut state = self.state.lock().unwrap();
        state.writes += 1;
        state.sent.extend_from_slice(&buf[..n]);
        Ok(n)
    }
}

fn success(body: &[u8]) -> Vec<u8> {
    let mut reply = (0x8000_0000 | u32::try_from(24 + body.len()).unwrap())
        .to_be_bytes()
        .to_vec();
    reply.extend_from_slice(&[
        0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]);
    reply.extend_from_slice(body);
    reply
}

fn client(io: Script) -> RpcClient<Script> {
    RpcClient::new(io, limits(), opaque_auth::default(), opaque_auth::default()).unwrap()
}

#[tokio::test]
async fn every_short_split_and_fallback_small_request() {
    for step in 1..=32 {
        let mut io = Script::new(vec![success(&[0, 0, 0, 3, b'a', b'b', b'c', 0])]);
        io.read_step = step;
        io.write_step = step;
        io.interrupted = true;
        let state = Arc::clone(&io.state);
        let mut rpc = client(io);
        let value: Payload = rpc.call(1, 2, 3, &Void).await.unwrap();
        assert_eq!(value.try_to_vec().unwrap(), b"abc");
        assert!(rpc.is_usable());
        drop(value);
        assert_eq!(rpc.backing_used(), 0);
        assert_eq!(state.lock().unwrap().sent.len(), 44);
    }
    let io = Script::new(vec![success(&[])]);
    let state = Arc::clone(&io.state);
    let mut rpc = client(io);
    rpc.call::<_, Void>(1, 2, 3, &Void).await.unwrap();
    assert_eq!(state.lock().unwrap().writes, 1);
}

#[tokio::test]
async fn retained_output_admission_then_success_and_client_drop() {
    let io = Script::new(vec![
        success(&[0, 0, 0, 3, b'a', b'b', b'c', 0]),
        success(&[]),
    ]);
    let state = Arc::clone(&io.state);
    let mut rpc = client(io);
    let value: Payload = rpc.call(1, 2, 3, &Void).await.unwrap();
    let writes = state.lock().unwrap().writes;
    let error = rpc.call::<_, Void>(1, 2, 3, &Void).await.unwrap_err();
    assert!(matches!(error.kind, RpcErrorKind::Admission));
    assert!(!error.io_started && error.connection_usable);
    assert_eq!(state.lock().unwrap().writes, writes);
    drop(value);
    rpc.call::<_, Void>(1, 2, 3, &Void).await.unwrap();
    let io = Script::new(vec![success(&[0, 0, 0, 3, b'a', b'b', b'c', 0])]);
    let mut rpc = client(io);
    let value: Payload = rpc.call(1, 2, 3, &Void).await.unwrap();
    drop(rpc);
    assert_eq!(value.try_to_vec().unwrap(), b"abc");
}

#[test]
fn cancellation_unpolled_and_started() {
    for send in [false, true] {
        let mut io = Script::new(vec![]);
        io.pending_send = send;
        io.pending_receive = !send;
        let state = Arc::clone(&io.state);
        let mut rpc = client(io);
        drop(rpc.call::<_, Void>(1, 2, 3, &Void));
        assert!(rpc.is_usable());
        let mut future = Box::pin(rpc.call::<_, Void>(1, 2, 3, &Void));
        let waker = Waker::noop();
        assert!(matches!(
            future.as_mut().poll(&mut Context::from_waker(waker)),
            Poll::Pending
        ));
        drop(future);
        assert!(!rpc.is_usable());
        assert_eq!(rpc.backing_used(), 0);
        let before = state.lock().unwrap().writes;
        let mut refused = Box::pin(rpc.call::<_, Void>(1, 2, 3, &Void));
        let Poll::Ready(Err(error)) = refused.as_mut().poll(&mut Context::from_waker(waker)) else {
            panic!()
        };
        assert!(!error.io_started && !error.connection_usable);
        assert_eq!(state.lock().unwrap().writes, before);
    }
}

#[test]
fn cancellation_after_partial_send_refuses_next_call_before_io() {
    for stop in [1, 2, 3, 4, 20, 43] {
        let mut io = Script::new(vec![]);
        io.write_step = 1;
        io.stop_write_after = Some(stop);
        let state = Arc::clone(&io.state);
        let mut rpc = client(io);
        let mut future = Box::pin(rpc.call::<_, Void>(1, 2, 3, &Void));
        let waker = Waker::noop();
        assert!(
            future
                .as_mut()
                .poll(&mut Context::from_waker(waker))
                .is_pending()
        );
        drop(future);
        assert_eq!(rpc.backing_used(), 0);
        assert_eq!(state.lock().unwrap().sent.len(), stop);
        let mut next = Box::pin(rpc.call::<_, Void>(1, 2, 3, &Void));
        let Poll::Ready(Err(error)) = next.as_mut().poll(&mut Context::from_waker(waker)) else {
            panic!()
        };
        assert!(!error.io_started && !error.connection_usable);
        assert_eq!(state.lock().unwrap().sent.len(), stop);
    }
}

#[tokio::test]
async fn malformed_and_zero_progress_poison() {
    let mut trailing = success(&[0, 0, 0, 0]);
    trailing[3] = 28;
    for reply in [
        trailing,
        success(&[0, 0, 0, 1, b'x', 1, 0, 0]),
        vec![0x80, 0, 0, 24, 0],
    ] {
        let mut rpc = client(Script::new(vec![reply]));
        let error = rpc.call::<_, Void>(1, 2, 3, &Void).await.unwrap_err();
        assert!(error.io_started && !error.connection_usable);
        assert_eq!(rpc.backing_used(), 0);
    }
    let mut io = Script::new(vec![]);
    io.zero_write = true;
    let error = client(io)
        .call::<_, Void>(1, 2, 3, &Void)
        .await
        .unwrap_err();
    assert!(matches!(error.kind, RpcErrorKind::Io(ref e) if e.kind() == ErrorKind::WriteZero));
}

// Keep a real vectored custom transport to test advancement across chunk edges.
struct Vectored(Script);
impl AsyncRead for Vectored {
    async fn async_read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.async_read(buf).await
    }
}
impl AsyncWrite for Vectored {
    async fn async_write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.async_write(buf).await
    }
    async fn async_write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> std::io::Result<usize> {
        if self.0.interrupted {
            self.0.interrupted = false;
            return Err(ErrorKind::Interrupted.into());
        }
        let mut remaining = self.0.write_step;
        let mut state = self.0.state.lock().unwrap();
        state.writes += 1;
        for buf in bufs {
            let n = buf.len().min(remaining);
            state.sent.extend_from_slice(&buf[..n]);
            remaining -= n;
        }
        Ok(self.0.write_step - remaining)
    }
}

#[tokio::test]
async fn fragments_do_not_create_blocks_and_empty_fragments_count() {
    for size in 1..=33 {
        let mut io = Script::new(vec![success(&[0, 0, 0, 3, b'a', b'b', b'c', 0])]);
        io.fragment_bytes = Some(size);
        io.empty_fragments = 3;
        io.final_empty = true;
        io.read_step = 1;
        let state = Arc::clone(&io.state);
        let mut rpc = client(io);
        let data: Payload = rpc.call(1, 2, 3, &Void).await.unwrap();
        assert_eq!(data.try_to_vec().unwrap(), b"abc");
        assert_eq!(data.segment_count(), 1);
        assert_eq!(data.retained_capacity(), 8);
        assert_eq!(rpc.backing_used(), 8);
        let pointer = data.as_contiguous().unwrap().as_ptr() as usize;
        assert!(
            state
                .lock()
                .unwrap()
                .read_pointers
                .iter()
                .any(|&(p, n)| pointer >= p && pointer < p + n)
        );
    }
    let mut io = Script::new(vec![success(&[])]);
    io.fragment_bytes = Some(1);
    io.empty_fragments = 4096;
    let error = client(io)
        .call::<_, Void>(1, 2, 3, &Void)
        .await
        .unwrap_err();
    assert!(matches!(error.kind, RpcErrorKind::Framing));
}

#[test]
fn cancellation_after_partial_marker_and_body_releases_reservation() {
    for stop in [1, 2, 3, 4, 5, 12, 17, 28, 30] {
        let mut io = Script::new(vec![success(&[0, 0, 0, 3, b'a', b'b', b'c', 0])]);
        io.read_step = 1;
        io.pending_after = Some(stop);
        let mut rpc = client(io);
        let mut future = Box::pin(rpc.call::<_, Payload>(1, 2, 3, &Void));
        let waker = Waker::noop();
        assert!(
            future
                .as_mut()
                .poll(&mut Context::from_waker(waker))
                .is_pending()
        );
        drop(future);
        assert!(!rpc.is_usable());
        assert_eq!(rpc.backing_used(), 0);
    }
}

#[tokio::test]
async fn preio_encode_errors_reuse_and_reply_header_errors_poison() {
    use nfs3_types::xdr_codec::Opaque;
    let io = Script::new(vec![success(&[])]);
    let state = Arc::clone(&io.state);
    let mut rpc = client(io);
    let too_large = Opaque::owned(vec![0; 4096]);
    let error = rpc.call::<_, Void>(1, 2, 3, &too_large).await.unwrap_err();
    assert!(!error.io_started && error.connection_usable);
    assert_eq!(state.lock().unwrap().writes, 0);
    rpc.call::<_, Void>(1, 2, 3, &Void).await.unwrap();

    for offset in [8, 12, 16, 20, 24] {
        let mut bad = success(&[]);
        bad[offset..offset + 4].copy_from_slice(&0xffff_ffff_u32.to_be_bytes());
        let mut rpc = client(Script::new(vec![bad]));
        assert!(
            !rpc.call::<_, Void>(1, 2, 3, &Void)
                .await
                .unwrap_err()
                .connection_usable
        );
    }
    let mut io = Script::new(vec![success(&[])]);
    io.fixed_xid = true;
    // Wrong XID independent of the randomized client seed (two different IDs).
    let mut rpc = client(io);
    let first = rpc.call::<_, Void>(1, 2, 3, &Void).await;
    if first.is_ok() {
        assert!(
            !rpc.call::<_, Void>(1, 2, 3, &Void)
                .await
                .unwrap_err()
                .connection_usable
        );
    } else {
        assert!(!rpc.is_usable());
    }
}

#[tokio::test]
async fn bounded_metadata_prefix_larger_than_stack_is_not_truncated() {
    use nfs3_types::xdr_codec::Opaque;
    let io = Script::new(vec![success(&[])]);
    let state = Arc::clone(&io.state);
    let mut rpc = client(io);
    rpc.call::<_, Void>(1, 2, 3, &Opaque::owned(vec![0x51; 1200]))
        .await
        .unwrap();
    let state = state.lock().unwrap();
    assert_eq!(state.writes, 2);
    assert_eq!(state.sent.len(), 1248);
    assert_eq!(&state.sent[44..48], &1200_u32.to_be_bytes());
    assert_eq!(&state.sent[48..], &[0x51; 1200]);
}

#[tokio::test]
async fn vectored_partial_send_is_byte_exact_without_bulk_copy() {
    use nfs3_types::xdr_codec::{Encode, Encoder};
    let payload_limits = limits().decode.payload;
    let payload =
        Payload::try_from_chunks((0..40).map(|i| vec![i; 5]).collect(), payload_limits).unwrap();
    for step in [1, 2, 3, 7, 43, 44, 45, 79, 128, 255, 4096] {
        let mut script = Script::new(vec![success(&[])]);
        script.write_step = step;
        script.interrupted = true;
        let state = Arc::clone(&script.state);
        let io = Vectored(script);
        let mut rpc =
            RpcClient::new(io, limits(), opaque_auth::default(), opaque_auth::default()).unwrap();
        rpc.call::<_, Void>(1, 2, 3, &payload).await.unwrap();
        let sent = &state.lock().unwrap().sent;
        let mut encoder = Encoder::new(limits().encode);
        payload.encode(&mut encoder).unwrap();
        let plan = encoder.finish().unwrap();
        let expected: Vec<u8> = plan.chunks().flatten().copied().collect();
        assert_eq!(&sent[44..], expected);
        assert_eq!(
            u32::from_be_bytes(sent[..4].try_into().unwrap()) & 0x7fff_ffff,
            u32::try_from(sent.len() - 4).unwrap()
        );
    }
}

#[tokio::test]
async fn complete_rpc_errors_reuse_but_trailing_or_policy_errors_poison() {
    // Independent MSG_DENIED / RPC_MISMATCH (supported versions 2..2).
    let denied: Vec<u8> = [0x8000_0018_u32, 0, 1, 1, 0, 2, 2]
        .into_iter()
        .flat_map(u32::to_be_bytes)
        .collect();
    let mut rpc = client(Script::new(vec![denied, success(&[])]));
    let error = rpc.call::<_, Void>(1, 2, 3, &Void).await.unwrap_err();
    assert!(matches!(error.kind, RpcErrorKind::Rejected(_)));
    assert!(error.io_started && error.connection_usable);
    assert_eq!(rpc.backing_used(), 0);
    rpc.call::<_, Void>(1, 2, 3, &Void).await.unwrap();

    let mut rejected = success(&[]);
    // MSG_ACCEPTED / PROG_UNAVAIL, then a valid second call.
    rejected[24..28].copy_from_slice(&[0, 0, 0, 1]);
    let mut rpc = client(Script::new(vec![rejected.clone(), success(&[])]));
    let error = rpc.call::<_, Void>(1, 2, 3, &Void).await.unwrap_err();
    assert!(error.io_started && error.connection_usable);
    rpc.call::<_, Void>(1, 2, 3, &Void).await.unwrap();
    rejected.extend_from_slice(&[0; 4]);
    rejected[3] = 28;
    let mut rpc = client(Script::new(vec![rejected]));
    assert!(
        !rpc.call::<_, Void>(1, 2, 3, &Void)
            .await
            .unwrap_err()
            .connection_usable
    );

    let io = Script::new(vec![success(&[0, 0, 0, 3, b'a', b'b', b'c', 0])]);
    let mut policy = limits();
    policy.decode.payload.max_logical_bytes = 2;
    let mut rpc =
        RpcClient::new(io, policy, opaque_auth::default(), opaque_auth::default()).unwrap();
    let error = rpc.call::<_, Payload>(1, 2, 3, &Void).await.unwrap_err();
    assert!(matches!(error.kind, RpcErrorKind::Decode(_)));
    assert!(error.io_started && !error.connection_usable);
}

#[tokio::test]
async fn truncated_marker_body_padding_and_excess_record() {
    for length in 0..32 {
        let mut reply = success(&[0, 0, 0, 3, b'a', b'b', b'c', 0]);
        reply.truncate(length);
        let mut rpc = client(Script::new(vec![reply]));
        rpc.call::<_, Payload>(1, 2, 3, &Void).await.unwrap_err();
        assert!(!rpc.is_usable());
        assert_eq!(rpc.backing_used(), 0);
    }
    for body in [
        vec![0, 0, 0, 1, b'x', 0, 1, 0],
        vec![0xff, 0xff, 0xff, 0xff],
    ] {
        let mut rpc = client(Script::new(vec![success(&body)]));
        assert!(
            !rpc.call::<_, Payload>(1, 2, 3, &Void)
                .await
                .unwrap_err()
                .connection_usable
        );
    }
    let mut rpc = client(Script::new(vec![vec![0xff, 0xff, 0xff, 0xff]]));
    assert!(matches!(
        rpc.call::<_, Void>(1, 2, 3, &Void).await.unwrap_err().kind,
        RpcErrorKind::Framing
    ));
}

fn nfs_read_reply(count: u32, data: &[u8]) -> Vec<u8> {
    // status OK, absent attributes, count, eof=true, opaque length, data + pad.
    let mut body = vec![0; 8];
    body.extend_from_slice(&count.to_be_bytes());
    body.extend_from_slice(&[0, 0, 0, 1]);
    body.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
    body.extend_from_slice(data);
    while !body.len().is_multiple_of(4) {
        body.push(0);
    }
    success(&body)
}

fn nfs_write_reply(count: u32) -> Vec<u8> {
    // OK, absent pre/post attrs, count, FILE_SYNC, 8-byte verifier.
    let mut body = vec![0; 12];
    body.extend_from_slice(&count.to_be_bytes());
    body.extend_from_slice(&[0, 0, 0, 2]);
    body.extend_from_slice(&[0x55; 8]);
    success(&body)
}

const fn handle() -> nfs3_types::nfs3::nfs_fh3 {
    nfs3_types::nfs3::nfs_fh3 {
        data: nfs3_types::xdr_codec::Opaque::borrowed(b"file"),
    }
}

fn maximum_verifier(body: &[u8]) -> Vec<u8> {
    let mut reply = success(body);
    reply.splice(24..24, std::iter::repeat_n(0x55, 400));
    reply[20..24].copy_from_slice(&400_u32.to_be_bytes());
    let len = u32::try_from(reply.len() - 4).unwrap();
    reply[..4].copy_from_slice(&(0x8000_0000 | len).to_be_bytes());
    reply
}

fn full_read_reply(data: &[u8]) -> Vec<u8> {
    let mut body = vec![0; 8]; // status, attribute tag
    body[7] = 1; // present attributes
    body.extend_from_slice(&[0, 0, 0, 1]); // NF3REG
    body.extend_from_slice(&[0; 80]); // remaining fattr3 fields
    body.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
    body.extend_from_slice(&[0, 0, 0, 1]);
    body.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
    body.extend_from_slice(data);
    while !body.len().is_multiple_of(4) {
        body.push(0);
    }
    maximum_verifier(&body)
}

fn full_write_reply(failure: bool) -> Vec<u8> {
    let mut body = if failure { 5_u32 } else { 0 }.to_be_bytes().to_vec();
    body.extend_from_slice(&1_u32.to_be_bytes()); // pre_op_attr present
    body.extend_from_slice(&[0; 24]); // size + mtime + ctime
    body.extend_from_slice(&1_u32.to_be_bytes()); // post_op_attr present
    body.extend_from_slice(&1_u32.to_be_bytes()); // NF3REG
    body.extend_from_slice(&[0; 80]); // remaining fattr3 fields
    if !failure {
        body.extend_from_slice(&0_u32.to_be_bytes()); // zero bytes written is valid
        body.extend_from_slice(&2_u32.to_be_bytes()); // FILE_SYNC
        body.extend_from_slice(&[0x55; 8]);
    }
    maximum_verifier(&body)
}

#[tokio::test]
async fn nfs_tight_reply_envelopes_and_preio_one_below() {
    use nfs3_types::nfs3::{Nfs3Option, Nfs3Result, READ3args, stable_how};
    for (bulk, record_bytes, segments) in [(0_u32, 560, 0), (1, 560, 2), (1001, 1532, 144)] {
        let data = vec![0x71; bulk as usize];
        let read = full_read_reply(&data);
        assert_eq!(read.len() - 4, 528 + data.len().div_ceil(4) * 4);
        let mut p = limits();
        p.record.block_bytes = 7; // payload starts at offset 528, three bytes into a block
        p.record.max_record_bytes = record_bytes;
        p.backing_bytes = record_bytes;
        p.encode.max_wire_bytes = record_bytes;
        p.decode.max_metadata_bytes = 400;
        p.decode.max_collection_elements = 0;
        p.decode.max_depth = 7;
        p.decode.max_reservation_bytes =
            400 + Payload::descriptor_reservation_cost(segments).unwrap();
        p.decode.payload = PayloadLimits {
            max_logical_bytes: bulk as usize,
            max_backing_capacity: segments * 7,
            max_segments: segments,
        };
        let io = Script::new(vec![read, full_write_reply(false), full_write_reply(true)]);
        let rpc = RpcClient::new(io, p, opaque_auth::default(), opaque_auth::default()).unwrap();
        let mut nfs = Nfs3Client::new(rpc, bulk).unwrap();
        let Nfs3Result::Ok(value) = nfs
            .read(&READ3args {
                file: handle(),
                offset: 0,
                count: bulk,
            })
            .await
            .unwrap()
        else {
            panic!()
        };
        assert!(matches!(value.file_attributes, Nfs3Option::Some(_)));
        assert_eq!(value.data.try_to_vec().unwrap(), data);
        // Count selected origins independently from the known wire offsets.
        let selected = if bulk == 0 {
            0
        } else {
            (527 + bulk as usize) / 7 - 528 / 7 + 1
        };
        assert_eq!(value.data.segment_count(), selected);
        if bulk == 1001 {
            assert_eq!(selected, 144); // tight descriptor reservation is exercised
        }
        drop(value);
        assert_eq!(nfs.rpc().backing_used(), 0);
        for failure in [false, true] {
            let result = nfs
                .write(handle(), 0, stable_how::FILE_SYNC, &Payload::empty())
                .await
                .unwrap();
            let wcc = match result {
                Nfs3Result::Ok(ok) => {
                    assert!(!failure);
                    ok.file_wcc
                }
                Nfs3Result::Err((_, err)) => {
                    assert!(failure);
                    err.file_wcc
                }
            };
            assert!(matches!(wcc.before, Nfs3Option::Some(_)));
            assert!(matches!(wcc.after, Nfs3Option::Some(_)));
            assert!(nfs.rpc().is_usable());
            assert_eq!(nfs.rpc().backing_used(), 0);
        }
        for mutate in [
            |p: &mut RpcLimits| p.decode.max_depth -= 1,
            |p: &mut RpcLimits| p.decode.max_depth = 1,
            |p: &mut RpcLimits| p.decode.max_reservation_bytes -= 1,
            |p: &mut RpcLimits| p.decode.max_reservation_bytes = 399,
            |p: &mut RpcLimits| {
                p.record.max_record_bytes -= 1;
                p.encode.max_wire_bytes -= 1;
            },
        ] {
            let mut insufficient = p;
            mutate(&mut insufficient);
            let io = Script::new(vec![]);
            let state = Arc::clone(&io.state);
            // Generic RPC deliberately still accepts these explicit smaller limits.
            let rpc = RpcClient::new(
                io,
                insufficient,
                opaque_auth::default(),
                opaque_auth::default(),
            )
            .unwrap();
            assert!(matches!(
                Nfs3Client::new(rpc, bulk),
                Err(RpcErrorKind::Configuration)
            ));
            let state = state.lock().unwrap();
            assert_eq!((state.reads, state.writes), (0, 0));
        }
    }
}

#[test]
fn nfs_protocol_reply_depth_is_tight() {
    use nfs3_types::nfs3::{READ3res, WRITE3res};
    use nfs3_types::rpc::rpc_msg;
    use nfs3_types::xdr_codec::{DecodeError, DecodeLimit, Decoder};
    assert_eq!(full_write_reply(false).len() - 4, 560);
    assert_eq!(full_write_reply(true).len() - 4, 544);
    for (reply, write) in [
        (full_read_reply(&[0x71]), false),
        (full_write_reply(false), true),
        (full_write_reply(true), true),
    ] {
        // Check the full reply and the NFS body separately so the header cannot
        // mask insufficient depth for full wcc nesting.
        for (offset, minimum) in [(0, 7), (424, if write { 7 } else { 6 })] {
            for depth in [minimum - 1, minimum] {
                let mut policy = limits().decode;
                policy.max_depth = depth;
                let record =
                    Payload::try_from_vec(reply[4 + offset..].to_vec(), policy.payload).unwrap();
                let mut decoder = Decoder::new(record, policy);
                // Decode in the same two independent roots as the transport.
                let header = if offset == 0 {
                    decoder.decode::<rpc_msg<'_, '_>>().map(drop)
                } else {
                    Ok(())
                };
                let result = header.and_then(|()| {
                    if write {
                        decoder.decode::<WRITE3res>().map(drop)
                    } else {
                        decoder.decode::<READ3res<Payload>>().map(drop)
                    }
                });
                if depth == minimum {
                    result.unwrap();
                    decoder.finish().unwrap();
                } else {
                    assert!(matches!(result, Err(DecodeError::Limit {
                    kind: DecodeLimit::Depth, actual, ..
                }) if actual == minimum));
                }
            }
        }
    }
}

#[tokio::test]
async fn nfs_exact_max_derived_write_count_and_semantic_errors() {
    use nfs3_types::nfs3::{Nfs3Result, READ3args, stable_how};
    const BULK: u32 = 1001; // deliberately unaligned
    let bytes = vec![0x5a; BULK as usize];
    let io = Script::new(vec![nfs_read_reply(BULK, &bytes), nfs_write_reply(BULK)]);
    let state = Arc::clone(&io.state);
    let mut nfs = Nfs3Client::new(client(io), BULK).unwrap();
    let args = READ3args {
        file: handle(),
        offset: 7,
        count: BULK,
    };
    let Nfs3Result::Ok(read) = nfs.read(&args).await.unwrap() else {
        panic!()
    };
    assert_eq!(read.data.try_to_vec().unwrap(), bytes);
    drop(read);
    let data = Payload::try_from_vec(bytes.clone(), limits().decode.payload).unwrap();
    assert!(matches!(
        nfs.write(handle(), 7, stable_how::FILE_SYNC, &data)
            .await
            .unwrap(),
        Nfs3Result::Ok(_)
    ));
    let sent = state.lock().unwrap().sent.clone();
    let first_len = (u32::from_be_bytes(sent[..4].try_into().unwrap()) & 0x7fff_ffff) as usize + 4;
    let write = &sent[first_len + 4..];
    // 40 RPC + 8 handle + 8 offset => derived count.
    assert_eq!(u32::from_be_bytes(write[56..60].try_into().unwrap()), BULK);

    for (count, data_len) in [(BULK - 1, BULK), (BULK + 1, BULK), (BULK + 1, BULK + 1)] {
        let mut nfs = Nfs3Client::new(
            client(Script::new(vec![nfs_read_reply(
                count,
                &vec![0; data_len as usize],
            )])),
            BULK,
        )
        .unwrap();
        let error = nfs.read(&args).await.unwrap_err();
        assert!(error.io_started && !error.connection_usable);
    }
    let mut nfs =
        Nfs3Client::new(client(Script::new(vec![nfs_write_reply(BULK + 1)])), BULK).unwrap();
    let data = Payload::try_from_vec(bytes, limits().decode.payload).unwrap();
    let error = nfs
        .write(handle(), 0, stable_how::FILE_SYNC, &data)
        .await
        .unwrap_err();
    assert!(matches!(error.kind, RpcErrorKind::Semantic));
    assert!(error.io_started && !error.connection_usable);
}

#[tokio::test]
async fn nfs_preio_argument_and_complete_error_reuse() {
    use nfs3_types::nfs3::{Nfs3Result, READ3args, nfsstat3, stable_how};
    let mut body = (nfsstat3::NFS3ERR_IO as u32).to_be_bytes().to_vec();
    body.extend_from_slice(&[0; 4]);
    let io = Script::new(vec![success(&body), nfs_read_reply(0, &[])]);
    let state = Arc::clone(&io.state);
    let mut nfs = Nfs3Client::new(client(io), 1001).unwrap();
    let data = Payload::try_from_vec(vec![0x5a; 1002], limits().decode.payload).unwrap();
    let pointer = data.chunks().next().unwrap().as_ptr();
    drop(nfs.write(handle(), 0, stable_how::FILE_SYNC, &data));
    let error = nfs
        .write(handle(), 0, stable_how::FILE_SYNC, &data)
        .await
        .unwrap_err();
    assert!(!error.io_started && error.connection_usable);
    assert_eq!(data.len(), 1002);
    assert_eq!(data.chunks().next().unwrap().as_ptr(), pointer);
    assert_eq!(state.lock().unwrap().reads, 0);
    let mut args = READ3args {
        file: handle(),
        offset: 0,
        count: 1002,
    };
    let error = nfs.read(&args).await.unwrap_err();
    assert!(!error.io_started && error.connection_usable);
    assert_eq!(state.lock().unwrap().writes, 0);
    args.count = 0;
    assert!(matches!(nfs.read(&args).await.unwrap(), Nfs3Result::Err(_)));
    assert!(nfs.rpc().is_usable());
    assert!(matches!(nfs.read(&args).await.unwrap(), Nfs3Result::Ok(_)));
}

#[test]
fn invalid_configuration_checked_math_and_nfs_unaligned_envelope() {
    for mutate in [
        |p: &mut RpcLimits| p.record.block_bytes = 0,
        |p: &mut RpcLimits| p.record.max_record_bytes = 0x8000_0000,
        |p: &mut RpcLimits| p.record.max_fragments = 0,
        |p: &mut RpcLimits| p.backing_bytes = 1,
        |p: &mut RpcLimits| p.decode.max_depth = 0,
        |p: &mut RpcLimits| p.encode.max_wire_bytes = 2049,
    ] {
        let mut p = limits();
        mutate(&mut p);
        p.validate().unwrap_err();
    }

    for mutate in [
        |p: &mut RpcLimits| p.decode.payload.max_segments = 126,
        |p: &mut RpcLimits| p.decode.payload.max_backing_capacity = 1015,
    ] {
        let mut p = limits();
        mutate(&mut p);
        let rpc = RpcClient::new(
            Script::new(vec![]),
            p,
            opaque_auth::default(),
            opaque_auth::default(),
        )
        .unwrap();
        assert!(Nfs3Client::new(rpc, 1001).is_err());
    }
    let mut p = limits();
    p.record.block_bytes = usize::MAX;
    p.validate().unwrap(); // shared div_ceil path, no overflowing round-up addition
    p.decode.max_metadata_bytes = 0;
    p.decode.max_collection_elements = 0;
    p.validate().unwrap(); // zero is a finite no-metadata/no-collection policy
}

#[test]
fn standard_error_chains_preserve_wrapped_causes() {
    use std::error::Error;

    use nfs3_types::async_io::FramingError;
    use nfs3_types::xdr_codec::{DecodeError, EncodeError, PayloadError};

    let wire = PayloadError::NonzeroPadding {
        offset: 7,
        value: 9,
    };
    let error = nfs3_client::rpc::RpcError {
        kind: RpcErrorKind::Decode(DecodeError::Wire(wire)),
        io_started: true,
        connection_usable: false,
    };
    let kind = error.source().unwrap();
    let decode = kind.source().unwrap();
    assert_eq!(
        decode.source().unwrap().downcast_ref::<PayloadError>(),
        Some(&wire)
    );
    assert!(error.to_string().contains("connection_usable=false"));
    let io = RpcErrorKind::Io(std::io::Error::from(ErrorKind::BrokenPipe));
    assert_eq!(
        io.source()
            .unwrap()
            .downcast_ref::<std::io::Error>()
            .unwrap()
            .kind(),
        ErrorKind::BrokenPipe
    );
    for kind in [
        RpcErrorKind::Storage(wire),
        RpcErrorKind::Encode(EncodeError::Allocation),
    ] {
        assert!(kind.source().is_some());
        assert!(!kind.to_string().is_empty());
    }
    let framing = FramingError::Storage(wire);
    assert_eq!(
        framing.source().unwrap().downcast_ref::<PayloadError>(),
        Some(&wire)
    );
    let framing = FramingError::Io(std::io::Error::from(ErrorKind::UnexpectedEof));
    assert!(framing.source().unwrap().is::<std::io::Error>());
    assert!(FramingError::Limit.source().is_none());
    assert!(RpcErrorKind::Configuration.source().is_none());
}
