use std::borrow::Cow;
use std::io::Cursor;

use nfs3_types::xdr_codec::{Opaque, Pack, PackedSize, Unpack, XdrCodec};

#[derive(Copy, Clone, Debug, PartialEq, Eq, XdrCodec)]
#[repr(u32)]
enum TestEnum {
    Field1 = 1,
    Field2 = 2,
    Field3 = 0x1234_5678,
}

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

#[test]
fn test_simple_struct_serialization() {
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
fn test_nested_struct_serialization() {
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
        [0x00, 0x00, 0x07, 0x89, 0x00, 0x00, 0x10, 0x11, 0x00, 0x00, 0x00, 0x01]
    );

    let mut cursor = Cursor::new(buffer);
    let (deserialized, len) = NestedStruct::unpack(&mut cursor).unwrap();
    assert_eq!(len, 12);
    assert_eq!(original, deserialized);
}

#[derive(Debug, PartialEq, XdrCodec)]
struct StructWithLifetime<'a> {
    inner: Opaque<'a>,
}

#[test]
fn test_struct_with_lifetime_serialization() {
    let original = StructWithLifetime {
        inner: Opaque(Cow::Borrowed("Hello".as_bytes())),
    };

    let mut buffer = Vec::new();
    let len = original.pack(&mut buffer).unwrap();
    assert_eq!(original.packed_size(), 12);
    assert_eq!(len, 12);
    assert_eq!(buffer[0..4], [0u8, 0, 0, 5]);
    assert_eq!(&buffer[4..], "Hello\0\0\0".as_bytes());

    let mut cursor = Cursor::new(buffer);
    let (deserialized, len) = StructWithLifetime::unpack(&mut cursor).unwrap();
    assert_eq!(len, 12);
    assert_eq!(original, deserialized);
}

#[derive(Debug, PartialEq, XdrCodec)]
struct TupleStruct(u32, u32);

#[test]
fn test_tuple_struct_serialization() {
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

#[derive(Debug, PartialEq, XdrCodec)]
struct UnitStruct;

#[test]
fn test_unit_struct_serialization() {
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
