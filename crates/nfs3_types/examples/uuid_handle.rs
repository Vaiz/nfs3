use bytes::Buf;
use nfs3_types::nfs3::nfs_fh3;
use nfs3_types::xdr_codec::{BoundedOpaque, Decoder, Encoder, Pack, Unpack};
use uuid::Uuid;

#[derive(Debug, PartialEq, Eq)]
struct FileHandle {
    generation: u64,
    id: Uuid,
}

fn main() -> nfs3_types::xdr_codec::Result<()> {
    let handle = FileHandle {
        generation: 42,
        id: Uuid::from_u128(0x550e8400_e29b_41d4_a716_446655440000),
    };

    // Match nfs3_server's 8-byte little-endian generation prefix.
    let mut data = [0; 24];
    data[..8].copy_from_slice(&handle.generation.to_le_bytes());
    data[8..].copy_from_slice(handle.id.as_bytes());
    let wire_handle = nfs_fh3 {
        data: BoundedOpaque::from_array(&data).into_owned(),
    };

    let mut encoder = Encoder::new(wire_handle.packed_size());
    wire_handle.pack(&mut encoder)?;
    let mut record = encoder.finish_record()?;
    record.advance(4); // Skip the RPC record marker before decoding XDR.
    let mut decoder = Decoder::copying(record.copy_to_bytes(record.remaining()));
    let received = nfs_fh3::unpack(&mut decoder)?;
    assert!(decoder.is_empty());
    assert_eq!(received.data.len(), 24);

    let restored = FileHandle {
        generation: u64::from_le_bytes(
            received.data[..8]
                .try_into()
                .expect("the generation occupies 8 bytes"),
        ),
        id: Uuid::from_slice(&received.data[8..]).expect("the UUID occupies 16 bytes"),
    };
    assert_eq!(restored, handle);
    println!("{restored:?}");
    Ok(())
}
