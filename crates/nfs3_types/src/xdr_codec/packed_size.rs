use xdr_codec::Opaque;

pub trait PackedSize {
    const PACKED_SIZE: Option<usize>;

    fn packed_size(&self) -> usize {
        Self::PACKED_SIZE.unwrap_or(self.count_packed_size())
    }

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
    sz + (4 - (sz % 4))
}
