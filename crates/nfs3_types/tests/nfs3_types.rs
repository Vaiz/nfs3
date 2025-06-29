// Tests for NFS3-specific types and serialization
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use nfs3_types::nfs3::{Nfs3Option, Nfs3Result, dirlist3, entry3, filename3, nfsstat3};
use nfs3_types::xdr_codec::{List, Pack, Unpack, Void};

#[test]
fn nfs3_result_success_void() {
    // Test for a successful result with no additional data
    let original = Nfs3Result::<Void, Void>::Ok(Void);
    let mut buffer = Vec::new();
    let len = original.pack(&mut buffer).unwrap();
    assert_eq!(len, 4);
    assert_eq!(original.packed_size(), len); // 4 bytes for the status
    assert_eq!(buffer, [0x00, 0x00, 0x00, 0x00]);
}

#[test]
fn nfs3_result_error_void() {
    // Test for an error result with no additional data
    let original = Nfs3Result::<Void, Void>::Err((nfsstat3::NFS3ERR_IO, Void));
    let mut buffer = Vec::new();
    let len = original.pack(&mut buffer).unwrap();
    assert_eq!(len, 4);
    assert_eq!(original.packed_size(), len); // 4 bytes for the status
    assert_eq!(buffer, [0x00, 0x00, 0x00, 0x05]);
}

#[test]
fn nfs3_result_success_with_data() {
    // Test for a successful result with additional data
    let original = Nfs3Result::<u32, u32>::Ok(0x1234_5678u32);
    let mut buffer = Vec::new();
    let len = original.pack(&mut buffer).unwrap();
    assert_eq!(len, 8); // 4 bytes for the status + 4 bytes for the data
    assert_eq!(original.packed_size(), len); // 4 bytes for the status + 4 bytes for the data
    assert_eq!(buffer, [0x00, 0x00, 0x00, 0x00, 0x12, 0x34, 0x56, 0x78]);
}

#[test]
fn nfs3_result_error_with_data() {
    // Test for an error result with additional data
    let original = Nfs3Result::<u32, u32>::Err((nfsstat3::NFS3ERR_IO, 0x8765_4321u32));
    let mut buffer = Vec::new();
    let len = original.pack(&mut buffer).unwrap();
    assert_eq!(len, 8); // 4 bytes for the status + 4 bytes for the data
    assert_eq!(original.packed_size(), len); // 4 bytes for the status + 4 bytes for the data
    assert_eq!(buffer, [0x00, 0x00, 0x00, 0x05, 0x87, 0x65, 0x43, 0x21]);
}

#[test]
fn nfs3_option_some_void() {
    // Test for a successful result with no additional data
    let original = Nfs3Option::<Void>::Some(Void);
    let mut buffer = Vec::new();
    let len = original.pack(&mut buffer).unwrap();
    assert_eq!(len, 4);
    assert_eq!(original.packed_size(), len); // 4 bytes for the status
    assert_eq!(buffer, [0x00, 0x00, 0x00, 0x01]);
}

#[test]
fn nfs3_option_none_void() {
    // Test for None result with Void
    let original = Nfs3Option::<Void>::None;
    let mut buffer = Vec::new();
    let len = original.pack(&mut buffer).unwrap();
    assert_eq!(len, 4);
    assert_eq!(original.packed_size(), len); // 4 bytes for the status
    assert_eq!(buffer, [0x00, 0x00, 0x00, 0x00]);
}

#[test]
fn nfs3_option_some_with_data() {
    // Test for a successful result with additional data
    let original = Nfs3Option::<u32>::Some(0x1234_5678u32);
    let mut buffer = Vec::new();
    let len = original.pack(&mut buffer).unwrap();
    assert_eq!(len, 8); // 4 bytes for the status + 4 bytes for the data
    assert_eq!(original.packed_size(), len); // 4 bytes for the status + 4 bytes for the data
    assert_eq!(buffer, [0x00, 0x00, 0x00, 0x01, 0x12, 0x34, 0x56, 0x78]);
}

#[test]
fn nfs3_option_none_with_data() {
    // Test for None result with additional u32
    let original = Nfs3Option::<u32>::None;
    let mut buffer = Vec::new();
    let len = original.pack(&mut buffer).unwrap();
    assert_eq!(len, 4); // 4 bytes for the status
    assert_eq!(original.packed_size(), len); // 4 bytes for the status
    assert_eq!(buffer, [0x00, 0x00, 0x00, 0x00]);
}

#[test]
fn empty_dirlist3_serialization() {
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
fn dirlist3_with_entries_serialization() {
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
fn filename3_edge_cases() {
    // Test empty filename
    let empty_name = filename3::from(&b""[..]);
    let mut buffer = Vec::new();
    let len = empty_name.pack(&mut buffer).unwrap();
    assert_eq!(len, 4); // Just the length field
    assert_eq!(buffer, [0, 0, 0, 0]);

    // Test single character filename
    let single_char = filename3::from(&b"a"[..]);
    let mut buffer = Vec::new();
    let len = single_char.pack(&mut buffer).unwrap();
    assert_eq!(len, 8); // 4 bytes length + 1 byte data + 3 bytes padding
    assert_eq!(buffer, [0, 0, 0, 1, b'a', 0, 0, 0]);

    // Test filename requiring no padding (multiple of 4)
    let quad_name = filename3::from(&b"test"[..]);
    let mut buffer = Vec::new();
    let len = quad_name.pack(&mut buffer).unwrap();
    assert_eq!(len, 8); // 4 bytes length + 4 bytes data
    assert_eq!(buffer, [0, 0, 0, 4, b't', b'e', b's', b't']);
}

#[test]
fn nfs3_result_roundtrip() {
    // Test void cases
    let void_cases = [
        Nfs3Result::<Void, Void>::Ok(Void),
        Nfs3Result::<Void, Void>::Err((nfsstat3::NFS3ERR_PERM, Void)),
    ];

    for original in void_cases {
        let mut buffer = Vec::new();
        let pack_len = original.pack(&mut buffer).unwrap();
        assert_eq!(pack_len, original.packed_size());

        let mut cursor = Cursor::new(buffer);
        let (deserialized, unpack_len) = Nfs3Result::<Void, Void>::unpack(&mut cursor).unwrap();
        assert_eq!(pack_len, unpack_len);
        assert_eq!(original, deserialized);
    }

    // Test u32 cases
    let u32_cases = [
        Nfs3Result::<u32, u32>::Ok(0x1234_5678),
        Nfs3Result::<u32, u32>::Err((nfsstat3::NFS3ERR_NOENT, 0x8765_4321)),
    ];

    for original in u32_cases {
        let mut buffer = Vec::new();
        let pack_len = original.pack(&mut buffer).unwrap();
        assert_eq!(pack_len, original.packed_size());

        let mut cursor = Cursor::new(buffer);
        let (deserialized, unpack_len) = Nfs3Result::<u32, u32>::unpack(&mut cursor).unwrap();
        assert_eq!(pack_len, unpack_len);
        assert_eq!(original, deserialized);
    }
}

#[test]
fn nfs3_error_codes() {
    // Test various NFS3 error codes (excluding NFS3_OK which is success, not error)
    let error_codes = [
        nfsstat3::NFS3ERR_PERM,
        nfsstat3::NFS3ERR_NOENT,
        nfsstat3::NFS3ERR_IO,
        nfsstat3::NFS3ERR_NXIO,
        nfsstat3::NFS3ERR_ACCES,
        nfsstat3::NFS3ERR_EXIST,
        nfsstat3::NFS3ERR_XDEV,
        nfsstat3::NFS3ERR_NODEV,
        nfsstat3::NFS3ERR_NOTDIR,
        nfsstat3::NFS3ERR_ISDIR,
        nfsstat3::NFS3ERR_INVAL,
        nfsstat3::NFS3ERR_FBIG,
        nfsstat3::NFS3ERR_NOSPC,
        nfsstat3::NFS3ERR_ROFS,
        nfsstat3::NFS3ERR_MLINK,
        nfsstat3::NFS3ERR_NAMETOOLONG,
        nfsstat3::NFS3ERR_NOTEMPTY,
        nfsstat3::NFS3ERR_DQUOT,
        nfsstat3::NFS3ERR_STALE,
        nfsstat3::NFS3ERR_REMOTE,
        nfsstat3::NFS3ERR_BADHANDLE,
        nfsstat3::NFS3ERR_NOT_SYNC,
        nfsstat3::NFS3ERR_BAD_COOKIE,
        nfsstat3::NFS3ERR_NOTSUPP,
        nfsstat3::NFS3ERR_TOOSMALL,
        nfsstat3::NFS3ERR_SERVERFAULT,
        nfsstat3::NFS3ERR_BADTYPE,
        nfsstat3::NFS3ERR_JUKEBOX,
    ];

    for error_code in error_codes {
        let result = Nfs3Result::<Void, Void>::Err((error_code, Void));
        let mut buffer = Vec::new();
        let len = result.pack(&mut buffer).unwrap();
        assert_eq!(len, 4);

        let mut cursor = Cursor::new(buffer);
        let (deserialized, _) = Nfs3Result::<Void, Void>::unpack(&mut cursor).unwrap();
        assert_eq!(result, deserialized);
    }

    // Test NFS3_OK separately as a success case
    let success_result = Nfs3Result::<Void, Void>::Ok(Void);
    let mut buffer = Vec::new();
    let len = success_result.pack(&mut buffer).unwrap();
    assert_eq!(len, 4);

    let mut cursor = Cursor::new(buffer);
    let (deserialized, _) = Nfs3Result::<Void, Void>::unpack(&mut cursor).unwrap();
    deserialized.expect("Expected success result for NFS3_OK");
}
