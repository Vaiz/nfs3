#![allow(
    dead_code,
    reason = "Each integration test uses a subset of the shared fixtures"
)]
#![expect(
    clippy::unwrap_used,
    reason = "Shared test fixture helpers abort on codec failure"
)]

use nfs3_types::xdr_codec::*;

pub const fn encode_limits() -> EncodeLimits {
    EncodeLimits {
        max_wire_bytes: 65536,
        max_metadata_bytes: 65536,
        max_chunks: 1024,
        max_retained_capacity: 65536,
        max_depth: 64,
        max_reservation_bytes: 1 << 20,
    }
}

pub fn encode(value: &(impl Encode + ?Sized)) -> Vec<u8> {
    let mut encoder = Encoder::new(encode_limits());
    encoder.encode(value).unwrap();
    let plan = encoder.finish().unwrap();
    let bytes: Vec<_> = plan.chunks().flatten().copied().collect();
    assert_eq!(bytes.len(), plan.len());
    bytes
}

pub fn decode<T: Decode>(bytes: &[u8]) -> DecodeResult<T> {
    let payload = PayloadLimits {
        max_logical_bytes: 65536,
        max_backing_capacity: 65536,
        max_segments: 1024,
    };
    let mut decoder = Decoder::new(
        Payload::try_from_vec(bytes.to_vec(), payload)?,
        DecodeLimits {
            max_metadata_bytes: 65536,
            max_collection_elements: 4096,
            max_depth: 64,
            max_reservation_bytes: 1 << 20,
            payload,
        },
    );
    let value = decoder.decode()?;
    decoder.finish()?;
    Ok(value)
}
