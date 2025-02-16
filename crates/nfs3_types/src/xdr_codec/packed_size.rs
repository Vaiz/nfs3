use xdr_codec::Opaque;

/// A trait for calculating the packed size of an object.
///
/// This trait provides a way to determine the packed size of an object, either through a constant
/// value or by calculating it dynamically.
pub trait PackedSize {
    /// An optional constant representing the packed size of the type. If this is `Some`, it
    /// indicates that the type has a constant size. If it is `None`, the packed size will be
    /// calculated using the `count_packed_size` method.
    const PACKED_SIZE: Option<usize>;

    /// Returns the packed size of the object. If `PACKED_SIZE` is `Some`, it returns that value.
    /// Otherwise, it calls `count_packed_size` to calculate the size.
    fn packed_size(&self) -> usize {
        Self::PACKED_SIZE.unwrap_or(self.count_packed_size())
    }

    /// Calculates the packed size of the object dynamically.
    fn count_packed_size(&self) -> usize;
}

impl PackedSize for bool {
    const PACKED_SIZE: Option<usize> = Some(4);

    fn count_packed_size(&self) -> usize {
        4
    }
}

impl PackedSize for u32 {
    const PACKED_SIZE: Option<usize> = Some(4);

    fn count_packed_size(&self) -> usize {
        4
    }
}

impl PackedSize for u64 {
    const PACKED_SIZE: Option<usize> = Some(8);

    fn count_packed_size(&self) -> usize {
        8
    }
}

impl PackedSize for Opaque<'_> {
    const PACKED_SIZE: Option<usize> = None;

    fn count_packed_size(&self) -> usize {
        4 + add_padding(self.len())
    }
}

impl PackedSize for [u8] {
    const PACKED_SIZE: Option<usize> = None;

    fn count_packed_size(&self) -> usize {
        4 + add_padding(self.len())
    }
}

impl<T> PackedSize for Vec<T>
where
    T: PackedSize,
{
    const PACKED_SIZE: Option<usize> = None;

    fn count_packed_size(&self) -> usize {
        let mut size = 4;
        if let Some(const_len) = T::PACKED_SIZE {
            size += const_len * self.len();
        } else {
            for item in self {
                size += item.packed_size();
            }
        }
        size
    }
}

#[inline]
pub(crate) fn add_padding(sz: usize) -> usize {
    (sz + 3) & !3
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_packed_size() {
        assert_eq!(true.packed_size(), 4);
        assert_eq!(false.packed_size(), 4);
        assert_eq!(u32::PACKED_SIZE, Some(4));
        assert_eq!(u32::PACKED_SIZE, Some(4));
        assert_eq!(u64::PACKED_SIZE, Some(8));
        assert_eq!(u64::PACKED_SIZE, Some(8));
        assert_eq!(Opaque::borrowed(&[]).packed_size(), 4);
        assert_eq!(Opaque::borrowed(&[0, 1, 2, 3]).packed_size(), 8);

        assert_eq!([].packed_size(), 4);
        assert_eq!([1u8].packed_size(), 8);
        assert_eq!([0u8, 1, 2, 3].packed_size(), 8);
        assert_eq!(vec![0u32, 1, 2, 3].packed_size(), 20);
    }

    #[test]
    fn test_add_padding() {
        assert_eq!(add_padding(0), 0);
        assert_eq!(add_padding(1), 4);
        assert_eq!(add_padding(2), 4);
        assert_eq!(add_padding(3), 4);
        assert_eq!(add_padding(4), 4);
        assert_eq!(add_padding(5), 8);
        assert_eq!(add_padding(6), 8);
        assert_eq!(add_padding(7), 8);
        assert_eq!(add_padding(8), 8);
    }
}
