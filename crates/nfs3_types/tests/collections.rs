// Tests for XDR collection types (Vec, List).

use nfs3_types::xdr_codec::List;
mod support;
use support::{decode, encode};

#[test]
fn vec_serialization() {
    let original = vec![0x1234, 0x5678, 0x9abc];

    let buffer = encode(&original);
    assert_eq!(
        buffer,
        [
            0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x12, 0x34, 0x00, 0x00, 0x56, 0x78, 0x00, 0x00,
            0x9a, 0xbc
        ]
    );

    let deserialized: Vec<u32> = decode(&buffer).unwrap();
    assert_eq!(original, deserialized);
}

#[test]
fn list_serialization() {
    let original = List(vec![0x1234, 0x5678, 0x9abc]);

    let buffer = encode(&original);
    assert_eq!(
        buffer,
        [
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x12, 0x34, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
            0x56, 0x78, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x9a, 0xbc, 0x00, 0x00, 0x00, 0x00,
        ]
    );

    let deserialized: List<u32> = decode(&buffer).unwrap();
    assert_eq!(original.0, deserialized.0);
}

#[test]
fn empty_collections() {
    // Test empty Vec
    let empty_vec: Vec<u32> = Vec::new();
    let buffer = encode(&empty_vec);
    assert_eq!(buffer, [0, 0, 0, 0]);

    let deserialized: Vec<u32> = decode(&buffer).unwrap();
    assert_eq!(empty_vec, deserialized);

    // Test empty List
    let empty_list = List(Vec::<u32>::new());
    let buffer = encode(&empty_list);
    assert_eq!(buffer, [0, 0, 0, 0]);

    let deserialized: List<u32> = decode(&buffer).unwrap();
    assert_eq!(empty_list.0, deserialized.0);
}

#[test]
fn single_item_collections() {
    // Test single item Vec
    let single_vec = vec![0x1234_5678];
    let buffer = encode(&single_vec);
    assert_eq!(buffer, [0, 0, 0, 1, 0x12, 0x34, 0x56, 0x78]);

    let deserialized: Vec<u32> = decode(&buffer).unwrap();
    assert_eq!(single_vec, deserialized);

    // Test single item List
    let single_list = List(vec![0x1234_5678]);
    let buffer = encode(&single_list);
    assert_eq!(buffer, [0, 0, 0, 1, 0x12, 0x34, 0x56, 0x78, 0, 0, 0, 0]);

    let deserialized: List<u32> = decode(&buffer).unwrap();
    assert_eq!(single_list.0, deserialized.0);
}

#[test]
fn large_collections() {
    // Test with larger collections to verify scaling
    let large_vec: Vec<u32> = (0..100).collect();
    let buffer = encode(&large_vec);
    assert_eq!(buffer.len(), 404);
    let deserialized: Vec<u32> = decode(&buffer).unwrap();
    assert_eq!(large_vec, deserialized);
}
