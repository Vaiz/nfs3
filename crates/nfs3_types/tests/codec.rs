#![allow(clippy::unwrap_used)]

use std::borrow::Cow;
use std::io::Cursor;

use nfs3_types::nfs3::{Nfs3Option, Nfs3Result, nfsstat3};
use nfs3_types::xdr_codec::{List, Opaque, Pack, Unpack, Void, XdrCodec};

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
        [
            0x00, 0x00, 0x07, 0x89, 0x00, 0x00, 0x10, 0x11, 0x00, 0x00, 0x00, 0x01
        ]
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

#[test]
fn test_vec_serialization() {
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
fn test_list_serialization() {
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
fn test_empty_dirlist3_serialization() {
    use nfs3_types::nfs3::dirlist3;

    let original = dirlist3 {
        entries: List(vec![]),
        eof: true,
    };

    let mut buffer = Vec::new();
    let len = original.pack(&mut buffer).unwrap();
    assert_eq!(original.packed_size(), 8); // 4 bytes for entries + 4 bytes for eof
    assert_eq!(len, 8);
    assert_eq!(buffer, [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01]);

    let mut cursor = Cursor::new(buffer);
    let (deserialized, len) = dirlist3::unpack(&mut cursor).unwrap();
    assert_eq!(len, 8);
    assert!(deserialized.entries.0.is_empty());
    assert_eq!(original.eof, deserialized.eof);
}

#[test]
fn test_dirlist3_with_entries_serialization() {
    use nfs3_types::nfs3::{dirlist3, entry3, filename3};

    let original = dirlist3 {
        entries: List(vec![
            entry3 {
                fileid: 0x1234,
                name: filename3::from(&b"file1"[..]),
                cookie: 0x5678,
            },
            entry3 {
                fileid: 0x9abc,
                name: filename3::from(&b"file2"[..]),
                cookie: 0xdef0,
            },
        ]),
        eof: false,
    };

    let mut buffer = Vec::new();
    let len = original.pack(&mut buffer).unwrap();
    assert_eq!(original.packed_size(), 72); // 2 entries * (4 + 4 + 8 + 4) + 4 for eof
    assert_eq!(len, 72);
    assert_eq!(
        buffer,
        [
            0x00, 0x00, 0x00, 0x01, // first entry
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x12, 0x34, // fileid
            0x00, 0x00, 0x00, 0x05, // name length
            0x66, 0x69, 0x6c, 0x65, 0x31, 0x00, 0x00, 0x00, // name "file1\0\0\0"
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x56, 0x78, // cookie
            0x00, 0x00, 0x00, 0x01, // second entry
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x9a, 0xbc, // fileid
            0x00, 0x00, 0x00, 0x05, // name length
            0x66, 0x69, 0x6c, 0x65, 0x32, 0x00, 0x00, 0x00, // name "file2\0\0\0"
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xde, 0xf0, // cookie
            0x00, 0x00, 0x00, 0x00, // end of list
            0x00, 0x00, 0x00, 0x00, // eof
        ]
    );

    let mut cursor = Cursor::new(buffer);
    let (deserialized, len) = dirlist3::unpack(&mut cursor).unwrap();
    assert_eq!(len, 72);
    assert_eq!(original.entries.0, deserialized.entries.0);
    assert_eq!(original.eof, deserialized.eof);
}

#[test]
fn test_bounded_list() {
    // (4+4)*3 + 4 = 28 bytes
    let mut bounded_list = nfs3_types::xdr_codec::BoundedList::<u32>::new(28);
    assert!(bounded_list.try_push(0x1234).is_ok());
    assert!(bounded_list.try_push(0x5678).is_ok());
    assert!(bounded_list.try_push(0x9abc).is_ok());
    assert!(bounded_list.try_push(0xdef0).is_err());

    let list = bounded_list.into_inner();
    assert_eq!(list.0, vec![0x1234, 0x5678, 0x9abc]);

    let mut bounded_list = nfs3_types::xdr_codec::BoundedList::<u32>::new(27);
    assert!(bounded_list.try_push(0x1234).is_ok());
    assert!(bounded_list.try_push(0x5678).is_ok());
    assert!(bounded_list.try_push(0x9abc).is_err());

    let list = bounded_list.into_inner();
    assert_eq!(list.0, vec![0x1234, 0x5678]);

    let mut bounded_list = nfs3_types::xdr_codec::BoundedList::<u32>::new(29);
    assert!(bounded_list.try_push(0x1234).is_ok());
    assert!(bounded_list.try_push(0x5678).is_ok());
    assert!(bounded_list.try_push(0x9abc).is_ok());
    assert!(bounded_list.try_push(0xdef0).is_err());

    let list = bounded_list.into_inner();
    assert_eq!(list.0, vec![0x1234, 0x5678, 0x9abc]);
}

#[test]
fn test_rpc_call_len() {
    use nfs3_types::rpc::{RPC_VERSION_2, call_body, msg_body, opaque_auth, rpc_msg};

    let call = call_body {
        rpcvers: RPC_VERSION_2,
        prog: 100_003,
        vers: 3,
        proc: 0,
        cred: opaque_auth::default(),
        verf: opaque_auth::default(),
    };

    assert_eq!(call.packed_size(), 32);

    let msg = rpc_msg {
        xid: 123,
        body: msg_body::CALL(call),
    };

    assert_eq!(msg.packed_size(), 40);
}

#[test]
fn test_nfs3_result() {
    // Test for a successful result with no additional data
    let original = Nfs3Result::<Void, Void>::Ok(Void);
    let mut buffer = Vec::new();
    let len = original.pack(&mut buffer).unwrap();
    assert_eq!(len, 4);
    assert_eq!(original.packed_size(), len); // 4 bytes for the status
    assert_eq!(buffer, [0x00, 0x00, 0x00, 0x00]);

    // Test for an error result with no additional data
    let original = Nfs3Result::<Void, Void>::Err((nfsstat3::NFS3ERR_IO, Void));
    let mut buffer = Vec::new();
    let len = original.pack(&mut buffer).unwrap();
    assert_eq!(len, 4);
    assert_eq!(original.packed_size(), len); // 4 bytes for the status
    assert_eq!(buffer, [0x00, 0x00, 0x00, 0x05]);

    // Test for a successful result with additional data
    let original = Nfs3Result::<u32, u32>::Ok(0x1234_5678u32);
    let mut buffer = Vec::new();
    let len = original.pack(&mut buffer).unwrap();
    assert_eq!(len, 8); // 4 bytes for the status + 4 bytes for the data
    assert_eq!(original.packed_size(), len); // 4 bytes for the status + 4 bytes for the data
    assert_eq!(buffer, [0x00, 0x00, 0x00, 0x00, 0x12, 0x34, 0x56, 0x78]);

    // Test for an error result with additional data
    let original = Nfs3Result::<u32, u32>::Err((nfsstat3::NFS3ERR_IO, 0x8765_4321u32));
    let mut buffer = Vec::new();
    let len = original.pack(&mut buffer).unwrap();
    assert_eq!(len, 8); // 4 bytes for the status + 4 bytes for the data
    assert_eq!(original.packed_size(), len); // 4 bytes for the status + 4 bytes for the data
    assert_eq!(buffer, [0x00, 0x00, 0x00, 0x05, 0x87, 0x65, 0x43, 0x21]);
}

#[test]
fn test_nfs3_option() {
    // Test for a successful result with no additional data
    let original = Nfs3Option::<Void>::Some(Void);
    let mut buffer = Vec::new();
    let len = original.pack(&mut buffer).unwrap();
    assert_eq!(len, 4);
    assert_eq!(original.packed_size(), len); // 4 bytes for the status
    assert_eq!(buffer, [0x00, 0x00, 0x00, 0x01]);

    // Test for None result with Void
    let original = Nfs3Option::<Void>::None;
    let mut buffer = Vec::new();
    let len = original.pack(&mut buffer).unwrap();
    assert_eq!(len, 4);
    assert_eq!(original.packed_size(), len); // 4 bytes for the status
    assert_eq!(buffer, [0x00, 0x00, 0x00, 0x00]);

    // Test for a successful result with additional data
    let original = Nfs3Option::<u32>::Some(0x1234_5678u32);
    let mut buffer = Vec::new();
    let len = original.pack(&mut buffer).unwrap();
    assert_eq!(len, 8); // 4 bytes for the status + 4 bytes for the data
    assert_eq!(original.packed_size(), len); // 4 bytes for the status + 4 bytes for the data
    assert_eq!(buffer, [0x00, 0x00, 0x00, 0x01, 0x12, 0x34, 0x56, 0x78]);

    // Test for None result with additional u32
    let original = Nfs3Option::<u32>::None;
    let mut buffer = Vec::new();
    let len = original.pack(&mut buffer).unwrap();
    assert_eq!(len, 4); // 4 bytes for the status
    assert_eq!(original.packed_size(), len); // 4 bytes for the status
    assert_eq!(buffer, [0x00, 0x00, 0x00, 0x00]);
}

#[test]
fn test_mount3res_packed_size() {
    use nfs3_types::mount::{fhandle3, mountres3, mountres3_ok, mountstat3};

    // Test for a successful mountres3 with no additional data
    let res = mountres3::Ok(mountres3_ok {
        fhandle: fhandle3(Opaque(Cow::Borrowed(&[0x12, 0x34, 0x56, 0x78]))),
        auth_flavors: vec![1, 2, 3],
    });
    let mut buffer = Vec::new();
    let len = res.pack(&mut buffer).unwrap();
    assert_eq!(len, 28); // 4 bytes for the status + 8 bytes for fhandle + 16 bytes for auth_flavors
    assert_eq!(res.packed_size(), len); // 4 bytes for the status + 4 bytes for fhandle + 4 bytes for auth_flavors
    assert_eq!(
        buffer,
        [
            0x00, 0x00, 0x00, 0x00, // mountstat3::MNT3_OK
            0x00, 0x00, 0x00, 0x04, // fhandle length
            0x12, 0x34, 0x56, 0x78, // fhandle data
            0x00, 0x00, 0x00, 0x03, // number of auth flavors
            0x00, 0x00, 0x00, 0x01, // auth flavor 1
            0x00, 0x00, 0x00, 0x02, // auth flavor 2
            0x00, 0x00, 0x00, 0x03, // auth flavor 3
        ]
    );

    // Test for an error mountres3 with no additional data
    let res = mountres3::Err(mountstat3::MNT3ERR_PERM);
    let mut buffer = Vec::new();
    let len = res.pack(&mut buffer).unwrap();
    assert_eq!(len, 4); // 4 bytes for the status
    assert_eq!(res.packed_size(), len); // 4 bytes for the status
    assert_eq!(buffer, [0x00, 0x00, 0x00, 0x01]); // mountstat3::MNT3ERR_PERM    
}
