// Tests for RPC and Mount protocol types

use super::encode;
use crate::mount::{dirpath, fhandle3, mountres3, mountres3_ok, mountstat3};
use crate::nfs3::nfs_fh3;
use crate::rpc::{
    RPC_VERSION_2, auth_flavor, auth_unix, call_body, msg_body, opaque_auth, rpc_msg,
};
use crate::xdr_codec::{BoundedOpaque, BoundedVec, Decoder, Encoder, Error, Opaque, Pack, Unpack};

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
    let buffer = encode(&call).unwrap().to_vec();
    let len = buffer.len();
    assert_eq!(len, 32);
    assert_eq!(call.packed_size(), len);

    let mut cursor = Decoder::copying(buffer);
    let deserialized = call_body::unpack(&mut cursor).unwrap();
    assert!(cursor.is_empty());
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
    let buffer = encode(&msg).unwrap().to_vec();
    let len = buffer.len();
    assert_eq!(len, 40);
    assert_eq!(msg.packed_size(), len);

    let mut cursor = Decoder::copying(buffer);
    let deserialized = rpc_msg::unpack(&mut cursor).unwrap();
    assert!(cursor.is_empty());
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
        fhandle: fhandle3(BoundedOpaque::from_array(&[0x12, 0x34, 0x56, 0x78])),
        auth_flavors: vec![1, 2, 3].try_into().unwrap(),
    });
    let buffer = encode(&res).unwrap().to_vec();
    let len = buffer.len();
    assert_eq!(len, 28); // 4 bytes for the status + 8 bytes for fhandle + 16 bytes for auth_flavors
    assert_eq!(res.packed_size(), len);
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
    let buffer = encode(&res).unwrap().to_vec();
    let len = buffer.len();
    assert_eq!(len, 4); // 4 bytes for the status
    assert_eq!(res.packed_size(), len); // 4 bytes for the status
    assert_eq!(buffer, [0x00, 0x00, 0x00, 0x01]); // mountstat3::MNT3ERR_PERM    
}

#[test]
fn mount3res_roundtrip() {
    // Test successful mount response
    let success_res = mountres3::Ok(mountres3_ok {
        fhandle: fhandle3(BoundedOpaque::from_array(&[0x12, 0x34, 0x56, 0x78])),
        auth_flavors: vec![1, 2, 3].try_into().unwrap(),
    });
    let buffer = encode(&success_res).unwrap().to_vec();
    let pack_len = buffer.len();
    assert_eq!(pack_len, success_res.packed_size());

    let mut cursor = Decoder::copying(buffer);
    let deserialized = mountres3::unpack(&mut cursor).unwrap();
    assert!(cursor.is_empty());

    match (&success_res, &deserialized) {
        (mountres3::Ok(ok1), mountres3::Ok(ok2)) => {
            assert_eq!(ok1.fhandle.0.as_ref(), ok2.fhandle.0.as_ref());
            assert_eq!(ok1.auth_flavors, ok2.auth_flavors);
        }
        _ => panic!("Expected success response"),
    }

    // Test error mount response
    let error_res = mountres3::Err(mountstat3::MNT3ERR_PERM);
    let buffer = encode(&error_res).unwrap().to_vec();
    let pack_len = buffer.len();
    assert_eq!(pack_len, error_res.packed_size());

    let mut cursor = Decoder::copying(buffer);
    let deserialized = mountres3::unpack(&mut cursor).unwrap();
    assert!(cursor.is_empty());
    match (&error_res, &deserialized) {
        (mountres3::Err(err1), mountres3::Err(err2)) => {
            // Compare error codes by their discriminant values
            assert_eq!(*err1 as u32, *err2 as u32);
        }
        _ => panic!("Expected error response"),
    }
}

#[test]
fn mount3res_accepts_twelve_auth_flavors() {
    let flavors = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
    let response = mountres3::Ok(mountres3_ok {
        fhandle: fhandle3(BoundedOpaque::from_array(&[0x12, 0x34, 0x56, 0x78])),
        auth_flavors: BoundedVec::from_array(flavors),
    });
    let mut input = Decoder::copying(encode(&response).unwrap());
    let mountres3::Ok(response) = mountres3::unpack(&mut input).unwrap() else {
        panic!("Expected success response");
    };
    assert_eq!(response.auth_flavors.as_ref(), flavors);
    assert!(input.is_empty());
}

#[test]
fn mount3res_rejects_thirteen_auth_flavors_before_reading_items() {
    let mut wire = vec![
        0, 0, 0, 0, // MNT3_OK
        0, 0, 0, 4, // fhandle length
        0x12, 0x34, 0x56, 0x78, // fhandle data
        0, 0, 0, 13, // auth flavor count
    ];
    wire.extend((0_u32..13).flat_map(u32::to_be_bytes));
    let mut input = Decoder::copying(wire);
    assert_eq!(
        mountres3::unpack(&mut input).unwrap_err(),
        Error::InvalidLength(13)
    );
    assert_eq!(input.remaining(), 13 * 4);
}

#[test]
fn fhandle3_edge_cases() {
    // Test empty file handle
    let empty_fh = fhandle3(BoundedOpaque::empty());
    let buffer = encode(&empty_fh).unwrap().to_vec();
    let len = buffer.len();
    assert_eq!(len, 4); // Just the length field
    assert_eq!(buffer, [0, 0, 0, 0]);

    // Test maximum size file handle (64 bytes)
    let max_data = vec![0x42; 64];
    let max_fh = fhandle3(BoundedOpaque::from_slice(&max_data).unwrap());
    let buffer = encode(&max_fh).unwrap().to_vec();
    let len = buffer.len();
    assert_eq!(len, 68); // 4 bytes length + 64 bytes data
}

#[test]
fn small_bounded_opaque_is_stored_inline() {
    fn assert_inline(value: &BoundedOpaque<'_, 64>) {
        let start = std::ptr::from_ref(value).addr();
        let payload = value.as_ptr().addr();
        assert!((start..start + size_of_val(value)).contains(&payload));
        assert!(payload + value.len() <= start + size_of_val(value));
    }

    let wire = &[0, 0, 0, 3, b'a', b'b', b'c', 0][..];
    let mut input = Decoder::copying(wire);
    let decoded = BoundedOpaque::<64>::unpack(&mut input).unwrap();
    assert!(input.is_empty());
    assert_inline(&decoded);
    assert_eq!(encode(&decoded).unwrap(), wire);

    let owned = BoundedOpaque::<64>::from_array(b"abc").into_owned();
    assert_inline(&owned);
    assert_eq!(decoded, owned);
    assert_eq!(owned.into_bytes(), &b"abc"[..]);

    let limit = [0xfe; 22];
    let owned = BoundedOpaque::<64>::from_array(&limit).into_owned();
    assert_inline(&owned);
    let wire = encode(&owned).unwrap();
    let decoded = BoundedOpaque::<64>::unpack(&mut Decoder::copying(wire)).unwrap();
    assert_inline(&decoded);
    assert_eq!(decoded, owned);

    let larger = [0xff; 23];
    let wire = encode(&Opaque::borrowed(&larger)).unwrap();
    let decoded = BoundedOpaque::<64>::unpack(&mut Decoder::copying(wire)).unwrap();
    assert_eq!(decoded.as_ref(), larger);
}

#[cfg(target_arch = "x86_64")]
#[test]
fn bounded_opaque_does_not_enlarge_x64_values() {
    assert_eq!(size_of::<BoundedOpaque<'static, 64>>(), 32);
    assert_eq!(size_of::<BoundedOpaque<'static, 1024>>(), 32);
    assert_eq!(size_of::<crate::rpc::rpc_msg<'static, 'static>>(), 104);
}

#[test]
fn small_bounded_opaque_checks_padding() {
    let mut truncated = Decoder::copying(&[0, 0, 0, 3, b'a', b'b', b'c'][..]);
    assert_eq!(
        BoundedOpaque::<64>::unpack(&mut truncated),
        Err(Error::InvalidLength(3))
    );
    let mut nonzero = Decoder::copying(&[0, 0, 0, 3, b'a', b'b', b'c', 1][..]);
    assert_eq!(
        BoundedOpaque::<64>::unpack(&mut nonzero),
        Err(Error::NonZeroPadding)
    );
}

#[test]
fn file_handle_and_mount_field_limits() {
    fn check<T: Pack + Unpack>(max: usize) {
        let valid = encode(&Opaque::owned(vec![42; max])).unwrap();
        let mut input = Decoder::copying(valid.clone());
        let field = T::unpack(&mut input).unwrap();
        assert!(input.is_empty());
        assert_eq!(encode(&field).unwrap(), valid);

        let oversized = encode(&Opaque::owned(vec![42; max + 1])).unwrap();
        let mut input = Decoder::copying(oversized.clone());
        assert!(matches!(T::unpack(&mut input), Err(Error::InvalidLength(n)) if n == max + 1));
        assert_eq!(input.into_remaining(), oversized.slice(4..));
    }
    check::<nfs_fh3>(64);
    check::<fhandle3<'static>>(64);
    check::<dirpath<'static>>(1024);
    check::<crate::mount::name<'static>>(255);
}

#[test]
fn opaque_auth_default() {
    let auth = opaque_auth::default();
    let buffer = encode(&auth).unwrap().to_vec();
    let len = buffer.len();
    assert_eq!(len, 8); // 4 bytes flavor + 4 bytes length (no data)
    assert_eq!(buffer, [0, 0, 0, 0, 0, 0, 0, 0]);

    let mut cursor = Decoder::copying(buffer);
    let deserialized = opaque_auth::unpack(&mut cursor).unwrap();
    assert!(cursor.is_empty());
    assert_eq!(auth.flavor, deserialized.flavor);
    assert_eq!(auth.body.as_ref(), deserialized.body.as_ref());
}

#[test]
fn auth_sys_constructor_limits() {
    let name = [b'm'; 256];
    let valid = auth_unix::new(1, &name[..255], 2, 3, vec![42; 16]).unwrap();
    let credential = opaque_auth::auth_unix(&valid).unwrap();
    assert_eq!(credential.body.len(), 340);
    assert_eq!(credential.body.as_ref(), encode(&valid).unwrap().as_ref());
    let decoded = auth_unix::unpack(&mut Decoder::copying(encode(&valid).unwrap())).unwrap();
    assert_eq!((decoded.stamp, decoded.uid, decoded.gid), (1, 2, 3));
    assert_eq!(decoded.machinename.as_ref(), &name[..255]);
    assert_eq!(decoded.gids.as_ref(), &[42; 16]);
    assert!(matches!(
        auth_unix::new(1, &name, 2, 3, vec![]),
        Err(Error::InvalidLength(256))
    ));
    assert!(matches!(
        auth_unix::new(1, b"host", 2, 3, vec![42; 17]),
        Err(Error::InvalidLength(17))
    ));
}

#[test]
fn auth_sys_decode_limits() {
    for (name_len, gids_len, expected_error) in [
        (255, 16, None),
        (256, 16, Some((Error::InvalidLength(256), 8))),
        (255, 17, Some((Error::InvalidLength(17), 276))),
    ] {
        let machinename = Opaque::owned(vec![b'm'; name_len]);
        let gids = BoundedVec::<u32, 17>::try_from(vec![42; gids_len]).unwrap();
        let mut out = Encoder::new(0);
        0u32.pack(&mut out).unwrap(); // stamp
        machinename.pack(&mut out).unwrap();
        0u32.pack(&mut out).unwrap(); // uid
        0u32.pack(&mut out).unwrap(); // gid
        gids.pack(&mut out).unwrap();
        let bytes = out.into_bytes();
        let mut input = Decoder::copying(bytes.clone());
        let result = auth_unix::unpack(&mut input);
        if let Some((expected_error, prefix_len)) = expected_error {
            assert_eq!(result.unwrap_err(), expected_error);
            assert_eq!(input.into_remaining(), bytes.slice(prefix_len..));
        } else {
            let decoded = result.unwrap();
            assert_eq!(decoded.machinename.as_ref(), machinename.as_ref());
            assert_eq!(decoded.gids.as_ref(), gids.as_ref());
            assert_eq!(encode(&decoded).unwrap(), bytes);
            assert!(input.is_empty());
        }
    }
}

#[test]
fn opaque_auth_body_limit() {
    let data = [42; 401];
    let body = BoundedOpaque::<400>::try_from(Opaque::borrowed(&data[..400])).unwrap();
    let auth = opaque_auth {
        flavor: auth_flavor::AUTH_UNIX,
        body,
    };
    let mut input = Decoder::copying(encode(&auth).unwrap());
    assert_eq!(
        opaque_auth::unpack(&mut input).unwrap().body.as_ref(),
        &data[..400]
    );
    assert!(input.is_empty());

    let body = Opaque::borrowed(&data);
    assert!(matches!(
        BoundedOpaque::<400>::try_from(body.clone()),
        Err(Error::InvalidLength(401))
    ));
    let mut out = Encoder::new(0);
    auth.flavor.pack(&mut out).unwrap();
    body.pack(&mut out).unwrap();
    let mut input = Decoder::copying(out.into_bytes());
    assert!(matches!(
        opaque_auth::unpack(&mut input),
        Err(Error::InvalidLength(401))
    ));
    assert_eq!(input.remaining(), 404);
}

#[test]
fn opaque_auth_with_data() {
    let auth = opaque_auth {
        flavor: auth_flavor::AUTH_UNIX,
        body: BoundedOpaque::from_slice(&[0x12, 0x34, 0x56, 0x78]).unwrap(),
    };
    let buffer = encode(&auth).unwrap().to_vec();
    let len = buffer.len();
    assert_eq!(len, 12); // 4 bytes flavor + 4 bytes length + 4 bytes data
    assert_eq!(buffer, [0, 0, 0, 1, 0, 0, 0, 4, 0x12, 0x34, 0x56, 0x78]);

    let mut cursor = Decoder::copying(buffer);
    let deserialized = opaque_auth::unpack(&mut cursor).unwrap();
    assert!(cursor.is_empty());
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
        let buffer = encode(&res).unwrap().to_vec();
        let len = buffer.len();
        assert_eq!(len, 4);

        let mut cursor = Decoder::copying(buffer);
        let deserialized = mountres3::unpack(&mut cursor).unwrap();
        assert!(cursor.is_empty());
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
            body: BoundedOpaque::empty(),
        };
        let buffer = encode(&auth).unwrap().to_vec();
        let len = buffer.len();
        assert_eq!(len, 8); // 4 bytes flavor + 4 bytes length

        let mut cursor = Decoder::copying(buffer);
        let deserialized = opaque_auth::unpack(&mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(auth.flavor, deserialized.flavor);
    }
}
