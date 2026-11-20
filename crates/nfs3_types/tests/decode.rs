#![allow(clippy::unwrap_used)]

use nfs3_types::xdr_codec::*;
use nfs3_types::{mount, nfs3, rpc};

const fn limits() -> DecodeLimits {
    DecodeLimits {
        max_metadata_bytes: 4096,
        max_collection_elements: 100,
        max_depth: 32,
        max_reservation_bytes: 16384,
        payload: PayloadLimits {
            max_logical_bytes: 4096,
            max_backing_capacity: 16384,
            max_segments: 1024,
        },
    }
}

fn record(bytes: &[u8]) -> Payload {
    Payload::try_from_vec(bytes.to_vec(), limits().payload).unwrap()
}

#[test]
fn descriptor_cost_matches_independent_selected_ranges() {
    assert_eq!(Payload::descriptor_reservation_cost(0), Some(0));
    assert_eq!(Payload::descriptor_reservation_cost(usize::MAX), None);
    // Four header-only origins and three padding-only origins must not charge.
    // The independently built body has exactly `count` one-byte origins.
    for count in [0_u32, 1, 2, 17, 144] {
        let mut chunks: Vec<Vec<u8>> = count.to_be_bytes().map(|b| vec![b]).into();
        chunks.extend((0..count).map(|_| vec![0x71]));
        chunks.extend((0..(4 - count % 4) % 4).map(|_| vec![0]));
        let cost = Payload::descriptor_reservation_cost(count as usize).unwrap();
        for budget in [Some(cost), cost.checked_sub(1)].into_iter().flatten() {
            let source = Payload::try_from_chunks(chunks.clone(), limits().payload).unwrap();
            let mut policy = limits();
            policy.max_reservation_bytes = budget;
            let mut decoder = Decoder::new(source, policy);
            if budget == cost {
                let value: Payload = decoder.decode().unwrap();
                assert_eq!(value.segment_count(), count as usize);
                assert_eq!(value.try_to_vec().unwrap(), vec![0x71; count as usize]);
                decoder.finish().unwrap();
            } else {
                assert!(matches!(
                    decoder.decode::<Payload>(),
                    Err(DecodeError::Limit {
                        kind: DecodeLimit::ReservationBytes, actual, ..
                    }) if actual == cost
                ));
            }
        }
    }
}

#[test]
fn metadata_independent_and_descriptor_failure_atomic() {
    let bytes = [0, 0, 0, 3, 10, 20, 30, 0];
    let source = record(&bytes);
    let pointer = source.as_contiguous().unwrap()[4..].as_ptr();
    let mut decoder = Decoder::new(source, limits());
    let value: Opaque<'_> = decoder.decode().unwrap();
    assert_ne!(value.as_ptr(), pointer);
    assert_eq!(value.as_ref(), &[10, 20, 30]);
    decoder.finish().unwrap();

    let mut policy = limits();
    policy.max_reservation_bytes = 0;
    let mut decoder = Decoder::new(record(&bytes), policy);
    let error = decoder.decode::<Payload>().unwrap_err();
    assert!(matches!(
        error,
        DecodeError::Limit {
            kind: DecodeLimit::ReservationBytes,
            ..
        }
    ));
    assert_eq!(decoder.decode::<u32>(), Err(error));
    assert_eq!(decoder.finish(), Err(error));
}

#[derive(Debug, XdrDecode)]
struct Generic<'a, T, const N: usize>
where
    T: std::fmt::Debug,
{
    value: T,
    name: Opaque<'a>,
    fixed: [u8; N],
}
#[derive(Debug, XdrDecode)]
struct Recursive(List<Self>);
#[derive(Debug, XdrDecode)]
struct Unit;
#[derive(Debug, XdrDecode)]
enum Tagged<T> {
    #[xdr(7)]
    Value(T),
    #[xdr(9)]
    Empty,
}

#[derive(Debug, PartialEq, Eq, XdrDecode)]
enum Simple {
    Zero,
    Five = 5,
    Six,
}

#[test]
fn derive_forms() {
    let mut d = Decoder::new(
        record(&[0, 0, 0, 7, 0, 0, 0, 3, 0, 0, 0, 0, 5, 0, 0, 0]),
        limits(),
    );
    let Tagged::Value(v): Tagged<Generic<'_, u32, 1>> = d.decode().unwrap() else {
        panic!()
    };
    assert_eq!(v.value, 3);
    assert!(v.name.is_empty());
    assert_eq!(v.fixed, [5]);
    d.decode::<Unit>().unwrap();
    d.finish().unwrap();
    let mut d = Decoder::new(record(&[0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0]), limits());
    let recursive: Recursive = d.decode().unwrap();
    assert_eq!(recursive.0.0.len(), 1);
    d.finish().unwrap();
    for (tag, expected) in [(0, Simple::Zero), (5, Simple::Five), (6, Simple::Six)] {
        assert_eq!(decoded::<Simple>(&words(&[tag]), 2), expected);
    }
}

// Independent wire fixtures: no encoder is used to construct these records.
fn words(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_be_bytes())
        .collect()
}

fn split_record(bytes: &[u8], split: usize) -> Payload {
    Payload::try_from_chunks(
        vec![bytes[..split].to_vec(), bytes[split..].to_vec()],
        limits().payload,
    )
    .unwrap()
}

fn decoded<T: Decode>(bytes: &[u8], split: usize) -> T {
    let mut decoder = Decoder::new(split_record(bytes, split), limits());
    let value = decoder.decode().unwrap();
    decoder.finish().unwrap();
    value
}

fn legacy_bytes<T: Pack + Unpack + Encode>(bytes: &[u8]) -> Vec<u8> {
    let (value, count) = T::unpack(&mut &*bytes).unwrap();
    assert_eq!(count, bytes.len());
    let mut result = Vec::new();
    value.pack(&mut result).unwrap();
    let mut e = Encoder::new(EncodeLimits {
        max_wire_bytes: 4096,
        max_metadata_bytes: 4096,
        max_chunks: 100,
        max_retained_capacity: 16384,
        max_depth: 64,
        max_reservation_bytes: 65536,
    });
    e.encode(&value).unwrap();
    let plan = e.finish().unwrap();
    assert_eq!(plan.len(), bytes.len());
    assert_eq!(plan.chunks().flatten().copied().collect::<Vec<_>>(), bytes);
    result
}

#[test]
fn rpc_and_manual_union_wire_fixtures_at_every_split() {
    let fixtures = [
        // CALL: xid, tag, rpcvers, program, version, procedure, two AUTH_NULLs.
        words(&[17, 0, 2, 100_003, 3, 0, 0, 0, 0, 0]),
        // Accepted PROG_MISMATCH reply including verifier and low/high.
        words(&[18, 1, 0, 0, 0, 2, 2, 3]),
        words(&[19, 1, 0, 0, 0, 0]), // accepted success
        words(&[20, 1, 1, 0, 2, 3]), // rejected RPC_MISMATCH
        words(&[21, 1, 1, 1, 5]),    // rejected AUTH_TOOWEAK
    ];
    for bytes in &fixtures {
        assert_eq!(legacy_bytes::<rpc::rpc_msg<'_, '_>>(bytes), *bytes);
        for split in 0..=bytes.len() {
            let value: rpc::rpc_msg<'_, '_> = decoded(bytes, split);
            let mut encoded = Vec::new();
            value.pack(&mut encoded).unwrap();
            assert_eq!(encoded, *bytes);
        }
    }
    for (bytes, ok) in [(words(&[0, 0, 1, 1]), true), (words(&[13]), false)] {
        for split in 0..=bytes.len() {
            let result: mount::mountres3<'_> = decoded(&bytes, split);
            assert_eq!(matches!(result, mount::mountres3::Ok(_)), ok);
        }
        assert_eq!(legacy_bytes::<mount::mountres3<'_>>(&bytes), bytes);
    }
    for tag in [1, 2, 5] {
        let value: nfs3::mknoddata3 = decoded(&words(&[tag]), 2);
        assert!(matches!(value, nfs3::mknoddata3::default));
    }
    // Valid MKNOD arms: six absent/default sattr words, and device major/minor
    // only for block/character devices. Exercise manual encoding, not tag invention.
    for tag in [3, 4, 6, 7] {
        let mut fixture = vec![0; if tag <= 4 { 9 } else { 7 }];
        fixture[0] = tag;
        let bytes = words(&fixture);
        assert_eq!(legacy_bytes::<nfs3::mknoddata3>(&bytes), bytes);
    }
    let failure = words(&[13, 0]);
    assert_eq!(legacy_bytes::<nfs3::READ3res<'_>>(&failure), failure);
    let result: nfs3::READ3resPayload = decoded(&failure, 3);
    assert!(matches!(
        result,
        nfs3::Nfs3Result::Err((nfs3::nfsstat3::NFS3ERR_ACCES, _))
    ));
}

fn assert_shared(source: &Payload, offset: usize, output: &Payload) {
    let expected: Vec<_> = source
        .chunks()
        .scan(0, |base, chunk| {
            let start = *base;
            *base += chunk.len();
            Some((start, chunk))
        })
        .filter_map(|(start, chunk)| {
            let end = start + chunk.len();
            if end <= offset || start >= offset + 3 {
                None
            } else {
                Some(chunk[offset.saturating_sub(start)..].as_ptr())
            }
        })
        .collect();
    let actual: Vec<_> = output.chunks().map(<[u8]>::as_ptr).collect();
    assert_eq!(actual, expected);
}

#[test]
fn read_write_bulk_fixture_identity_and_count_preservation() {
    let read = words(&[0, 0, 9, 1, 3, 0x6162_6300]);
    let write = words(&[3, 0x6668_2100, 0, 7, 9, 2, 3, 0x6162_6300]);
    assert_eq!(legacy_bytes::<nfs3::READ3res<'_>>(&read), read);
    assert_eq!(legacy_bytes::<nfs3::WRITE3args<'_>>(&write), write);
    for split in 0..=read.len() {
        let source = split_record(&read, split);
        let mut d = Decoder::new(source.try_clone().unwrap(), limits());
        let nfs3::Nfs3Result::Ok(value): nfs3::READ3resPayload = d.decode().unwrap() else {
            panic!()
        };
        assert_eq!(value.count, 9); // deliberately differs from opaque length
        assert!(value.eof);
        assert_eq!(value.data.try_to_vec().unwrap(), b"abc");
        assert_shared(&source, 20, &value.data);
        d.finish().unwrap();
        let legacy_form: nfs3::READ3res<'_> = decoded(&read, split);
        assert_eq!(legacy_form.unwrap().data.as_ref(), b"abc");
    }
    for split in 0..=write.len() {
        let source = split_record(&write, split);
        let mut d = Decoder::new(source.try_clone().unwrap(), limits());
        let value: nfs3::WRITE3argsPayload = d.decode().unwrap();
        assert_eq!(value.count, 9);
        assert_eq!(value.offset, 7);
        assert_eq!(value.file.data.as_ref(), b"fh!");
        assert_eq!(value.stable, nfs3::stable_how::FILE_SYNC);
        assert_shared(&source, 28, &value.data);
        d.finish().unwrap();
        drop(source);
        assert_eq!(value.data.try_to_vec().unwrap(), b"abc");
    }
}

#[test]
fn metadata_and_directory_fixtures() {
    let lookup = words(&[3, 0x6668_2100, 3, 0x6162_6300]);
    // READDIR success: absent attrs, verifier, one entry, eof.
    let directory = words(&[0, 0, 0, 4, 1, 0, 7, 3, 0x6162_6300, 0, 8, 0, 1]);
    assert_eq!(legacy_bytes::<nfs3::LOOKUP3args<'_>>(&lookup), lookup);
    assert_eq!(legacy_bytes::<nfs3::READDIR3res<'_>>(&directory), directory);
    for split in 0..=lookup.len() {
        let value: nfs3::LOOKUP3args<'_> = decoded(&lookup, split);
        assert_eq!(value.what.dir.data.as_ref(), b"fh!");
        assert_eq!(value.what.name.0.as_ref(), b"abc");
    }
    for split in 0..=directory.len() {
        let value: nfs3::READDIR3res<'_> = decoded(&directory, split);
        let value = value.unwrap();
        assert!(value.reply.eof);
        assert_eq!(value.reply.entries.0.len(), 1);
        assert_eq!(value.reply.entries.0[0].fileid, 7);
        assert_eq!(value.reply.entries.0[0].name.0.as_ref(), b"abc");
        assert_eq!(value.reply.entries.0[0].cookie, 8);
    }
}

#[derive(Debug)]
struct Swallows;
impl Decode for Swallows {
    fn decode(d: &mut Decoder) -> DecodeResult<Self> {
        let _ = d.decode::<bool>();
        Ok(Self)
    }
}

#[test]
fn strict_errors_sticky_even_when_nested_error_swallowed() {
    let invalid = words(&[2, 5]);
    let mut d = Decoder::new(record(&invalid), limits());
    let error = d.decode::<Swallows>().unwrap_err();
    assert!(matches!(
        error,
        DecodeError::Wire(PayloadError::InvalidBool { value: 2, .. })
    ));
    assert_eq!(d.position(), Err(error));
    assert_eq!(d.remaining(), Err(error));
    assert_eq!(d.opaque(0), Err(error));
    assert_eq!(d.collection::<u32>(0), Err(error));
    assert_eq!(d.fail::<()>(DecodeError::InvalidTag(99)), Err(error));
    assert_eq!(d.finish(), Err(error));
    for bytes in [words(&[1, 0x6101_0000]), words(&[u32::MAX])] {
        let mut d = Decoder::new(record(&bytes), limits());
        assert!(d.decode::<Opaque<'_>>().is_err());
    }
    let mut d = Decoder::new(record(&[1, 0, 2, 0]), limits());
    assert!(matches!(
        d.decode::<[u8; 1]>(),
        Err(DecodeError::Wire(PayloadError::NonzeroPadding { .. }))
    ));
    for tag in [0, 8, u32::MAX] {
        let mut d = Decoder::new(record(&words(&[tag])), limits());
        assert_eq!(
            d.decode::<nfs3::mknoddata3>().unwrap_err(),
            DecodeError::InvalidTag(tag)
        );
    }
    let mut d = Decoder::new(record(&words(&[2])), limits());
    assert_eq!(
        d.decode::<Tagged<u32>>().unwrap_err(),
        DecodeError::InvalidTag(2)
    );
    let mut d = Decoder::new(record(&words(&[u32::MAX])), limits());
    assert!(matches!(
        d.decode::<Vec<u32>>(),
        Err(DecodeError::Wire(PayloadError::Truncated { .. }))
    ));
}

#[test]
fn aggregate_depth_host_expansion_and_geometric_work() {
    let bytes = words(&[3, 1, 2, 3]);
    let mut policy = limits();
    // Growth targets 1,2,4 cost (1+2+4)*4 = 28, not the final 16.
    policy.max_reservation_bytes = 28;
    let mut d = Decoder::new(record(&bytes), policy);
    assert_eq!(d.decode::<Vec<u32>>().unwrap(), [1, 2, 3]);
    d.finish().unwrap();
    policy.max_reservation_bytes = 27;
    let mut d = Decoder::new(record(&bytes), policy);
    assert!(matches!(
        d.decode::<Vec<u32>>(),
        Err(DecodeError::Limit {
            kind: DecodeLimit::ReservationBytes,
            actual: 28,
            ..
        })
    ));

    // Nested Vec host structures expand beyond their four-byte wire prefixes.
    policy = limits();
    policy.max_reservation_bytes = size_of::<Vec<u32>>() - 1;
    let mut d = Decoder::new(record(&words(&[1, 0])), policy);
    assert!(matches!(
        d.decode::<Vec<Vec<u32>>>(),
        Err(DecodeError::Limit {
            kind: DecodeLimit::ReservationBytes,
            ..
        })
    ));
    policy = limits();
    policy.max_collection_elements = 2;
    let mut d = Decoder::new(record(&words(&[2, 1, 7, 0])), policy);
    assert!(matches!(
        d.decode::<Vec<Vec<u32>>>(),
        Err(DecodeError::Limit {
            kind: DecodeLimit::CollectionElements,
            actual: 3,
            ..
        })
    ));
    policy = limits();
    policy.max_depth = 3;
    let mut d = Decoder::new(record(&words(&[1, 0, 0])), policy);
    assert!(matches!(
        d.decode::<Recursive>(),
        Err(DecodeError::Limit {
            kind: DecodeLimit::Depth,
            ..
        })
    ));

    policy = limits();
    policy.max_metadata_bytes = 5;
    let mut d = Decoder::new(record(&words(&[3, 0x6162_6300, 3, 0x6465_6600])), policy);
    d.decode::<Opaque<'_>>().unwrap();
    assert!(matches!(
        d.decode::<Opaque<'_>>(),
        Err(DecodeError::Limit {
            kind: DecodeLimit::MetadataBytes,
            actual: 6,
            ..
        })
    ));
}

fn rejects_bound<T: Decode>(bytes: &[u8]) {
    let mut policy = limits();
    policy.max_reservation_bytes = 0;
    let mut d = Decoder::new(record(bytes), policy);
    // Field validation must precede any reservation.
    assert!(matches!(
        d.decode::<T>(),
        Err(DecodeError::Wire(PayloadError::LimitExceeded { .. }))
    ));
}

#[test]
fn protocol_bounds_precede_allocation() {
    rejects_bound::<rpc::opaque_auth<'_>>(&words(&[0, 401]));
    rejects_bound::<rpc::auth_unix>(&words(&[0, 256]));
    rejects_bound::<nfs3::nfs_fh3>(&words(&[65]));
    rejects_bound::<mount::fhandle3<'_>>(&words(&[65]));
    rejects_bound::<mount::dirpath<'_>>(&words(&[1025]));
    rejects_bound::<mount::name<'_>>(&words(&[256]));
    let mut d = Decoder::new(record(&words(&[0, 0, 0, 0, 17])), limits());
    assert!(matches!(
        d.decode::<rpc::auth_unix>(),
        Err(DecodeError::Limit {
            kind: DecodeLimit::FieldElements,
            actual: 17,
            limit: 16,
        })
    ));
}
