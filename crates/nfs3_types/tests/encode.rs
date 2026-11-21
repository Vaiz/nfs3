#![expect(
    clippy::unwrap_used,
    reason = "Test fixture helpers abort on codec failure"
)]

use nfs3_types::xdr_codec::*;
use nfs3_types::{mount, nfs3, portmap, rpc};

const fn limits() -> EncodeLimits {
    EncodeLimits {
        max_wire_bytes: 4096,
        max_metadata_bytes: 4096,
        max_chunks: 100,
        max_retained_capacity: 16384,
        max_depth: 64,
        max_reservation_bytes: 65536,
    }
}

fn payload(chunks: Vec<Vec<u8>>) -> Payload {
    Payload::try_from_chunks(
        chunks,
        PayloadLimits {
            max_logical_bytes: 4096,
            max_backing_capacity: 16384,
            max_segments: 100,
        },
    )
    .unwrap()
}

fn plan(value: &(impl Encode + ?Sized)) -> SendPlan {
    let mut e = Encoder::new(limits());
    e.encode(value).unwrap();
    e.finish().unwrap()
}

// Materialization is deliberately test-only.
fn flat(plan: &SendPlan) -> Vec<u8> {
    let bytes: Vec<_> = plan.chunks().flatten().copied().collect();
    assert_eq!(bytes.len(), plan.len());
    assert!(plan.chunks().all(|chunk| !chunk.is_empty()));
    bytes
}

fn words(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_be_bytes())
        .collect()
}

#[test]
fn metadata_coalesces_and_borrowed_opaque_is_copied() {
    let input = b"abc".to_vec();
    let source = input.as_ptr();
    let mut e = Encoder::new(limits());
    e.encode(&17_u32).unwrap();
    e.encode(&Opaque::borrowed(&input)).unwrap();
    e.encode(&[9_u8]).unwrap();
    e.encode(&3_u64).unwrap();
    let p = e.finish().unwrap();
    assert_eq!(p.chunks().len(), 1);
    assert_ne!(p.chunks().next().unwrap()[8..].as_ptr(), source);
    drop(input);
    assert_eq!(
        flat(&p),
        [
            0, 0, 0, 17, 0, 0, 0, 3, b'a', b'b', b'c', 0, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3,
        ]
    );
}

#[test]
fn nfs_write_read_bulk_golden_pointer_identity_and_structural_counts() {
    fn send_sync_static<T: Send + Sync + 'static>() {}

    let data = payload(vec![b"a".to_vec(), b"bc".to_vec()]);
    let pointers: Vec<_> = data.chunks().map(<[u8]>::as_ptr).collect();
    let write = nfs3::WRITE3args {
        file: nfs3::nfs_fh3 {
            data: Opaque::borrowed(b"fh!"),
        },
        offset: 0x0000_0001_0000_0002,
        count: 99, // Structural, intentionally not data.len().
        stable: nfs3::stable_how::FILE_SYNC,
        data: &data,
    };
    let old = nfs3::WRITE3args {
        file: write.file.clone(),
        offset: write.offset,
        count: write.count,
        stable: write.stable,
        data: Opaque::borrowed(b"abc"),
    };
    let mut golden = words(&[3]);
    golden.extend_from_slice(b"fh!\0");
    golden.extend(words(&[1, 2, 99, 2, 3]));
    golden.extend_from_slice(b"abc\0");
    let p = plan(&write);
    assert_eq!(flat(&p), golden);
    assert_eq!(flat(&plan(&old)), golden);
    drop(write);
    drop(data); // the borrowed input's backing is now owned by the send plan
    assert_eq!(
        p.chunks().map(<[u8]>::len).collect::<Vec<_>>(),
        [28, 1, 2, 1]
    );
    for (chunk, pointer) in p.chunks().skip(1).take(2).zip(pointers) {
        assert_eq!(chunk.as_ptr(), pointer);
    }

    let read = nfs3::READ3res::<Payload>::Ok(nfs3::READ3resok {
        file_attributes: nfs3::Nfs3Option::None,
        count: 7, // Preserve the supplied count, not the opaque length.
        eof: true,
        data: payload(vec![b"ab".to_vec(), b"c".to_vec()]),
    });
    let old: nfs3::READ3res<Opaque<'_>> = nfs3::Nfs3Result::Ok(nfs3::READ3resok {
        file_attributes: nfs3::Nfs3Option::None,
        count: 7,
        eof: true,
        data: Opaque::borrowed(b"abc"),
    });
    let mut golden = words(&[0, 0, 7, 1, 3]);
    golden.extend_from_slice(b"abc\0");
    let p = plan(&read);
    drop(read);
    assert_eq!(flat(&p), golden);
    assert_eq!(flat(&plan(&old)), golden);
    send_sync_static::<SendPlan>();
    let p = std::thread::spawn(move || {
        assert_eq!(flat(&p), golden);
        p
    })
    .join()
    .unwrap();
    assert_eq!(p.len(), 24);
}

#[test]
fn empty_bulk_and_padding_for_all_residues() {
    let mut zero = limits();
    zero.max_wire_bytes = 0;
    zero.max_metadata_bytes = 0;
    zero.max_chunks = 0;
    zero.max_retained_capacity = 0;
    zero.max_reservation_bytes = 0;
    let mut e = Encoder::new(zero);
    e.encode(&Void).unwrap();
    e.encode(&[0_u8; 0]).unwrap();
    let p = e.finish().unwrap();
    assert!(p.is_empty());
    assert_eq!(p.chunks().len(), 0);

    for len in 0..=8 {
        let bulk = payload(vec![vec![], vec![0x71; len], vec![]]);
        let mut e = Encoder::new(limits());
        e.encode(&Opaque::borrowed(b"x")).unwrap();
        e.encode(&bulk).unwrap();
        e.encode(&23_u32).unwrap();
        let p = e.finish().unwrap();
        let mut expected = vec![0, 0, 0, 1, b'x', 0, 0, 0];
        expected.extend(words(&[u32::try_from(len).unwrap()]));
        expected.extend(vec![0x71; len]);
        expected.extend(vec![0; (4 - len % 4) % 4]);
        expected.extend(words(&[23]));
        assert_eq!(flat(&p), expected);
        if len == 0 {
            assert_eq!(p.chunks().len(), 1);
        } else {
            assert_eq!(p.chunks().len(), 3); // Metadata, bulk, padding+scalar.
        }
    }
}

#[derive(XdrCodec)]
struct Generic<'a, T, const N: usize>
where
    T: std::fmt::Debug,
{
    value: T,
    name: Opaque<'a>,
    fixed: [u8; N],
}
#[derive(XdrCodec)]
struct Tuple<T>(T);
#[derive(XdrCodec)]
struct Unit;
#[derive(XdrCodec)]
struct Recursive(List<Self>);
#[derive(XdrCodec)]
enum Tagged<T> {
    #[xdr(7)]
    Value(T),
    #[xdr(9)]
    Empty,
}
#[derive(XdrDecode, XdrEncode)]
#[repr(u32)]
enum Simple {
    Zero,
    Five = 5,
    Six,
    Max = 0xffff_ffff,
}

#[test]
fn independent_direction_bounds() {
    struct DecodeOnly(u32);
    impl Decode for DecodeOnly {
        fn decode(decoder: &mut Decoder) -> DecodeResult<Self> {
            decoder.decode().map(Self)
        }
    }

    // The combined derive is present on Tuple. Borrowed Payload deliberately
    // has no Decode implementation; only the Encode bound may be required.
    let data = payload(vec![b"abc".to_vec()]);
    let pointer = data.chunks().next().unwrap().as_ptr();
    let output = plan(&Tuple(&data));
    assert_eq!(flat(&output), b"\0\0\0\x03abc\0");
    assert_eq!(output.chunks().nth(1).unwrap().as_ptr(), pointer);

    let mut decoder = Decoder::new(
        payload(vec![17_u32.to_be_bytes().to_vec()]),
        DecodeLimits {
            max_metadata_bytes: 4,
            max_collection_elements: 0,
            max_depth: 4,
            max_reservation_bytes: 4,
            payload: PayloadLimits {
                max_logical_bytes: 4,
                max_backing_capacity: 4,
                max_segments: 1,
            },
        },
    );
    let Tuple(DecodeOnly(value)) = decoder.decode::<Tuple<DecodeOnly>>().unwrap();
    assert_eq!(value, 17);
    decoder.finish().unwrap();
}

#[test]
fn generic_tuple_unit_recursive_list_and_union_forms() {
    let value = Tagged::Value(Generic {
        value: 3_u32,
        name: Opaque::borrowed(b""),
        fixed: [5_u8],
    });
    assert_eq!(
        flat(&plan(&value)),
        [0, 0, 0, 7, 0, 0, 0, 3, 0, 0, 0, 0, 5, 0, 0, 0]
    );
    assert!(plan(&Unit).is_empty());
    assert_eq!(flat(&plan(&Tuple(4_u64))), words(&[0, 4]));
    assert_eq!(flat(&plan(&Tagged::<u32>::Empty)), words(&[9]));
    let value = Recursive(List(vec![Recursive(List(vec![]))]));
    assert_eq!(flat(&plan(&value)), words(&[1, 0, 0]));
    for (value, tag) in [
        (Simple::Zero, 0),
        (Simple::Five, 5),
        (Simple::Six, 6),
        (Simple::Max, u32::MAX),
    ] {
        assert_eq!(flat(&plan(&value)), words(&[tag]));
    }
    let directory = nfs3::dirlist3 {
        entries: List(vec![nfs3::entry3 {
            fileid: 4,
            name: b"abc".as_slice().into(),
            cookie: 5,
        }]),
        eof: true,
    };
    assert_eq!(
        flat(&plan(&directory)),
        [
            0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 3, b'a', b'b', b'c', 0, 0, 0, 0, 0, 0, 0,
            0, 5, 0, 0, 0, 0, 0, 0, 0, 1,
        ]
    );
}

fn fails(value: &(impl Encode + ?Sized), policy: EncodeLimits) -> EncodeError {
    let mut e = Encoder::new(policy);
    let error = e.encode(value).unwrap_err();
    assert_eq!(e.encode(&Void), Err(error));
    assert_eq!(e.metadata(&[]), Err(error));
    assert_eq!(e.fail::<()>(EncodeError::InvalidRepresentation), Err(error));
    assert_eq!(e.finish().err(), Some(error));
    error
}

#[test]
fn aggregate_limits_have_exact_boundaries() {
    let bulk = payload(vec![vec![1], vec![2, 3]]);
    for (kind, actual) in [
        (EncodeLimit::WireBytes, 8),
        (EncodeLimit::MetadataBytes, 5), // Length + tail padding.
        (EncodeLimit::Chunks, 4),        // Prefix + two descriptors + padding.
        (EncodeLimit::RetainedCapacity, bulk.retained_capacity()),
        (EncodeLimit::Depth, 2), // Payload then u32 length.
    ] {
        for allowed in [actual - 1, actual] {
            let mut policy = limits();
            match kind {
                EncodeLimit::WireBytes => policy.max_wire_bytes = allowed,
                EncodeLimit::MetadataBytes => policy.max_metadata_bytes = allowed,
                EncodeLimit::Chunks => policy.max_chunks = allowed,
                EncodeLimit::RetainedCapacity => policy.max_retained_capacity = allowed,
                EncodeLimit::Depth => policy.max_depth = allowed,
                _ => unreachable!(),
            }
            if allowed < actual {
                assert_eq!(
                    fails(&bulk, policy),
                    EncodeError::Limit {
                        kind,
                        limit: allowed,
                        actual
                    }
                );
            } else {
                let mut e = Encoder::new(policy);
                e.encode(&bulk).unwrap();
                assert_eq!(e.finish().unwrap().len(), 8);
            }
        }
    }
    // Shared origins deliberately count once per retained descriptor/use.
    let mut policy = limits();
    policy.max_retained_capacity = bulk.retained_capacity();
    let mut e = Encoder::new(policy);
    e.encode(&bulk).unwrap();
    assert!(matches!(e.encode(&bulk), Err(EncodeError::Limit {
        kind: EncodeLimit::RetainedCapacity, actual, ..
    }) if actual == 2 * bulk.retained_capacity()));
    e.finish().unwrap_err();
}

#[test]
fn every_metadata_byte_and_reservation_is_charged() {
    for value in [
        Opaque::borrowed(b""),
        Opaque::borrowed(b"a"),
        Opaque::borrowed(b"abcd"),
    ] {
        let expected = 4 + value.len() + (4 - value.len() % 4) % 4;
        let mut policy = limits();
        policy.max_metadata_bytes = expected;
        let mut e = Encoder::new(policy);
        e.encode(&value).unwrap();
        assert_eq!(e.finish().unwrap().len(), expected);
        policy.max_metadata_bytes -= 1;
        assert!(matches!(
            fails(&value, policy),
            EncodeError::Limit {
                kind: EncodeLimit::MetadataBytes,
                ..
            }
        ));
    }
    let mut policy = limits();
    policy.max_reservation_bytes = 0;
    assert!(matches!(
        fails(&1_u32, policy),
        EncodeError::Limit {
            kind: EncodeLimit::ReservationBytes,
            ..
        }
    ));
    let mut policy = limits();
    policy.max_chunks = 0;
    assert_eq!(
        fails(&1_u32, policy),
        EncodeError::Limit {
            kind: EncodeLimit::Chunks,
            limit: 0,
            actual: 1,
        }
    );
}

struct Swallow;
impl Encode for Swallow {
    fn encode(&self, encoder: &mut Encoder) -> EncodeResult<()> {
        let _ = encoder.encode(&1_u32);
        let _ = encoder.fail::<()>(EncodeError::InvalidRepresentation);
        Ok(())
    }
}
struct CustomError;
impl Encode for CustomError {
    fn encode(&self, _: &mut Encoder) -> EncodeResult<()> {
        Err(EncodeError::InvalidRepresentation)
    }
}

#[test]
fn swallowed_nested_errors_and_custom_errors_stay_sticky() {
    let mut policy = limits();
    policy.max_depth = 1;
    assert_eq!(
        fails(&Swallow, policy),
        EncodeError::Limit {
            kind: EncodeLimit::Depth,
            limit: 1,
            actual: 2,
        }
    );
    assert_eq!(
        fails(&CustomError, limits()),
        EncodeError::InvalidRepresentation
    );
    let mut e = Encoder::new(limits());
    e.encode(&11_u32).unwrap();
    assert_eq!(
        e.encode(&CustomError),
        Err(EncodeError::InvalidRepresentation)
    );
    assert_eq!(e.finish().err(), Some(EncodeError::InvalidRepresentation));
}

#[test]
fn invalid_manual_representations_and_known_field_bounds() {
    assert_eq!(
        fails(&nfs3::mknoddata3::default, limits()),
        EncodeError::InvalidRepresentation
    );
    let bad: nfs3::Nfs3Result<Void, Void> = nfs3::Nfs3Result::Err((nfs3::nfsstat3::NFS3_OK, Void));
    assert_eq!(fails(&bad, limits()), EncodeError::InvalidRepresentation);
    assert_eq!(
        fails(&mount::mountres3::Err(mount::mountstat3::MNT3_OK), limits()),
        EncodeError::InvalidRepresentation
    );
    let field: Vec<Box<dyn Encode>> = vec![
        Box::new(nfs3::nfs_fh3 {
            data: Opaque::owned(vec![0; 65]),
        }),
        Box::new(mount::fhandle3(Opaque::owned(vec![0; 65]))),
        Box::new(mount::name(Opaque::owned(vec![0; 256]))),
        Box::new(mount::dirpath(Opaque::owned(vec![0; 1025]))),
        Box::new(rpc::opaque_auth {
            flavor: rpc::auth_flavor::AUTH_NULL,
            body: Opaque::owned(vec![0; 401]),
        }),
        Box::new(rpc::auth_unix {
            machinename: Opaque::owned(vec![0; 256]),
            ..Default::default()
        }),
    ];
    for value in field {
        assert!(matches!(
            fails(&*value, limits()),
            EncodeError::Limit {
                kind: EncodeLimit::FieldBytes,
                ..
            }
        ));
    }
    let auth = rpc::auth_unix {
        gids: vec![0; 17],
        ..Default::default()
    };
    assert_eq!(
        fails(&auth, limits()),
        EncodeError::Limit {
            kind: EncodeLimit::FieldElements,
            limit: 16,
            actual: 17,
        }
    );
    for count in [0, 16] {
        let auth = rpc::auth_unix {
            machinename: Opaque::owned(vec![1; 255]),
            gids: vec![0; count],
            ..Default::default()
        };
        let mut golden = words(&[0, 255]);
        golden.extend(vec![1; 255]);
        golden.push(0);
        golden.extend(words(&[0, 0, u32::try_from(count).unwrap()]));
        golden.extend(vec![0; count * 4]);
        assert_eq!(flat(&plan(&auth)), golden);
    }
    let handle = nfs3::nfs_fh3 {
        data: Opaque::owned(vec![1; 64]),
    };
    let mut golden = words(&[64]);
    golden.extend(vec![1; 64]);
    assert_eq!(flat(&plan(&handle)), golden);
    let mount = mount::mountres3::Ok(mount::mountres3_ok {
        fhandle: mount::fhandle3(Opaque::owned(vec![1; 64])),
        auth_flavors: vec![0, 1],
    });
    let mut golden = words(&[0, 64]);
    golden.extend(vec![1; 64]);
    golden.extend(words(&[2, 0, 1]));
    assert_eq!(flat(&plan(&mount)), golden);
    let mapping = portmap::mapping {
        prog: 1,
        vers: 2,
        prot: 6,
        port: 111,
    };
    assert_eq!(flat(&plan(&mapping)), words(&[1, 2, 6, 111]));
}

#[test]
fn cached_chunks_resume_after_short_and_interrupted_writes() {
    // Writer simulation only: RPC framing/transport changes are outside P3.
    let mut e = Encoder::new(limits());
    e.encode(&9_u32).unwrap();
    e.encode(&payload(vec![b"ab".to_vec(), b"c".to_vec()]))
        .unwrap();
    e.encode(&7_u32).unwrap();
    let p = e.finish().unwrap();
    let mut chunks = p.chunks();
    let mut current = chunks.next().unwrap();
    let mut offset = 0;
    let mut written = Vec::new();
    for (step, size) in [1, 0, 2, 3].into_iter().cycle().enumerate() {
        if step % 5 == 0 {
            continue;
        } // Interrupted: preserve all offsets.
        if size == 0 {
            continue;
        } // Pending: preserve all offsets.
        let count = size.min(current.len() - offset);
        written.extend_from_slice(&current[offset..offset + count]);
        offset += count;
        if offset == current.len() {
            if let Some(next) = chunks.next() {
                current = next;
                offset = 0;
            } else {
                break;
            }
        }
    }
    assert_eq!(written, flat(&p));
    assert_eq!(chunks.len(), 0);
    assert!(chunks.next().is_none());
    assert!(chunks.next().is_none());
}

#[cfg(target_pointer_width = "64")]
#[test]
fn unrepresentable_collection_count_is_rejected_without_traversal() {
    let values = [Void; u32::MAX as usize + 1]; // Zero-sized, no allocation.
    let mut e = Encoder::new(limits());
    let expected = EncodeError::UnrepresentableCount(values.len());
    assert_eq!(e.collection(&values, usize::MAX), Err(expected));
    assert_eq!(e.finish().err(), Some(expected));
}
