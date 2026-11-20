use nfs3_types::xdr_codec::{BoundedList, Pack};

#[test]
fn finite_entry_growth_and_wire_terminal_space() {
    for (wire, max_entries, expected) in [(112, 9, 9), (40, 9, 3), (112, 0, 0)] {
        for count in 0..12 {
            let mut list = BoundedList::with_entry_limit(wire, max_entries);
            for value in 0..count {
                assert_eq!(list.try_push(value as u64).is_ok(), value < expected);
            }
            let list = list.into_inner();
            assert_eq!(list.0.len(), count.min(expected));
            assert!(list.0.capacity() <= max_entries);
            assert!(list.packed_size() <= wire);
            assert_eq!(list.0, (0..count.min(expected) as u64).collect::<Vec<_>>());
        }
    }
}
