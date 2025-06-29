// Basic XDR codec tests for fundamental data types and structures
#![allow(clippy::unwrap_used)]

use std::borrow::Cow;
use std::io::Cursor;

use nfs3_types::xdr_codec::{Opaque, Pack, Unpack, XdrCodec};

#[derive(Copy, Clone, Debug, PartialEq, Eq, XdrCodec)]
#[repr(u32)]
enum TestEnum {
    Field1 = 1,
    Field2 = 2,
    Field3 = 0x1234_5678,
}

#[derive(Debug, PartialEq, XdrCodec)]
struct SimpleStruct {
    a: u32,
    b: u32,
}

#[derive(Debug, PartialEq, XdrCodec)]
struct NestedStruct {
    inner: SimpleStruct,
    flag: bool,
}

#[derive(Debug, PartialEq, XdrCodec)]
struct StructWithLifetime<'a> {
    inner: Opaque<'a>,
}

#[derive(Debug, PartialEq, XdrCodec)]
struct TupleStruct(u32, u32);

#[derive(Debug, PartialEq, XdrCodec)]
struct UnitStruct;

#[test]
fn enum_pack() {
    let mut bytes = Vec::new();
    let len = TestEnum::Field1.pack(&mut bytes).unwrap();
    assert_eq!(TestEnum::Field1.packed_size(), 4);
    assert_eq!(len, 4);
    assert_eq!(bytes, [0, 0, 0, 1]);

    let mut bytes = Vec::new();
    let len = TestEnum::Field2.pack(&mut bytes).unwrap();
    assert_eq!(TestEnum::Field2.packed_size(), 4);
    assert_eq!(len, 4);
    assert_eq!(bytes, [0, 0, 0, 2]);

    let mut bytes = Vec::new();
    let len = TestEnum::Field3.pack(&mut bytes).unwrap();
    assert_eq!(TestEnum::Field3.packed_size(), 4);
    assert_eq!(len, 4);
    assert_eq!(bytes, [0x12, 0x34, 0x56, 0x78]);
}

#[test]
fn enum_unpack() {
    fn unpack(buf: [u8; 4]) -> TestEnum {
        let (e, len) = TestEnum::unpack(&mut Cursor::new(buf)).unwrap();
        assert_eq!(len, 4);
        e
    }

    assert_eq!(TestEnum::Field1, unpack([0, 0, 0, 1]));
    assert_eq!(TestEnum::Field2, unpack([0, 0, 0, 2]));
    assert_eq!(TestEnum::Field3, unpack([0x12, 0x34, 0x56, 0x78]));
}

#[test]
fn enum_roundtrip() {
    let test_cases = [TestEnum::Field1, TestEnum::Field2, TestEnum::Field3];

    for original in test_cases {
        let mut buffer = Vec::new();
        let pack_len = original.pack(&mut buffer).unwrap();
        assert_eq!(pack_len, original.packed_size());

        let mut cursor = Cursor::new(buffer);
        let (deserialized, unpack_len) = TestEnum::unpack(&mut cursor).unwrap();
        assert_eq!(pack_len, unpack_len);
        assert_eq!(original, deserialized);
    }
}

#[test]
fn simple_struct_serialization() {
    let original = SimpleStruct { a: 0x123, b: 0x456 };

    let mut buffer = Vec::new();
    let len = original.pack(&mut buffer).unwrap();
    assert_eq!(original.packed_size(), 8);
    assert_eq!(len, 8);
    assert_eq!(buffer, [0x00, 0x00, 0x01, 0x23, 0x00, 0x00, 0x04, 0x56]);

    let mut cursor = Cursor::new(buffer);
    let (deserialized, len) = SimpleStruct::unpack(&mut cursor).unwrap();
    assert_eq!(len, 8);
    assert_eq!(original, deserialized);
}

#[test]
fn nested_struct_serialization() {
    let original = NestedStruct {
        inner: SimpleStruct {
            a: 0x789,
            b: 0x1011,
        },
        flag: true,
    };

    let mut buffer = Vec::new();
    let len = original.pack(&mut buffer).unwrap();
    assert_eq!(original.packed_size(), 12);
    assert_eq!(len, 12);
    assert_eq!(
        buffer,
        [
            0x00, 0x00, 0x07, 0x89, 0x00, 0x00, 0x10, 0x11, 0x00, 0x00, 0x00, 0x01
        ]
    );

    let mut cursor = Cursor::new(buffer);
    let (deserialized, len) = NestedStruct::unpack(&mut cursor).unwrap();
    assert_eq!(len, 12);
    assert_eq!(original, deserialized);
}

#[test]
fn struct_with_lifetime_serialization() {
    let original = StructWithLifetime {
        inner: Opaque(Cow::Borrowed(b"Hello")),
    };

    let mut buffer = Vec::new();
    let len = original.pack(&mut buffer).unwrap();
    assert_eq!(original.packed_size(), 12);
    assert_eq!(len, 12);
    assert_eq!(buffer[0..4], [0u8, 0, 0, 5]);
    assert_eq!(&buffer[4..], b"Hello\0\0\0");

    let mut cursor = Cursor::new(buffer);
    let (deserialized, len) = StructWithLifetime::unpack(&mut cursor).unwrap();
    assert_eq!(len, 12);
    assert_eq!(original, deserialized);
}

#[test]
fn tuple_struct_serialization() {
    let original = TupleStruct(0x123, 0x456);

    let mut buffer = Vec::new();
    let len = original.pack(&mut buffer).unwrap();
    assert_eq!(original.packed_size(), 8);
    assert_eq!(len, 8);
    assert_eq!(buffer, [0x00, 0x00, 0x01, 0x23, 0x00, 0x00, 0x04, 0x56]);

    let mut cursor = Cursor::new(buffer);
    let (deserialized, len) = TupleStruct::unpack(&mut cursor).unwrap();
    assert_eq!(len, 8);
    assert_eq!(original, deserialized);
}

#[test]
fn unit_struct_serialization() {
    let original = UnitStruct;

    let mut buffer = Vec::new();
    let len = original.pack(&mut buffer).unwrap();
    assert_eq!(original.packed_size(), 0);
    assert_eq!(len, 0);
    assert_eq!(buffer, []);

    let mut cursor = Cursor::new(buffer);
    let (deserialized, len) = UnitStruct::unpack(&mut cursor).unwrap();
    assert_eq!(len, 0);
    assert_eq!(original, deserialized);
}

#[test]
fn struct_edge_cases() {
    // Test with zero values
    let zero_struct = SimpleStruct { a: 0, b: 0 };
    let mut buffer = Vec::new();
    let len = zero_struct.pack(&mut buffer).unwrap();
    assert_eq!(len, 8);
    assert_eq!(buffer, [0, 0, 0, 0, 0, 0, 0, 0]);

    // Test with maximum values
    let max_struct = SimpleStruct {
        a: u32::MAX,
        b: u32::MAX,
    };
    let mut buffer = Vec::new();
    let len = max_struct.pack(&mut buffer).unwrap();
    assert_eq!(len, 8);
    assert_eq!(buffer, [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
}

#[test]
fn opaque_data_edge_cases() {
    // Test empty opaque data
    let empty_opaque = Opaque(Cow::Borrowed(b""));
    let mut buffer = Vec::new();
    let len = empty_opaque.pack(&mut buffer).unwrap();
    assert_eq!(len, 4); // Length field only
    assert_eq!(buffer, [0, 0, 0, 0]);

    // Test opaque data requiring padding
    let unpadded_opaque = Opaque(Cow::Borrowed(b"ABC")); // 3 bytes, needs 1 byte padding
    let mut buffer = Vec::new();
    let len = unpadded_opaque.pack(&mut buffer).unwrap();
    assert_eq!(len, 8); // 4 bytes length + 3 bytes data + 1 byte padding
    assert_eq!(buffer, [0, 0, 0, 3, b'A', b'B', b'C', 0]);
}
