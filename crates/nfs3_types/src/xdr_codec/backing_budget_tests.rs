use crate::xdr_codec::{
    BackingBudget, EncodeLimits, Encoder, PayloadLimits, ReceiveBuffer, RecordBuilder, RecordCursor,
};

#[test]
fn send_plan_and_cross_thread_final_release_outlive_domain_handle() {
    let domain = BackingBudget::new(16);
    let observer = domain.clone();
    let mut reservation = domain.try_reserve(16).unwrap();
    let buffer = ReceiveBuffer::try_new_charged(8, &mut reservation).unwrap();
    let limits = PayloadLimits {
        max_logical_bytes: 8,
        max_backing_capacity: 8,
        max_segments: 1,
    };
    let payload = buffer.freeze(8, limits).unwrap();
    drop(reservation);
    let mut encoder = Encoder::new(EncodeLimits {
        max_wire_bytes: 16,
        max_metadata_bytes: 8,
        max_chunks: 4,
        max_retained_capacity: 8,
        max_depth: 4,
        max_reservation_bytes: 4096,
    });
    encoder.encode(&payload).unwrap();
    let plan = encoder.finish().unwrap();
    drop(payload);
    drop(domain);
    assert_eq!(observer.used(), 8);
    std::thread::spawn(move || {
        assert_eq!(plan.chunks().map(<[u8]>::len).sum::<usize>(), 12);
        drop(plan);
    })
    .join()
    .unwrap();
    assert_eq!(observer.used(), 0);
}

#[test]
fn builder_work_and_descriptor_failure_drop_charges() {
    for (segments, work) in [(0, 4096), (4, 0)] {
        let domain = BackingBudget::new(16);
        let mut reservation = domain.try_reserve(16).unwrap();
        let limits = PayloadLimits {
            max_logical_bytes: 16,
            max_backing_capacity: 16,
            max_segments: segments,
        };
        let mut builder = RecordBuilder::new(limits, work);
        let buffer = ReceiveBuffer::try_new_charged(8, &mut reservation).unwrap();
        assert!(builder.push(buffer, 8).is_err());
        drop(builder);
        drop(reservation);
        assert_eq!(domain.used(), 0);
    }
    assert!(RecordBuilder::reservation_bound(usize::MAX).is_none());
}

#[test]
fn admission_and_selected_final_drop() {
    let domain = BackingBudget::new(16);
    let mut reservation = domain.try_reserve(16).unwrap();
    domain.try_reserve(1).unwrap_err();
    let limits = PayloadLimits {
        max_logical_bytes: 16,
        max_backing_capacity: 16,
        max_segments: 4,
    };
    let mut builder = RecordBuilder::new(limits, 4096);
    let mut pointer = std::ptr::null();
    for bytes in [&[0, 0, 0, 3][..], b"abc", &[0][..]] {
        let mut buffer = ReceiveBuffer::try_new_charged(bytes.len(), &mut reservation).unwrap();
        buffer.as_mut_slice().copy_from_slice(bytes);
        if bytes == b"abc" {
            pointer = buffer.as_mut_slice().as_ptr();
        }
        builder.push(buffer, bytes.len()).unwrap();
    }
    assert_eq!(domain.used(), 16);
    drop(reservation);
    assert_eq!(domain.used(), 8);
    let mut cursor = RecordCursor::new(builder.finish());
    let data = cursor.read_opaque(limits).unwrap();
    cursor.finish().unwrap();
    assert_eq!(domain.used(), 3);
    assert_eq!(data.as_contiguous().unwrap().as_ptr(), pointer);
    let clone = data.try_clone().unwrap();
    drop(data);
    assert_eq!(domain.used(), 3);
    drop(clone);
    assert_eq!(domain.used(), 0);
    domain.try_reserve(16).unwrap();
}

#[test]
fn empty_error_and_mutable_drop_release() {
    let domain = BackingBudget::new(16);
    let limits = PayloadLimits {
        max_logical_bytes: 16,
        max_backing_capacity: 16,
        max_segments: 4,
    };
    for prefix in [0, 17] {
        let mut reservation = domain.try_reserve(16).unwrap();
        let buffer = ReceiveBuffer::try_new_charged(16, &mut reservation).unwrap();
        drop(reservation);
        drop(buffer.freeze(prefix, limits));
        assert_eq!(domain.used(), 0);
    }
    let mut reservation = domain.try_reserve(16).unwrap();
    let buffer = ReceiveBuffer::try_new_charged(8, &mut reservation).unwrap();
    drop(buffer);
    assert_eq!(domain.used(), 8);
    drop(reservation);
    assert_eq!(domain.used(), 0);
}
