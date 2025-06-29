// Tests for XDR utility functions and edge cases
#![allow(clippy::unwrap_used)]

use std::borrow::Cow;
use std::io::Cursor;

use nfs3_types::xdr_codec::{Opaque, Pack, Unpack, Void};

#[test]
fn void_serialization() {
    let void_val = Void;

    let mut buffer = Vec::new();
    let len = void_val.pack(&mut buffer).unwrap();
    assert_eq!(len, 0);
    assert_eq!(void_val.packed_size(), 0);
    assert_eq!(buffer, []);

    let mut cursor = Cursor::new(buffer);
    let (deserialized, unpack_len) = Void::unpack(&mut cursor).unwrap();
    assert_eq!(unpack_len, 0);
    assert_eq!(void_val, deserialized);
}

#[test]
fn boolean_serialization() {
    // Test true
    let true_val = true;
    let mut buffer = Vec::new();
    let len = true_val.pack(&mut buffer).unwrap();
    assert_eq!(len, 4);
    assert_eq!(true_val.packed_size(), 4);
    assert_eq!(buffer, [0, 0, 0, 1]);

    let mut cursor = Cursor::new(buffer);
    let (deserialized, unpack_len) = bool::unpack(&mut cursor).unwrap();
    assert_eq!(unpack_len, 4);
    assert_eq!(true_val, deserialized);

    // Test false
    let false_val = false;
    let mut buffer = Vec::new();
    let len = false_val.pack(&mut buffer).unwrap();
    assert_eq!(len, 4);
    assert_eq!(false_val.packed_size(), 4);
    assert_eq!(buffer, [0, 0, 0, 0]);

    let mut cursor = Cursor::new(buffer);
    let (deserialized, unpack_len) = bool::unpack(&mut cursor).unwrap();
    assert_eq!(unpack_len, 4);
    assert_eq!(false_val, deserialized);
}

#[test]
fn integer_serialization() {
    // Test various u32 values
    let test_cases_u32 = [0u32, 1, 255, 256, 65535, 65536, u32::MAX];

    for original in test_cases_u32 {
        let mut buffer = Vec::new();
        let len = original.pack(&mut buffer).unwrap();
        assert_eq!(len, 4);
        assert_eq!(original.packed_size(), 4);

        let mut cursor = Cursor::new(buffer);
        let (deserialized, unpack_len) = u32::unpack(&mut cursor).unwrap();
        assert_eq!(unpack_len, 4);
        assert_eq!(original, deserialized);
    }
}

#[test]
fn u64_serialization() {
    let test_cases = [
        0u64,
        1,
        u64::from(u32::MAX),
        u64::from(u32::MAX) + 1,
        u64::MAX,
    ];

    for original in test_cases {
        let mut buffer = Vec::new();
        let len = original.pack(&mut buffer).unwrap();
        assert_eq!(len, 8);
        assert_eq!(original.packed_size(), 8);

        let mut cursor = Cursor::new(buffer);
        let (deserialized, unpack_len) = u64::unpack(&mut cursor).unwrap();
        assert_eq!(unpack_len, 8);
        assert_eq!(original, deserialized);
    }
}

#[test]
fn opaque_padding_verification() {
    // Test various sizes to verify XDR padding is correctly applied
    let test_cases = [
        (0, 4),  // 0 bytes data -> 4 bytes total (length only)
        (1, 8),  // 1 byte data -> 8 bytes total (length + data + 3 pad)
        (2, 8),  // 2 bytes data -> 8 bytes total (length + data + 2 pad)
        (3, 8),  // 3 bytes data -> 8 bytes total (length + data + 1 pad)
        (4, 8),  // 4 bytes data -> 8 bytes total (length + data + 0 pad)
        (5, 12), // 5 bytes data -> 12 bytes total (length + data + 3 pad)
        (8, 12), // 8 bytes data -> 12 bytes total (length + data + 0 pad)
    ];

    for (data_size, expected_total) in test_cases {
        let data = vec![0x42; data_size];
        let opaque = Opaque(Cow::Borrowed(&data));

        assert_eq!(opaque.packed_size(), expected_total);

        let mut buffer = Vec::new();
        let len = opaque.pack(&mut buffer).unwrap();
        assert_eq!(len, expected_total);

        // Verify the length field
        assert_eq!(
            &buffer[0..4],
            &u32::try_from(data_size).unwrap().to_be_bytes()
        );

        // Verify the data
        if data_size > 0 {
            assert_eq!(&buffer[4..4 + data_size], &data);
        }

        // Verify padding bytes are zero
        let padding_start = 4 + data_size;
        let padding_bytes = expected_total - padding_start;
        if padding_bytes > 0 {
            for &byte in &buffer[padding_start..] {
                assert_eq!(byte, 0, "Padding byte should be zero");
            }
        }
    }
}

#[test]
fn large_data_serialization() {
    // Test with larger amounts of data to ensure no overflow issues
    let large_data = vec![0x55; 1024]; // 1KB of data
    let opaque = Opaque(Cow::Borrowed(&large_data));

    let expected_size = 4 + 1024; // No padding needed since 1024 is multiple of 4
    assert_eq!(opaque.packed_size(), expected_size);

    let mut buffer = Vec::new();
    let len = opaque.pack(&mut buffer).unwrap();
    assert_eq!(len, expected_size);

    let mut cursor = Cursor::new(buffer);
    let (deserialized, unpack_len) = Opaque::unpack(&mut cursor).unwrap();
    assert_eq!(unpack_len, expected_size);
    assert_eq!(opaque.as_ref(), deserialized.as_ref());
}

#[test]
fn endianness_verification() {
    // Verify that multi-byte values are serialized in big-endian (network) byte order
    let test_u32 = 0x1234_5678_u32;
    let mut buffer = Vec::new();
    test_u32.pack(&mut buffer).unwrap();
    assert_eq!(buffer, [0x12, 0x34, 0x56, 0x78]);

    let test_u64 = 0x1234_5678_9abc_def0_u64;
    let mut buffer = Vec::new();
    test_u64.pack(&mut buffer).unwrap();
    assert_eq!(buffer, [0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0]);
}

#[test]
fn truncated_data_handling() {
    // Test behavior with truncated data (should fail gracefully)
    let mut cursor = Cursor::new([0x00, 0x00]); // Only 2 bytes, need 4 for u32
    let result = u32::unpack(&mut cursor);
    assert!(result.is_err(), "Should fail when not enough data");

    // Test with opaque data that claims to be longer than available data
    let mut cursor = Cursor::new([0x00, 0x00, 0x00, 0x05, 0x41, 0x42]); // Claims 5 bytes, only has 2
    let result = Opaque::unpack(&mut cursor);
    assert!(result.is_err(), "Should fail when opaque data is truncated");
}

#[test]
fn cow_borrowed_vs_owned() {
    // Test that both borrowed and owned Cow variants work correctly
    let borrowed_data = b"Hello, World!";
    let borrowed_opaque = Opaque(Cow::Borrowed(borrowed_data));

    let owned_data = borrowed_data.to_vec();
    let owned_opaque = Opaque(Cow::Owned(owned_data));

    // Both should serialize identically
    let mut borrowed_buffer = Vec::new();
    let borrowed_len = borrowed_opaque.pack(&mut borrowed_buffer).unwrap();

    let mut owned_buffer = Vec::new();
    let owned_len = owned_opaque.pack(&mut owned_buffer).unwrap();

    assert_eq!(borrowed_len, owned_len);
    assert_eq!(borrowed_buffer, owned_buffer);

    // Both should have the same packed size
    assert_eq!(borrowed_opaque.packed_size(), owned_opaque.packed_size());
}

#[test]
fn zero_vs_nonzero_data() {
    // Test serialization of zero vs non-zero data to ensure no special handling
    let zero_data = vec![0u8; 16];
    let zero_opaque = Opaque(Cow::Borrowed(&zero_data));

    let nonzero_data = vec![0x55u8; 16];
    let nonzero_opaque = Opaque(Cow::Borrowed(&nonzero_data));

    // Both should have the same size
    assert_eq!(zero_opaque.packed_size(), nonzero_opaque.packed_size());

    // Both should serialize to different contents but same structure
    let mut zero_buffer = Vec::new();
    let zero_len = zero_opaque.pack(&mut zero_buffer).unwrap();

    let mut nonzero_buffer = Vec::new();
    let nonzero_len = nonzero_opaque.pack(&mut nonzero_buffer).unwrap();

    assert_eq!(zero_len, nonzero_len);
    assert_ne!(zero_buffer, nonzero_buffer); // Different data contents
    assert_eq!(&zero_buffer[..4], &nonzero_buffer[..4]); // Same length field
}

#[test]
fn empty_vs_nonempty_opaque() {
    // Test empty opaque vs non-empty to verify different behavior
    let empty_opaque = Opaque(Cow::Borrowed(&[]));
    let nonempty_opaque = Opaque(Cow::Borrowed(&[0x42]));

    // Different sizes
    assert_eq!(empty_opaque.packed_size(), 4);
    assert_eq!(nonempty_opaque.packed_size(), 8);

    // Test serialization
    let mut empty_buffer = Vec::new();
    let empty_len = empty_opaque.pack(&mut empty_buffer).unwrap();
    assert_eq!(empty_len, 4);
    assert_eq!(empty_buffer, [0, 0, 0, 0]);

    let mut nonempty_buffer = Vec::new();
    let nonempty_len = nonempty_opaque.pack(&mut nonempty_buffer).unwrap();
    assert_eq!(nonempty_len, 8);
    assert_eq!(nonempty_buffer, [0, 0, 0, 1, 0x42, 0, 0, 0]);
}
