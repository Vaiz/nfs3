use std::future::ready;
use std::io::ErrorKind;

use bytes::{Buf, Bytes, BytesMut};
use future_assert::FutureTestExt;
use nfs3_types::rpc::{
    accept_stat_data, accepted_reply, fragment_header, msg_body, opaque_auth, reply_body, rpc_msg,
};
use nfs3_types::xdr_codec::{Encoder, Opaque, Pack, Void};

use super::RpcClient;
use crate::RpcError;
use crate::io::{AsyncRead, AsyncWrite};

/// In-memory transport that transfers at most 7 bytes per read or write.
#[derive(Default)]
struct Mock {
    input: Bytes,
    output: Vec<u8>,
}

impl AsyncRead for Mock {
    fn async_read(
        &mut self,
        buf: &mut [u8],
    ) -> impl Future<Output = std::io::Result<usize>> + Send {
        let n = buf.len().min(self.input.len()).min(7);
        buf[..n].copy_from_slice(&self.input.split_to(n));
        ready(Ok(n))
    }
}

impl AsyncWrite for Mock {
    fn async_write(&mut self, buf: &[u8]) -> impl Future<Output = std::io::Result<usize>> + Send {
        let n = buf.len().min(7);
        self.output.extend_from_slice(&buf[..n]);
        ready(Ok(n))
    }
}

fn reply(xid: u32, result: &Opaque<'_>, fragments: &[usize]) -> Bytes {
    let msg = rpc_msg {
        xid,
        body: msg_body::REPLY(reply_body::MSG_ACCEPTED(accepted_reply {
            verf: opaque_auth::default(),
            reply_data: accept_stat_data::SUCCESS,
        })),
    };
    let mut out = Encoder::new(0);
    msg.pack(&mut out).unwrap();
    result.pack(&mut out).unwrap();
    let mut record = out.finish_record().unwrap();
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

#[test]
fn fragmented_reply_is_reassembled() {
    let payload = Opaque::owned((0..=255u8).cycle().take(5000).collect());
    let mut client = RpcClient::new(Mock::default());
    client.io.input = reply(client.xid, &payload, &[0, 13, 2000, 0]);

    let result: Opaque<'static> = client.call(1, 1, 0, &payload).unwrap_ready().unwrap();
    assert_eq!(result, payload);
    assert!(client.io.input.is_empty());

    // The call, with its payload in a separate chunk, was written completely.
    let output = &client.io.output;
    let marker = u32::from_be_bytes(output[..4].try_into().unwrap());
    assert_eq!(marker as usize, 0x8000_0000 | (output.len() - 4));
    assert!(output.ends_with(&payload[payload.len() - 100..]));
}

#[test]
fn truncated_reply_is_an_io_error() {
    let mut client = RpcClient::new(Mock::default());
    let wire = reply(client.xid, &Opaque::borrowed(b"abc"), &[0]);
    client.io.input = wire.slice(..wire.len() - 1);

    let err = client
        .call::<_, Opaque<'static>>(1, 1, 0, &Void)
        .unwrap_ready()
        .unwrap_err();
    assert!(matches!(err, RpcError::Io(ref e) if e.kind() == ErrorKind::UnexpectedEof));
    assert!(!err.is_connection_reusable());
}

#[test]
fn fragmented_reply_at_the_limit_is_accepted() {
    let mut client = RpcClient::new(Mock::default());
    let fragments = [1024, 0];
    let marker_bytes = fragments.len() * 4;
    let overhead =
        reply(client.xid, &Opaque::borrowed(&[0; 1024]), &fragments).len() - marker_bytes - 1024;
    let payload = Opaque::owned(vec![0; 2 * 1024 * 1024 - overhead]);
    client.io.input = reply(client.xid, &payload, &fragments);

    let result: Opaque<'static> = client.call(1, 1, 0, &Void).unwrap_ready().unwrap();
    assert_eq!(result, payload);
    assert!(client.io.input.is_empty());
}

#[test]
fn oversized_single_fragment_reply_is_rejected() {
    let marker = fragment_header::new(2 * 1024 * 1024 + 1, true).unwrap();
    assert_oversized_reply(marker.into_xdr_buf().to_vec());
}

#[test]
fn oversized_fragmented_reply_is_rejected() {
    let mut wire = Vec::new();
    wire.extend_from_slice(&fragment_header::new(4, false).unwrap().into_xdr_buf());
    wire.extend_from_slice(&[0; 4]);
    wire.extend_from_slice(
        &fragment_header::new(2 * 1024 * 1024, true)
            .unwrap()
            .into_xdr_buf(),
    );
    assert_oversized_reply(wire);
}

fn assert_oversized_reply(mut wire: Vec<u8>) {
    // The over-limit fragment must be rejected before any of its payload is read.
    wire.push(0);
    let mut client = RpcClient::new(Mock {
        input: Bytes::from(wire),
        ..Mock::default()
    });

    let err = client
        .call::<_, Void>(1, 1, 0, &Void)
        .unwrap_ready()
        .unwrap_err();
    assert!(
        matches!(err, RpcError::Io(ref e) if e.kind() == ErrorKind::InvalidData),
        "{err}"
    );
    assert!(err.to_string().contains("exceeds the size limit"));
    assert!(!err.is_connection_reusable());
    assert_eq!(client.io.input.as_ref(), &[0]);
}
