use crate::xdr_codec::{Payload, PayloadError, PayloadLimit, PayloadLimits, RecordCursor};

const fn limits() -> PayloadLimits {
    PayloadLimits {
        max_logical_bytes: 128,
        max_backing_capacity: 1024,
        max_segments: 128,
    }
}

fn cursor(chunks: Vec<Vec<u8>>) -> RecordCursor {
    RecordCursor::new(Payload::try_from_chunks(chunks, limits()).unwrap())
}

// Independently authored XDR: u32, u64, false, true, opaque "abc", opaque "".
const GOLDEN: &[u8] = &[
    0x12, 0x34, 0x56, 0x78, 1, 2, 3, 4, 5, 6, 7, 8, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 3, b'a', b'b',
    b'c', 0, 0, 0, 0, 0,
];

fn check_golden(mut cursor: RecordCursor) {
    assert_eq!(cursor.read_u32().unwrap(), 0x1234_5678);
    assert_eq!(cursor.read_u64().unwrap(), 0x0102_0304_0506_0708);
    assert!(!cursor.read_bool().unwrap());
    assert!(cursor.read_bool().unwrap());
    let opaque = cursor.read_opaque(limits()).unwrap();
    assert_eq!(opaque.try_to_vec().unwrap(), b"abc");
    let empty = cursor.read_opaque(limits()).unwrap();
    assert!(empty.is_empty());
    assert_eq!(empty.retained_capacity(), 0);
    assert_eq!(cursor.position(), GOLDEN.len());
    assert_eq!(cursor.remaining(), 0);
    cursor.read_exact(&mut []).unwrap();
    cursor.finish().unwrap();
    assert_eq!(opaque.try_to_vec().unwrap(), b"abc");
}

#[test]
fn independent_golden_at_every_byte_boundary() {
    for split in 0..=GOLDEN.len() {
        check_golden(cursor(vec![
            GOLDEN[..split].to_vec(),
            GOLDEN[split..].to_vec(),
        ]));
    }
    check_golden(cursor(GOLDEN.iter().map(|&byte| vec![byte]).collect()));
}

#[test]
fn opaque_after_multiple_reads_matches_independent_bytes_and_public_slice() {
    for split in 0..=GOLDEN.len() {
        let record = Payload::try_from_chunks(
            vec![GOLDEN[..split].to_vec(), GOLDEN[split..].to_vec()],
            limits(),
        )
        .unwrap();
        let expected = record.try_slice(24..27).unwrap();
        assert_eq!(expected.try_to_vec().unwrap(), b"abc");
        let mut c = RecordCursor::new(record);
        assert_eq!(c.read_u32().unwrap(), 0x1234_5678);
        assert_eq!(c.read_u64().unwrap(), 0x0102_0304_0506_0708);
        assert!(!c.read_bool().unwrap());
        assert!(c.read_bool().unwrap());
        let actual = c.read_opaque(limits()).unwrap();
        assert_eq!(actual.try_to_vec().unwrap(), b"abc");
        assert_eq!(
            actual.chunks().collect::<Vec<_>>(),
            expected.chunks().collect::<Vec<_>>()
        );
        assert_eq!(actual.retained_capacity(), expected.retained_capacity());
        assert!(c.read_opaque(limits()).unwrap().is_empty());
        c.finish().unwrap();
    }
}

#[test]
fn all_padding_lengths_and_explicit_copy_at_every_boundary() {
    // Opaque "a"/"ab"/"abc"/"abcd": three, two, one and zero pad bytes.
    for wire in [
        &[0, 0, 0, 1, b'a', 0, 0, 0][..],
        &[0, 0, 0, 2, b'a', b'b', 0, 0],
        &[0, 0, 0, 3, b'a', b'b', b'c', 0],
        &[0, 0, 0, 4, b'a', b'b', b'c', b'd'],
    ] {
        for split in 0..=wire.len() {
            let chunks = || vec![wire[..split].to_vec(), wire[split..].to_vec()];
            let mut shared = cursor(chunks());
            assert_eq!(
                shared.read_opaque(limits()).unwrap().try_to_vec().unwrap(),
                &wire[4..4 + usize::from(wire[3])]
            );
            shared.finish().unwrap();
            let mut copied = cursor(chunks());
            let mut out = [0xee; 6];
            let n = copied.read_opaque_into(&mut out, 4).unwrap();
            assert_eq!(&out[..n], &wire[4..4 + n]);
            assert!(out[n..].iter().all(|&b| b == 0xee));
            copied.finish().unwrap();
        }
    }
}

#[test]
fn bool_and_scalar_failures_preserve_cached_state() {
    for split in 0..=8 {
        let wire = [0, 0, 0, 2, 0, 0, 0, 1];
        let mut c = cursor(vec![wire[..split].to_vec(), wire[split..].to_vec()]);
        assert_eq!(
            c.read_bool(),
            Err(PayloadError::InvalidBool {
                offset: 0,
                value: 2
            })
        );
        assert_eq!(c.position(), 0);
        assert_eq!(c.read_u32().unwrap(), 2);
        c.read_u64().unwrap_err();
        assert_eq!(c.position(), 4);
        assert!(c.read_bool().unwrap());
        c.read_u32().unwrap_err();
        c.finish().unwrap();
    }
    for n in 0..8 {
        let mut c = cursor(vec![vec![1; n]]);
        c.read_u64().unwrap_err();
        assert_eq!(c.position(), 0);
        let mut out = [0; 8];
        c.read_exact(&mut out[..n]).unwrap();
        assert_eq!(&out[..n], &vec![1; n]);
        c.finish().unwrap();
    }
}

#[test]
fn opaque_truncation_and_padding_errors_are_atomic() {
    let valid = [0, 0, 0, 3, b'a', b'b', b'c', 0];
    for end in 0..valid.len() {
        for split in 0..=end {
            let chunks = || vec![valid[..split].to_vec(), valid[split..end].to_vec()];
            let mut shared = cursor(chunks());
            assert!(matches!(
                shared.read_opaque(limits()),
                Err(PayloadError::Truncated { .. })
            ));
            assert_eq!(shared.position(), 0);
            let mut copied = cursor(chunks());
            let mut output = [0xee; 8];
            copied.read_opaque_into(&mut output, 8).unwrap_err();
            assert_eq!(output, [0xee; 8]);
            assert_eq!(copied.position(), 0);
            let mut exact = [0xee; 8];
            assert!(copied.read_exact(&mut exact).is_err());
            assert_eq!(exact, [0xee; 8]);
            copied.read_exact(&mut exact[..end]).unwrap();
            assert_eq!(&exact[..end], &valid[..end]);
            copied.finish().unwrap();
        }
    }
    for pad_index in 5..8 {
        let mut wire = [0, 0, 0, 1, b'x', 0, 0, 0];
        wire[pad_index] = 9;
        for split in 0..=8 {
            let chunks = || vec![wire[..split].to_vec(), wire[split..].to_vec()];
            let expected = PayloadError::NonzeroPadding {
                offset: pad_index,
                value: 9,
            };
            let mut shared = cursor(chunks());
            assert_eq!(shared.read_opaque(limits()).unwrap_err(), expected);
            assert_eq!(shared.position(), 0);
            let mut copied = cursor(chunks());
            let mut output = [0xee; 8];
            assert_eq!(copied.read_opaque_into(&mut output, 8), Err(expected));
            assert_eq!(output, [0xee; 8]);
            assert_eq!(copied.position(), 0);
            assert_eq!(copied.read_u32().unwrap(), 1);
        }
    }
}

#[test]
fn extraction_limits_and_small_copy_are_explicit() {
    let wire = [0, 0, 0, 3, b'a', b'b', b'c', 0];
    let mut backing = Vec::with_capacity(256);
    backing.extend_from_slice(&wire);
    let ptr = backing.as_ptr();
    let mut c = cursor(vec![backing]);
    for bounds in [
        PayloadLimits {
            max_logical_bytes: 2,
            ..limits()
        },
        PayloadLimits {
            max_backing_capacity: 255,
            ..limits()
        },
        PayloadLimits {
            max_segments: 0,
            ..limits()
        },
    ] {
        assert!(matches!(
            c.read_opaque(bounds),
            Err(PayloadError::LimitExceeded { .. })
        ));
        assert_eq!(c.position(), 0);
    }
    let mut small = [0xee; 2];
    assert_eq!(
        c.read_opaque_into(&mut small, 3),
        Err(PayloadError::BufferTooSmall {
            needed: 3,
            available: 2
        })
    );
    assert_eq!(small, [0xee; 2]);
    let mut copied = [0xee; 3];
    c.read_opaque_into(&mut copied, 2).unwrap_err();
    assert_eq!(copied, [0xee; 3]);
    assert_eq!(c.position(), 0);
    let shared = c
        .read_opaque(PayloadLimits {
            max_logical_bytes: 3,
            max_backing_capacity: 256,
            max_segments: 1,
        })
        .unwrap();
    assert_eq!(
        shared.as_contiguous().unwrap().as_ptr(),
        ptr.wrapping_add(4)
    );
    c.finish().unwrap();
    let mut c = cursor(vec![wire.to_vec()]);
    assert_eq!(c.read_opaque_into(&mut copied, 3).unwrap(), 3);
    c.finish().unwrap();
    assert_eq!(copied, b"abc"[..]);

    let mut c = cursor(vec![
        vec![0, 0, 0, 3],
        vec![b'a'],
        vec![b'b'],
        vec![b'c'],
        vec![0],
    ]);
    assert!(matches!(
        c.read_opaque(PayloadLimits {
            max_segments: 2,
            ..limits()
        }),
        Err(PayloadError::LimitExceeded {
            kind: PayloadLimit::Segments,
            actual: 3,
            ..
        })
    ));
    assert_eq!(c.position(), 0);
    assert_eq!(
        c.read_opaque(PayloadLimits {
            max_segments: 3,
            ..limits()
        })
        .unwrap()
        .segment_count(),
        3
    );
    c.finish().unwrap();
}

#[test]
fn maximum_wire_lengths_are_checked_without_reserving() {
    // On 64-bit, padding u32::MAX is representable but the wire is truncated.
    // On 32-bit, the checked len+padding fails. This host test is not 32-bit execution.
    for value in [u32::MAX - 3, u32::MAX - 2, u32::MAX - 1, u32::MAX] {
        let mut c = cursor(vec![value.to_be_bytes().to_vec()]);
        let result = c.read_opaque(PayloadLimits {
            max_logical_bytes: usize::MAX,
            max_backing_capacity: usize::MAX,
            max_segments: usize::MAX,
        });
        if usize::BITS == 32 && value > u32::MAX - 3 {
            assert_eq!(result.unwrap_err(), PayloadError::Overflow);
        } else {
            assert!(matches!(result, Err(PayloadError::Truncated { .. })));
        }
        assert_eq!(c.position(), 0);
        let mut output = [0xee; 3];
        c.read_opaque_into(&mut output, usize::MAX).unwrap_err();
        assert_eq!(output, [0xee; 3]);
        assert_eq!(c.read_u32().unwrap(), value);
        c.finish().unwrap();
    }
}

#[test]
fn finish_and_summary_debug() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<RecordCursor>();
    let mut c = cursor(vec![b"secret".to_vec()]);
    assert!(!format!("{c:?}").contains("secret"));
    let mut out = [0; 2];
    c.read_exact(&mut out).unwrap();
    assert_eq!(
        c.finish(),
        Err(PayloadError::Trailing {
            offset: 2,
            remaining: 4
        })
    );
    cursor(vec![]).finish().unwrap();
    let mut c = cursor(vec![vec![0, 0, 0, 1], vec![b'x', 0, 0, 0]]);
    let payload = c.read_opaque(limits()).unwrap();
    std::thread::spawn(move || {
        c.finish().unwrap();
        assert_eq!(payload.try_to_vec().unwrap(), b"x");
    })
    .join()
    .unwrap();
}

#[test]
fn failed_opaque_after_partial_chunk_progress_preserves_cache_and_output() {
    let wire = [9, 8, 7, 6, 0, 0, 0, 1, b'x', 0, 0, 9, 0, 0, 0, 1];
    let mut c = cursor(vec![
        wire[..5].to_vec(),
        wire[5..10].to_vec(),
        wire[10..].to_vec(),
    ]);
    assert_eq!(c.read_u32().unwrap(), 0x0908_0706);
    let expected = PayloadError::NonzeroPadding {
        offset: 11,
        value: 9,
    };
    assert_eq!(c.read_opaque(limits()).unwrap_err(), expected);
    assert_eq!(c.position(), 4);
    let mut output = [0xee; 8];
    assert_eq!(c.read_opaque_into(&mut output, 8), Err(expected));
    assert_eq!(c.position(), 4);
    assert_eq!(output, [0xee; 8]);
    c.read_exact(&mut output).unwrap();
    assert_eq!(&output, &wire[4..12]);
    assert!(c.read_bool().unwrap());
    c.finish().unwrap();
}
