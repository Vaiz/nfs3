#![allow(clippy::unwrap_used)]
#![cfg(any(feature = "tokio", feature = "smol"))]

use nfs3_client::bounded::{BoundedRpcClient, RpcLimits};
use nfs3_client::bounded_nfs::BoundedNfsClient;
use nfs3_client::io::{AsyncRead, AsyncWrite};
use nfs3_types::rpc::opaque_auth;
use nfs3_types::xdr_codec::{DecodeLimits, EncodeLimits, Payload, PayloadLimits, Void};

const fn limits() -> RpcLimits {
    RpcLimits {
        block_bytes: 7,
        max_record_bytes: 1024,
        max_fragments: 128,
        backing_bytes: 2048,
        max_builder_reservation_bytes: 65536,
        decode: DecodeLimits {
            max_metadata_bytes: 512,
            max_collection_elements: 64,
            max_depth: 32,
            max_reservation_bytes: 65536,
            payload: PayloadLimits {
                max_logical_bytes: 64,
                max_backing_capacity: 1024,
                max_segments: 128,
            },
        },
        encode: EncodeLimits {
            max_wire_bytes: 1024,
            max_metadata_bytes: 1024,
            max_chunks: 128,
            max_retained_capacity: 1024,
            max_depth: 32,
            max_reservation_bytes: 65536,
        },
    }
}

// Independent hand-authored peer: parse only the RPC marker/XID, emit fragmented
// RPC success and XDR opaque bytes. Does not use product Encoder/Decoder.
fn peer(listener: &std::net::TcpListener) {
    use std::io::{Read, Write};
    let (mut socket, _) = listener.accept().unwrap();
    for _ in 0..2 {
        let mut marker = [0; 4];
        socket.read_exact(&mut marker).unwrap();
        let len = (u32::from_be_bytes(marker) & 0x7fff_ffff) as usize;
        assert!(len <= 1024);
        let mut request = vec![0; len];
        socket.read_exact(&mut request).unwrap();
        assert_eq!(&request[4..16], &[0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 1]);
        let mut body = request[..4].to_vec();
        body.extend_from_slice(&[
            0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, b'a', b'b',
            b'c', 0,
        ]);
        // Empty nonfinal fragment, then byte-sized fragments, then final empty.
        socket.write_all(&[0; 4]).unwrap();
        for byte in body {
            socket.write_all(&[0, 0, 0, 1, byte]).unwrap();
        }
        socket.write_all(&[0x80, 0, 0, 0]).unwrap();
    }
}

async fn exercise<IO: AsyncRead + AsyncWrite>(io: IO) {
    let mut client =
        BoundedRpcClient::new(io, limits(), opaque_auth::default(), opaque_auth::default())
            .unwrap();
    let first: Payload = client.call(1, 2, 3, &Void).await.unwrap();
    let pointer = first.chunks().next().unwrap().as_ptr() as usize;
    let clone = first.try_clone().unwrap();
    assert_eq!(clone.chunks().next().unwrap().as_ptr() as usize, pointer);
    let second: Payload = client.call(1, 2, 3, &Void).await.unwrap();
    assert!(client.is_usable());
    drop(client);
    assert_eq!(first.try_to_vec().unwrap(), b"abc");
    assert_eq!(second.try_to_vec().unwrap(), b"abc");
    assert_eq!(clone.try_to_vec().unwrap(), b"abc");
}

fn nfs_peer(listener: &std::net::TcpListener) {
    use std::io::{Read, Write};
    let (mut socket, _) = listener.accept().unwrap();
    for procedure in [6_u32, 7] {
        let mut marker = [0; 4];
        socket.read_exact(&mut marker).unwrap();
        let len = (u32::from_be_bytes(marker) & 0x7fff_ffff) as usize;
        assert!(len <= 1024);
        let mut request = vec![0; len];
        socket.read_exact(&mut request).unwrap();
        assert_eq!(&request[20..24], &procedure.to_be_bytes());
        assert_eq!(&request[56..60], &17_u32.to_be_bytes());
        if procedure == 7 {
            assert_eq!(&request[64..68], &17_u32.to_be_bytes());
            assert_eq!(&request[68..85], &[0x5a; 17]);
            assert_eq!(&request[85..], &[0; 3]);
        }
        let mut reply = request[..4].to_vec();
        reply.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        reply.extend_from_slice(&[0; 8]); // NFS OK, absent attributes/pre_op_attr
        if procedure == 6 {
            reply.extend_from_slice(&[0, 0, 0, 17, 0, 0, 0, 1, 0, 0, 0, 17]);
            reply.extend_from_slice(&[0x5a; 17]);
            reply.extend_from_slice(&[0; 3]);
        } else {
            reply.extend_from_slice(&[0; 4]); // absent post_op_attr
            reply.extend_from_slice(&[0, 0, 0, 17, 0, 0, 0, 2]);
            reply.extend_from_slice(&[0x55; 8]);
        }
        // Different RPC and physical segmentation on each operation.
        for (index, chunk) in reply.chunks(3).enumerate() {
            let last = (index + 1) * 3 >= reply.len();
            let flag = if last { 0x8000_0000 } else { 0 };
            socket
                .write_all(&(u32::try_from(chunk.len()).unwrap() | flag).to_be_bytes())
                .unwrap();
            socket.write_all(chunk).unwrap();
        }
    }
}

async fn exercise_nfs<IO: AsyncRead + AsyncWrite>(io: IO) {
    use nfs3_types::nfs3::{Nfs3Result, READ3args, nfs_fh3, stable_how};
    use nfs3_types::xdr_codec::Opaque;
    let rpc = BoundedRpcClient::new(io, limits(), opaque_auth::default(), opaque_auth::default())
        .unwrap();
    let mut nfs = BoundedNfsClient::new(rpc, 17).unwrap();
    let file = nfs_fh3 {
        data: Opaque::borrowed(b"file"),
    };
    let Nfs3Result::Ok(value) = nfs
        .read(&READ3args {
            file: file.clone(),
            offset: 0,
            count: 17,
        })
        .await
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(value.data.try_to_vec().unwrap(), [0x5a; 17]);
    let Nfs3Result::Ok(value) = nfs
        .write(file, 0, stable_how::FILE_SYNC, value.data)
        .await
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(value.count, 17);
    assert_eq!(nfs.rpc().backing_used(), 0);
    assert!(nfs.rpc().is_usable());
}

#[cfg(feature = "tokio")]
#[tokio::test]
async fn tokio_nfs_read_write_loopback() {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let peer = std::thread::spawn(move || nfs_peer(&listener));
    let socket = tokio::net::TcpStream::connect(address).await.unwrap();
    exercise_nfs(nfs3_client::tokio::TokioIo::new(socket)).await;
    peer.join().unwrap();
}

#[cfg(feature = "smol")]
#[test]
fn smol_nfs_read_write_loopback() {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let peer = std::thread::spawn(move || nfs_peer(&listener));
    smol::block_on(async {
        let socket = smol::net::TcpStream::connect(address).await.unwrap();
        exercise_nfs(nfs3_client::smol::SmolIo::new(socket)).await;
    });
    peer.join().unwrap();
}

#[cfg(feature = "tokio")]
#[tokio::test]
async fn tokio_real_loopback() {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let peer = std::thread::spawn(move || peer(&listener));
    let socket = tokio::net::TcpStream::connect(address).await.unwrap();
    exercise(nfs3_client::tokio::TokioIo::new(socket)).await;
    peer.join().unwrap();
}

#[cfg(feature = "smol")]
#[test]
fn smol_real_loopback() {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let peer = std::thread::spawn(move || peer(&listener));
    smol::block_on(async {
        let socket = smol::net::TcpStream::connect(address).await.unwrap();
        exercise(nfs3_client::smol::SmolIo::new(socket)).await;
    });
    peer.join().unwrap();
}
