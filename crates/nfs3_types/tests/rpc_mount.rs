// Tests for RPC and Mount protocol types
#![allow(clippy::unwrap_used)]

use std::borrow::Cow;
use std::io::Cursor;

use nfs3_types::mount::{fhandle3, mountres3, mountres3_ok, mountstat3};
use nfs3_types::rpc::{RPC_VERSION_2, auth_flavor, call_body, msg_body, opaque_auth, rpc_msg};
use nfs3_types::xdr_codec::{Opaque, Pack, Unpack};

#[test]
fn rpc_call_len() {
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
fn rpc_call_serialization() {
    let call = call_body {
        rpcvers: RPC_VERSION_2,
        prog: 100_003,
        vers: 3,
        proc: 0,
        cred: opaque_auth::default(),
        verf: opaque_auth::default(),
    };

    let mut buffer = Vec::new();
    let len = call.pack(&mut buffer).unwrap();
    assert_eq!(len, 32);
    assert_eq!(call.packed_size(), len);

    let mut cursor = Cursor::new(buffer);
    let (deserialized, unpack_len) = call_body::unpack(&mut cursor).unwrap();
    assert_eq!(len, unpack_len);
    assert_eq!(call.rpcvers, deserialized.rpcvers);
    assert_eq!(call.prog, deserialized.prog);
    assert_eq!(call.vers, deserialized.vers);
    assert_eq!(call.proc, deserialized.proc);
}

#[test]
fn rpc_msg_serialization() {
    let call = call_body {
        rpcvers: RPC_VERSION_2,
        prog: 100_003,
        vers: 3,
        proc: 0,
        cred: opaque_auth::default(),
        verf: opaque_auth::default(),
    };

    let msg = rpc_msg {
        xid: 123,
        body: msg_body::CALL(call),
    };

    let mut buffer = Vec::new();
    let len = msg.pack(&mut buffer).unwrap();
    assert_eq!(len, 40);
    assert_eq!(msg.packed_size(), len);

    let mut cursor = Cursor::new(buffer);
    let (deserialized, unpack_len) = rpc_msg::unpack(&mut cursor).unwrap();
    assert_eq!(len, unpack_len);
    assert_eq!(msg.xid, deserialized.xid);
    match (&msg.body, &deserialized.body) {
        (msg_body::CALL(call1), msg_body::CALL(call2)) => {
            assert_eq!(call1.rpcvers, call2.rpcvers);
            assert_eq!(call1.prog, call2.prog);
            assert_eq!(call1.vers, call2.vers);
            assert_eq!(call1.proc, call2.proc);
        }
        _ => panic!("Expected CALL message body"),
    }
}

#[test]
fn mount3res_success_packed_size() {
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
}

#[test]
fn mount3res_error_packed_size() {
    // Test for an error mountres3 with no additional data
    let res = mountres3::Err(mountstat3::MNT3ERR_PERM);
    let mut buffer = Vec::new();
    let len = res.pack(&mut buffer).unwrap();
    assert_eq!(len, 4); // 4 bytes for the status
    assert_eq!(res.packed_size(), len); // 4 bytes for the status
    assert_eq!(buffer, [0x00, 0x00, 0x00, 0x01]); // mountstat3::MNT3ERR_PERM    
}

#[test]
fn mount3res_roundtrip() {
    // Test successful mount response
    let success_res = mountres3::Ok(mountres3_ok {
        fhandle: fhandle3(Opaque(Cow::Borrowed(&[0x12, 0x34, 0x56, 0x78]))),
        auth_flavors: vec![1, 2, 3],
    });

    let mut buffer = Vec::new();
    let pack_len = success_res.pack(&mut buffer).unwrap();
    assert_eq!(pack_len, success_res.packed_size());

    let mut cursor = Cursor::new(buffer);
    let (deserialized, unpack_len) = mountres3::unpack(&mut cursor).unwrap();
    assert_eq!(pack_len, unpack_len);

    match (&success_res, &deserialized) {
        (mountres3::Ok(ok1), mountres3::Ok(ok2)) => {
            assert_eq!(ok1.fhandle.0.as_ref(), ok2.fhandle.0.as_ref());
            assert_eq!(ok1.auth_flavors, ok2.auth_flavors);
        }
        _ => panic!("Expected success response"),
    }

    // Test error mount response
    let error_res = mountres3::Err(mountstat3::MNT3ERR_PERM);
    let mut buffer = Vec::new();
    let pack_len = error_res.pack(&mut buffer).unwrap();
    assert_eq!(pack_len, error_res.packed_size());

    let mut cursor = Cursor::new(buffer);
    let (deserialized, unpack_len) = mountres3::unpack(&mut cursor).unwrap();
    assert_eq!(pack_len, unpack_len);
    match (&error_res, &deserialized) {
        (mountres3::Err(err1), mountres3::Err(err2)) => {
            // Compare error codes by their discriminant values
            assert_eq!(*err1 as u32, *err2 as u32);
        }
        _ => panic!("Expected error response"),
    }
}

#[test]
fn fhandle3_edge_cases() {
    // Test empty file handle
    let empty_fh = fhandle3(Opaque(Cow::Borrowed(&[])));
    let mut buffer = Vec::new();
    let len = empty_fh.pack(&mut buffer).unwrap();
    assert_eq!(len, 4); // Just the length field
    assert_eq!(buffer, [0, 0, 0, 0]);

    // Test maximum size file handle (64 bytes)
    let max_data = vec![0x42; 64];
    let max_fh = fhandle3(Opaque(Cow::Borrowed(&max_data)));
    let mut buffer = Vec::new();
    let len = max_fh.pack(&mut buffer).unwrap();
    assert_eq!(len, 68); // 4 bytes length + 64 bytes data
}

#[test]
fn opaque_auth_default() {
    let auth = opaque_auth::default();
    let mut buffer = Vec::new();
    let len = auth.pack(&mut buffer).unwrap();
    assert_eq!(len, 8); // 4 bytes flavor + 4 bytes length (no data)
    assert_eq!(buffer, [0, 0, 0, 0, 0, 0, 0, 0]);

    let mut cursor = Cursor::new(buffer);
    let (deserialized, unpack_len) = opaque_auth::unpack(&mut cursor).unwrap();
    assert_eq!(len, unpack_len);
    assert_eq!(auth.flavor, deserialized.flavor);
    assert_eq!(auth.body.as_ref(), deserialized.body.as_ref());
}

#[test]
fn opaque_auth_with_data() {
    let auth = opaque_auth {
        flavor: auth_flavor::AUTH_UNIX,
        body: Opaque(Cow::Borrowed(&[0x12, 0x34, 0x56, 0x78])),
    };

    let mut buffer = Vec::new();
    let len = auth.pack(&mut buffer).unwrap();
    assert_eq!(len, 12); // 4 bytes flavor + 4 bytes length + 4 bytes data
    assert_eq!(buffer, [0, 0, 0, 1, 0, 0, 0, 4, 0x12, 0x34, 0x56, 0x78]);

    let mut cursor = Cursor::new(buffer);
    let (deserialized, unpack_len) = opaque_auth::unpack(&mut cursor).unwrap();
    assert_eq!(len, unpack_len);
    assert_eq!(auth.flavor, deserialized.flavor);
    assert_eq!(auth.body.as_ref(), deserialized.body.as_ref());
}

#[test]
fn mount_error_codes() {
    let error_codes = [
        mountstat3::MNT3ERR_PERM,
        mountstat3::MNT3ERR_NOENT,
        mountstat3::MNT3ERR_IO,
        mountstat3::MNT3ERR_ACCES,
        mountstat3::MNT3ERR_NOTDIR,
        mountstat3::MNT3ERR_INVAL,
        mountstat3::MNT3ERR_NAMETOOLONG,
        mountstat3::MNT3ERR_NOTSUPP,
        mountstat3::MNT3ERR_SERVERFAULT,
    ];

    for error_code in error_codes {
        let res = mountres3::Err(error_code);
        let mut buffer = Vec::new();
        let len = res.pack(&mut buffer).unwrap();
        assert_eq!(len, 4);

        let mut cursor = Cursor::new(buffer);
        let (deserialized, _) = mountres3::unpack(&mut cursor).unwrap();
        match deserialized {
            mountres3::Err(deserialized_code) => {
                assert_eq!(error_code as u32, deserialized_code as u32);
            }
            mountres3::Ok(_) => panic!("Expected error result for error code {error_code:?}"),
        }
    }

    // Test MNT3_OK separately as a success case would require a valid mount response
    // which is more complex, so we'll skip it in this simple error code test
}

#[test]
fn rpc_program_constants() {
    // Test that common RPC program numbers are correctly defined
    assert_eq!(RPC_VERSION_2, 2);

    // Test typical NFS program numbers (these might be defined elsewhere)
    let call = call_body {
        rpcvers: RPC_VERSION_2,
        prog: 100_003, // NFS program number
        vers: 3,       // NFS version 3
        proc: 0,       // NULL procedure
        cred: opaque_auth::default(),
        verf: opaque_auth::default(),
    };

    assert_eq!(call.prog, 100_003);
    assert_eq!(call.vers, 3);
    assert_eq!(call.rpcvers, RPC_VERSION_2);
}

#[test]
fn auth_flavors() {
    // Test different authentication flavors
    let auth_flavors = [
        auth_flavor::AUTH_NULL,
        auth_flavor::AUTH_UNIX,
        auth_flavor::AUTH_SHORT,
        auth_flavor::AUTH_DES,
    ];

    for flavor in auth_flavors {
        let auth = opaque_auth {
            flavor,
            body: Opaque(Cow::Borrowed(&[])),
        };

        let mut buffer = Vec::new();
        let len = auth.pack(&mut buffer).unwrap();
        assert_eq!(len, 8); // 4 bytes flavor + 4 bytes length

        let mut cursor = Cursor::new(buffer);
        let (deserialized, _) = opaque_auth::unpack(&mut cursor).unwrap();
        assert_eq!(auth.flavor, deserialized.flavor);
    }
}
