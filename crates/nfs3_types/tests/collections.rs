// Tests for XDR collection types (Vec, List, BoundedList)
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use nfs3_types::xdr_codec::{BoundedList, List, Pack, Unpack};

#[test]
fn vec_serialization() {
    let original = vec![0x1234, 0x5678, 0x9abc];

    let mut buffer = Vec::new();
    let len = original.pack(&mut buffer).unwrap();
    assert_eq!(original.packed_size(), 16); // 4 bytes for length + 12 bytes for data
    assert_eq!(len, 16);
    assert_eq!(
        buffer,
        [
            0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x12, 0x34, 0x00, 0x00, 0x56, 0x78, 0x00, 0x00,
            0x9a, 0xbc
        ]
    );

    let mut cursor = Cursor::new(buffer);
    let (deserialized, len) = Vec::<u32>::unpack(&mut cursor).unwrap();
    assert_eq!(len, 16);
    assert_eq!(original, deserialized);
}

#[test]
fn list_serialization() {
    let original = List(vec![0x1234, 0x5678, 0x9abc]);

    let mut buffer = Vec::new();
    let len = original.pack(&mut buffer).unwrap();
    assert_eq!(original.packed_size(), 28);
    assert_eq!(len, 28);
    assert_eq!(
        buffer,
        [
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x12, 0x34, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
            0x56, 0x78, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x9a, 0xbc, 0x00, 0x00, 0x00, 0x00,
        ]
    );

    let mut cursor = Cursor::new(buffer);
    let (deserialized, len) = List::<u32>::unpack(&mut cursor).unwrap();
    assert_eq!(len, 28);
    assert_eq!(original.0, deserialized.0);
}

#[test]
fn bounded_list_basic() {
    // (4+4)*3 + 4 = 28 bytes
    let mut bounded_list = BoundedList::<u32>::new(28);
    assert!(bounded_list.try_push(0x1234).is_ok());
    assert!(bounded_list.try_push(0x5678).is_ok());
    assert!(bounded_list.try_push(0x9abc).is_ok());
    assert!(bounded_list.try_push(0xdef0).is_err());

    let list = bounded_list.into_inner();
    assert_eq!(list.0, vec![0x1234, 0x5678, 0x9abc]);
}

#[test]
fn bounded_list_exact_boundary() {
    let mut bounded_list = BoundedList::<u32>::new(27);
    assert!(bounded_list.try_push(0x1234).is_ok());
    assert!(bounded_list.try_push(0x5678).is_ok());
    assert!(bounded_list.try_push(0x9abc).is_err());

    let list = bounded_list.into_inner();
    assert_eq!(list.0, vec![0x1234, 0x5678]);

    let mut bounded_list = BoundedList::<u32>::new(29);
    assert!(bounded_list.try_push(0x1234).is_ok());
    assert!(bounded_list.try_push(0x5678).is_ok());
    assert!(bounded_list.try_push(0x9abc).is_ok());
    assert!(bounded_list.try_push(0xdef0).is_err());

    let list = bounded_list.into_inner();
    assert_eq!(list.0, vec![0x1234, 0x5678, 0x9abc]);
}

#[test]
fn empty_collections() {
    // Test empty Vec
    let empty_vec: Vec<u32> = Vec::new();
    let mut buffer = Vec::new();
    let len = empty_vec.pack(&mut buffer).unwrap();
    assert_eq!(len, 4); // Just the length field
    assert_eq!(buffer, [0, 0, 0, 0]);

    let mut cursor = Cursor::new(buffer);
    let (deserialized, len) = Vec::<u32>::unpack(&mut cursor).unwrap();
    assert_eq!(len, 4);
    assert_eq!(empty_vec, deserialized);

    // Test empty List
    let empty_list = List(Vec::<u32>::new());
    let mut buffer = Vec::new();
    let len = empty_list.pack(&mut buffer).unwrap();
    assert_eq!(len, 4); // Just the end marker
    assert_eq!(buffer, [0, 0, 0, 0]);

    let mut cursor = Cursor::new(buffer);
    let (deserialized, len) = List::<u32>::unpack(&mut cursor).unwrap();
    assert_eq!(len, 4);
    assert_eq!(empty_list.0, deserialized.0);
}

#[test]
fn single_item_collections() {
    // Test single item Vec
    let single_vec = vec![0x1234_5678];
    let mut buffer = Vec::new();
    let len = single_vec.pack(&mut buffer).unwrap();
    assert_eq!(len, 8); // 4 bytes length + 4 bytes data
    assert_eq!(buffer, [0, 0, 0, 1, 0x12, 0x34, 0x56, 0x78]);

    let mut cursor = Cursor::new(buffer);
    let (deserialized, len) = Vec::<u32>::unpack(&mut cursor).unwrap();
    assert_eq!(len, 8);
    assert_eq!(single_vec, deserialized);

    // Test single item List
    let single_list = List(vec![0x1234_5678]);
    let mut buffer = Vec::new();
    let len = single_list.pack(&mut buffer).unwrap();
    assert_eq!(len, 12); // 4 bytes indicator + 4 bytes data + 4 bytes end marker
    assert_eq!(buffer, [0, 0, 0, 1, 0x12, 0x34, 0x56, 0x78, 0, 0, 0, 0]);

    let mut cursor = Cursor::new(buffer);
    let (deserialized, len) = List::<u32>::unpack(&mut cursor).unwrap();
    assert_eq!(len, 12);
    assert_eq!(single_list.0, deserialized.0);
}

#[test]
fn bounded_list_edge_cases() {
    // Test with zero size limit (should not allow any items)
    // Even zero size should have the end marker, so it should be at least 4
    let mut bounded_list = BoundedList::<u32>::new(4); // Just end marker
    assert!(bounded_list.try_push(0x1234).is_err());

    // Test with exactly one item size
    // Each item needs 4 bytes indicator + 4 bytes data, plus 4 bytes end marker = 12 bytes total
    let mut bounded_list = BoundedList::<u32>::new(12);
    assert!(bounded_list.try_push(0x1234).is_ok());
    assert!(bounded_list.try_push(0x5678).is_err());

    let list = bounded_list.into_inner();
    assert_eq!(list.0, vec![0x1234]);
}

#[test]
fn large_collections() {
    // Test with larger collections to verify scaling
    let large_vec: Vec<u32> = (0..100).collect();
    let mut buffer = Vec::new();
    let len = large_vec.pack(&mut buffer).unwrap();
    assert_eq!(len, 404); // 4 bytes length + 100 * 4 bytes data

    let mut cursor = Cursor::new(buffer);
    let (deserialized, len) = Vec::<u32>::unpack(&mut cursor).unwrap();
    assert_eq!(len, 404);
    assert_eq!(large_vec, deserialized);

    // Test bounded list with many items
    // Each item needs 4 bytes indicator + 4 bytes data = 8 bytes per item
    // Plus 4 bytes for the end marker
    // So for 100 items: 100 * 8 + 4 = 804 bytes
    let mut bounded_list = BoundedList::<u32>::new(804);
    for i in 0..100 {
        assert!(bounded_list.try_push(i).is_ok(), "Failed to push item {i}");
    }
    assert!(bounded_list.try_push(100).is_err()); // Should fail on 101st item

    let list = bounded_list.into_inner();
    assert_eq!(list.0.len(), 100);
    assert_eq!(list.0, (0..100).collect::<Vec<u32>>());
}
