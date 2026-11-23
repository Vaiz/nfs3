// Tests for XDR collection types (BoundedVec, List, BoundedList)

use std::cell::Cell;
use std::ptr;
use std::rc::Rc;

use super::encode;
use crate::xdr_codec::{BoundedList, BoundedVec, Decoder, Error, List, Pack, Unpack};

fn stored_inline<T, const N: usize>(items: &BoundedVec<T, N>) -> bool {
    let start = ptr::from_ref(items).addr();
    (start..start + size_of_val(items)).contains(&items.as_ptr().addr())
}

#[test]
fn bounded_vec_storage_depends_on_type_and_bound() {
    #[repr(align(16))]
    struct Aligned(u32);

    let full = BoundedVec::<u32, 16>::from_array([42; 16]);
    assert!(stored_inline(&full));
    let encoded = encode(&full).unwrap();
    let mut input = Decoder::copying(encoded);
    let decoded = BoundedVec::<u32, 16>::unpack(&mut input).unwrap();
    assert_eq!(decoded, full);
    assert!(input.is_empty());
    assert!(stored_inline(&decoded));

    let converted = BoundedVec::<u32, 12>::try_from(vec![42]).unwrap();
    assert!(stored_inline(&converted));
    assert!(stored_inline(&BoundedVec::<u32, 12>::default()));

    let above_bound = BoundedVec::<u32, 17>::from_array([42]);
    assert!(!stored_inline(&above_bound));
    let larger_element = BoundedVec::<u64, 16>::from_array([42]);
    assert!(!stored_inline(&larger_element));

    let aligned = BoundedVec::<Aligned, 1>::from_array([Aligned(42)]);
    assert!(!stored_inline(&aligned));
    assert_eq!(aligned[0].0, 42);
}

#[test]
fn bounded_vec_inline_owns_items_and_supports_interior_mutability() {
    let value = Rc::new(Cell::new(42));
    let items = BoundedVec::<Rc<Cell<u32>>, 1>::from_array([Rc::clone(&value)]);
    assert!(stored_inline(&items));
    let cloned = items.clone();
    assert!(stored_inline(&cloned));
    items[0].set(7);
    assert_eq!(cloned[0].get(), 7);
    assert_eq!(Rc::strong_count(&value), 3);
    drop(items);
    drop(cloned);
    assert_eq!(Rc::strong_count(&value), 1);

    let cells = BoundedVec::<Cell<u32>, 1>::from_array([Cell::new(42)]);
    cells[0].set(7);
    assert_eq!(cells[0].get(), 7);
}

#[test]
fn bounded_vec_drops_initialized_items_after_decode_error() {
    thread_local! {
        static DROPS: Cell<usize> = const { Cell::new(0) };
    }

    struct Tracked(u32);

    impl Unpack for Tracked {
        fn unpack(input: &mut Decoder) -> crate::xdr_codec::Result<Self> {
            u32::unpack(input).map(Self)
        }
    }

    impl Drop for Tracked {
        fn drop(&mut self) {
            DROPS.with(|drops| drops.set(drops.get() + self.0 as usize));
        }
    }

    let truncated: &'static [u8] = &[0, 0, 0, 2, 0, 0, 0, 1];
    let mut input = Decoder::copying(truncated);
    assert!(BoundedVec::<Tracked, 16>::unpack(&mut input).is_err());
    assert_eq!(DROPS.with(Cell::get), 1);
}

#[test]
fn bounded_vec_serialization() {
    let original = BoundedVec::<u32, 3>::try_from(vec![0x1234, 0x5678, 0x9abc]).unwrap();
    let buffer = encode(&original).unwrap().to_vec();
    let len = buffer.len();
    assert_eq!(original.packed_size(), 16); // 4 bytes for length + 12 bytes for data
    assert_eq!(len, 16);
    assert_eq!(
        buffer,
        [
            0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x12, 0x34, 0x00, 0x00, 0x56, 0x78, 0x00, 0x00,
            0x9a, 0xbc
        ]
    );

    let mut cursor = Decoder::copying(buffer);
    let deserialized = BoundedVec::unpack(&mut cursor).unwrap();
    assert!(cursor.is_empty());
    assert_eq!(original, deserialized);
}

#[test]
fn bounded_vec_checks_count_before_reading_items() {
    let bytes: &'static [u8] = &[0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 42];
    let mut input = Decoder::copying(bytes);
    let bounded = BoundedVec::<u64, 1>::try_from(vec![42]).unwrap();
    assert_eq!(encode(&bounded).unwrap().as_ref(), bytes);
    assert_eq!(BoundedVec::unpack(&mut input).unwrap(), bounded);
    assert!(input.is_empty());

    assert_eq!(
        BoundedVec::<u64, 0>::try_from(vec![42]),
        Err(Error::InvalidLength(1))
    );
    let mut input = Decoder::copying(bytes);
    assert!(matches!(
        BoundedVec::<u64, 0>::unpack(&mut input),
        Err(Error::InvalidLength(1))
    ));
    assert_eq!(input.remaining(), 8);

    let too_many: &'static [u8] = &[0, 0, 0, 17];
    let mut input = Decoder::copying(too_many);
    assert!(matches!(
        BoundedVec::<u32, 16>::unpack(&mut input),
        Err(Error::InvalidLength(17))
    ));
}

#[test]
fn list_serialization() {
    let original = List(vec![0x1234, 0x5678, 0x9abc]);
    let buffer = encode(&original).unwrap().to_vec();
    let len = buffer.len();
    assert_eq!(original.packed_size(), 28);
    assert_eq!(len, 28);
    assert_eq!(
        buffer,
        [
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x12, 0x34, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
            0x56, 0x78, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x9a, 0xbc, 0x00, 0x00, 0x00, 0x00,
        ]
    );

    let mut cursor = Decoder::copying(buffer);
    let deserialized = List::<u32>::unpack(&mut cursor).unwrap();
    assert!(cursor.is_empty());
    assert_eq!(original.0, deserialized.0);
}

#[test]
fn bounded_list_basic() {
    // (4+4)*3 + 4 = 28 bytes
    let mut bounded_list = BoundedList::<u32>::new(28);
    bounded_list.try_push(0x1234).unwrap();
    bounded_list.try_push(0x5678).unwrap();
    bounded_list.try_push(0x9abc).unwrap();
    assert!(bounded_list.try_push(0xdef0).is_err());

    let list = bounded_list.into_inner();
    assert_eq!(list.0, vec![0x1234, 0x5678, 0x9abc]);
}

#[test]
fn bounded_list_exact_boundary() {
    let mut bounded_list = BoundedList::<u32>::new(27);
    bounded_list.try_push(0x1234).unwrap();
    bounded_list.try_push(0x5678).unwrap();
    assert!(bounded_list.try_push(0x9abc).is_err());

    let list = bounded_list.into_inner();
    assert_eq!(list.0, vec![0x1234, 0x5678]);

    let mut bounded_list = BoundedList::<u32>::new(29);
    bounded_list.try_push(0x1234).unwrap();
    bounded_list.try_push(0x5678).unwrap();
    bounded_list.try_push(0x9abc).unwrap();
    assert!(bounded_list.try_push(0xdef0).is_err());

    let list = bounded_list.into_inner();
    assert_eq!(list.0, vec![0x1234, 0x5678, 0x9abc]);
}

#[test]
fn empty_collections() {
    // Test empty BoundedVec
    let empty_vec = BoundedVec::<u32, 1>::default();
    let buffer = encode(&empty_vec).unwrap().to_vec();
    let len = buffer.len();
    assert_eq!(len, 4); // Just the length field
    assert_eq!(buffer, [0, 0, 0, 0]);

    let mut cursor = Decoder::copying(buffer);
    let deserialized = BoundedVec::unpack(&mut cursor).unwrap();
    assert!(cursor.is_empty());
    assert_eq!(empty_vec, deserialized);

    // Test empty List
    let empty_list = List(Vec::<u32>::new());
    let buffer = encode(&empty_list).unwrap().to_vec();
    let len = buffer.len();
    assert_eq!(len, 4); // Just the end marker
    assert_eq!(buffer, [0, 0, 0, 0]);

    let mut cursor = Decoder::copying(buffer);
    let deserialized = List::<u32>::unpack(&mut cursor).unwrap();
    assert!(cursor.is_empty());
    assert_eq!(empty_list.0, deserialized.0);
}

#[test]
fn single_item_collections() {
    // Test single item BoundedVec
    let single_vec = BoundedVec::<u32, 1>::try_from(vec![0x1234_5678]).unwrap();
    let buffer = encode(&single_vec).unwrap().to_vec();
    let len = buffer.len();
    assert_eq!(len, 8); // 4 bytes length + 4 bytes data
    assert_eq!(buffer, [0, 0, 0, 1, 0x12, 0x34, 0x56, 0x78]);

    let mut cursor = Decoder::copying(buffer);
    let deserialized = BoundedVec::unpack(&mut cursor).unwrap();
    assert!(cursor.is_empty());
    assert_eq!(single_vec, deserialized);

    // Test single item List
    let single_list = List(vec![0x1234_5678]);
    let buffer = encode(&single_list).unwrap().to_vec();
    let len = buffer.len();
    assert_eq!(len, 12); // 4 bytes indicator + 4 bytes data + 4 bytes end marker
    assert_eq!(buffer, [0, 0, 0, 1, 0x12, 0x34, 0x56, 0x78, 0, 0, 0, 0]);

    let mut cursor = Decoder::copying(buffer);
    let deserialized = List::<u32>::unpack(&mut cursor).unwrap();
    assert!(cursor.is_empty());
    assert_eq!(single_list.0, deserialized.0);
}

#[test]
fn bounded_list_edge_cases() {
    // Test with zero size limit (should not allow any items)
    // Even zero size should have the end marker, so it should be at least 4
    let mut bounded_list = BoundedList::<u32>::new(4); // Just end marker
    assert!(bounded_list.try_push(0x1234).is_err());

    // Test with exactly one item size
    // Each item needs 4 bytes indicator + 4 bytes data, plus 4 bytes end marker = 12 bytes total
    let mut bounded_list = BoundedList::<u32>::new(12);
    bounded_list.try_push(0x1234).unwrap();
    assert!(bounded_list.try_push(0x5678).is_err());

    let list = bounded_list.into_inner();
    assert_eq!(list.0, vec![0x1234]);
}

#[test]
fn large_collections() {
    // Test with larger collections to verify scaling
    let large_vec = BoundedVec::<u32, 100>::try_from((0..100).collect::<Vec<_>>()).unwrap();
    let buffer = encode(&large_vec).unwrap().to_vec();
    let len = buffer.len();
    assert_eq!(len, 404); // 4 bytes length + 100 * 4 bytes data

    let mut cursor = Decoder::copying(buffer);
    let deserialized = BoundedVec::unpack(&mut cursor).unwrap();
    assert!(cursor.is_empty());
    assert_eq!(large_vec, deserialized);

    // Test bounded list with many items
    // Each item needs 4 bytes indicator + 4 bytes data = 8 bytes per item
    // Plus 4 bytes for the end marker
    // So for 100 items: 100 * 8 + 4 = 804 bytes
    let mut bounded_list = BoundedList::<u32>::new(804);
    for i in 0..100 {
        assert!(bounded_list.try_push(i).is_ok(), "Failed to push item {i}");
    }
    assert!(bounded_list.try_push(100).is_err()); // Should fail on 101st item

    let list = bounded_list.into_inner();
    assert_eq!(list.0.len(), 100);
    assert_eq!(list.0, (0..100).collect::<Vec<u32>>());
}
