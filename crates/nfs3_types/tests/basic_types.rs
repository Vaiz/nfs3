// Basic XDR codec tests for fundamental data types and structures

use std::borrow::Cow;

use nfs3_types::xdr_codec::{Opaque, XdrCodec};
mod support;
use support::{decode, encode};

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
fn enum_encode() {
    let bytes = encode(&TestEnum::Field1);
    assert_eq!(bytes, [0, 0, 0, 1]);

    let bytes = encode(&TestEnum::Field2);
    assert_eq!(bytes, [0, 0, 0, 2]);

    let bytes = encode(&TestEnum::Field3);
    assert_eq!(bytes, [0x12, 0x34, 0x56, 0x78]);
}

#[test]
fn enum_decode() {
    assert_eq!(TestEnum::Field1, decode(&[0, 0, 0, 1]).unwrap());
    assert_eq!(TestEnum::Field2, decode(&[0, 0, 0, 2]).unwrap());
    assert_eq!(TestEnum::Field3, decode(&[0x12, 0x34, 0x56, 0x78]).unwrap());
}

#[test]
fn enum_roundtrip() {
    let test_cases = [TestEnum::Field1, TestEnum::Field2, TestEnum::Field3];

    for original in test_cases {
        let buffer = encode(&original);
        let deserialized: TestEnum = decode(&buffer).unwrap();
        assert_eq!(original, deserialized);
    }
}

#[test]
fn simple_struct_serialization() {
    let original = SimpleStruct { a: 0x123, b: 0x456 };

    let buffer = encode(&original);
    assert_eq!(buffer, [0x00, 0x00, 0x01, 0x23, 0x00, 0x00, 0x04, 0x56]);

    let deserialized: SimpleStruct = decode(&buffer).unwrap();
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

    let buffer = encode(&original);
    assert_eq!(
        buffer,
        [
            0x00, 0x00, 0x07, 0x89, 0x00, 0x00, 0x10, 0x11, 0x00, 0x00, 0x00, 0x01
        ]
    );

    let deserialized: NestedStruct = decode(&buffer).unwrap();
    assert_eq!(original, deserialized);
}

#[test]
fn struct_with_lifetime_serialization() {
    let original = StructWithLifetime {
        inner: Opaque(Cow::Borrowed(b"Hello")),
    };

    let buffer = encode(&original);
    assert_eq!(buffer[0..4], [0u8, 0, 0, 5]);
    assert_eq!(&buffer[4..], b"Hello\0\0\0");

    let deserialized: StructWithLifetime<'static> = decode(&buffer).unwrap();
    assert!(matches!(deserialized.inner.0, Cow::Owned(_)));
    assert_eq!(original, deserialized);
}

#[test]
fn tuple_struct_serialization() {
    let original = TupleStruct(0x123, 0x456);

    let buffer = encode(&original);
    assert_eq!(buffer, [0x00, 0x00, 0x01, 0x23, 0x00, 0x00, 0x04, 0x56]);

    let deserialized: TupleStruct = decode(&buffer).unwrap();
    assert_eq!(original, deserialized);
}

#[test]
fn unit_struct_serialization() {
    let original = UnitStruct;

    let buffer = encode(&original);
    assert_eq!(buffer, []);

    let deserialized: UnitStruct = decode(&buffer).unwrap();
    assert_eq!(original, deserialized);
}

#[test]
fn struct_edge_cases() {
    // Test with zero values
    let zero_struct = SimpleStruct { a: 0, b: 0 };
    let buffer = encode(&zero_struct);
    assert_eq!(buffer, [0, 0, 0, 0, 0, 0, 0, 0]);

    // Test with maximum values
    let max_struct = SimpleStruct {
        a: u32::MAX,
        b: u32::MAX,
    };
    let buffer = encode(&max_struct);
    assert_eq!(buffer, [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
}

#[test]
fn opaque_data_edge_cases() {
    // Test empty opaque data
    let empty_opaque = Opaque(Cow::Borrowed(b""));
    let buffer = encode(&empty_opaque);
    assert_eq!(buffer, [0, 0, 0, 0]);

    // Test opaque data requiring padding
    let unpadded_opaque = Opaque(Cow::Borrowed(b"ABC")); // 3 bytes, needs 1 byte padding
    let buffer = encode(&unpadded_opaque);
    assert_eq!(buffer, [0, 0, 0, 3, b'A', b'B', b'C', 0]);
}
