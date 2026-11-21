use nfs3_types::xdr_codec::{Payload, PayloadError, PayloadLimit, PayloadLimits, ReceiveBuffer};

const fn limits() -> PayloadLimits {
    PayloadLimits {
        max_logical_bytes: 64,
        max_backing_capacity: 128,
        max_segments: 8,
    }
}

#[test]
fn adoption_ranges_and_explicit_materialization() {
    let mut first = Vec::with_capacity(16);
    first.extend_from_slice(b"abcd");
    let ptr = first.as_ptr();
    let capacity = first.capacity();
    let payload =
        Payload::try_from_chunks(vec![first, vec![], b"efgh".to_vec()], limits()).unwrap();
    assert_eq!(payload.chunks().next().unwrap().as_ptr(), ptr);
    assert_eq!(payload.len(), 8);
    assert_eq!(payload.segment_count(), 2);
    assert_eq!(payload.retained_capacity(), capacity + 4);
    assert!(payload.as_contiguous().is_none());
    let selected = payload.try_slice(1..4).unwrap();
    assert_eq!(selected.as_contiguous().unwrap(), b"bcd");
    assert_eq!(
        selected.as_contiguous().unwrap().as_ptr(),
        ptr.wrapping_add(1)
    );
    assert_eq!(selected.retained_capacity(), capacity);
    let nested = selected.try_slice(1..2).unwrap();
    assert_eq!(
        nested.as_contiguous().unwrap().as_ptr(),
        ptr.wrapping_add(2)
    );
    let copy = nested.try_to_vec().unwrap();
    assert_eq!(copy, b"c");
    assert_ne!(copy.as_ptr(), nested.as_contiguous().unwrap().as_ptr());
    assert_eq!(
        payload.try_slice(4..8).unwrap().as_contiguous().unwrap(),
        b"efgh"
    );
    assert_eq!(
        payload.try_slice(3..5).unwrap().try_to_vec().unwrap(),
        b"de"
    );
    assert_eq!(
        payload.try_clone().unwrap().try_to_vec().unwrap(),
        b"abcdefgh"
    );
    for boundary in 0..=8 {
        let empty = payload.try_slice(boundary..boundary).unwrap();
        assert!(empty.is_empty());
        assert_eq!(empty.retained_capacity(), 0);
        assert_eq!(empty.segment_count(), 0);
        assert_eq!(empty.as_contiguous(), Some(&[][..]));
    }
    let reversed_start = 2;
    for range in [9..9, 0..usize::MAX, reversed_start..1] {
        assert!(matches!(
            payload.try_slice(range),
            Err(PayloadError::InvalidRange { .. })
        ));
    }
}

#[test]
fn explicit_limits_and_empty_inputs() {
    let bytes = || {
        let mut bytes = Vec::with_capacity(32);
        bytes.extend_from_slice(b"abc");
        bytes
    };
    for (limits, kind) in [
        (
            PayloadLimits {
                max_logical_bytes: 2,
                ..limits()
            },
            PayloadLimit::LogicalBytes,
        ),
        (
            PayloadLimits {
                max_backing_capacity: 31,
                ..limits()
            },
            PayloadLimit::BackingCapacity,
        ),
        (
            PayloadLimits {
                max_segments: 0,
                ..limits()
            },
            PayloadLimit::Segments,
        ),
    ] {
        assert!(
            matches!(Payload::try_from_vec(bytes(), limits), Err(PayloadError::LimitExceeded { kind: k, .. }) if k == kind)
        );
        assert!(
            matches!(Payload::try_from_chunks(vec![bytes()], limits), Err(PayloadError::LimitExceeded { kind: k, .. }) if k == kind)
        );
    }
    let zero = PayloadLimits {
        max_logical_bytes: 0,
        max_backing_capacity: 0,
        max_segments: 0,
    };
    let empty = Payload::try_from_chunks(vec![Vec::with_capacity(32)], zero).unwrap();
    assert_eq!(empty.retained_capacity(), 0);
    assert!(empty.try_to_vec().unwrap().is_empty());
    let exact = PayloadLimits {
        max_logical_bytes: 3,
        max_backing_capacity: 32,
        max_segments: 1,
    };
    assert_eq!(
        Payload::try_from_vec(bytes(), exact)
            .unwrap()
            .retained_capacity(),
        32
    );
}

#[test]
fn initialized_prefix_freeze_preserves_backing() {
    let mut buffer = ReceiveBuffer::try_new(16, 16).unwrap();
    assert_eq!(buffer.as_mut_slice(), &[0; 16]);
    buffer.as_mut_slice()[..3].copy_from_slice(b"abc");
    let ptr = buffer.as_mut_slice().as_ptr();
    let capacity = buffer.capacity();
    let payload = buffer.freeze(3, limits()).unwrap();
    assert_eq!(payload.as_contiguous().unwrap(), b"abc");
    assert_eq!(payload.as_contiguous().unwrap().as_ptr(), ptr);
    assert_eq!(payload.retained_capacity(), capacity);
    ReceiveBuffer::try_new(2, 1).unwrap_err();
    ReceiveBuffer::try_new(usize::MAX, usize::MAX).unwrap_err();
    ReceiveBuffer::try_new(2, 2)
        .unwrap()
        .freeze(3, limits())
        .unwrap_err();
    ReceiveBuffer::try_new(16, 16)
        .unwrap()
        .freeze(
            1,
            PayloadLimits {
                max_backing_capacity: 15,
                ..limits()
            },
        )
        .unwrap_err();
    let empty = ReceiveBuffer::try_new(16, 16)
        .unwrap()
        .freeze(
            0,
            PayloadLimits {
                max_logical_bytes: 0,
                max_backing_capacity: 0,
                max_segments: 0,
            },
        )
        .unwrap();
    assert_eq!(empty.retained_capacity(), 0);
    assert_eq!(ReceiveBuffer::try_new(0, 0).unwrap().capacity(), 0);
}

#[test]
fn shared_owners_cross_thread_and_debug_is_summary_only() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<Payload>();
    send_sync::<ReceiveBuffer>();
    send_sync::<PayloadError>();
    let payload = Payload::try_from_vec(b"secret".to_vec(), limits()).unwrap();
    assert!(!format!("{payload:?}").contains("secret"));
    let shared = payload.try_clone().unwrap();
    let ptr = payload.as_contiguous().unwrap().as_ptr() as usize;
    drop(payload);
    std::thread::spawn(move || {
        assert_eq!(shared.as_contiguous().unwrap(), b"secret");
        assert_eq!(shared.as_contiguous().unwrap().as_ptr() as usize, ptr);
    })
    .join()
    .unwrap();
}
