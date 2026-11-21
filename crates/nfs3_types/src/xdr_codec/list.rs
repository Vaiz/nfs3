use derive_where::derive_where;

use crate::xdr_codec::{Decoder, Encoder, Pack, Result, Unpack};

/// Represents a sequence of optional values in NFS3.
///
/// This struct is a wrapper around a `Vec<T>`, where `T` is a type that implements
/// the [`Pack`] and [`Unpack`] traits for serialization and deserialization.
#[derive(Debug)]
#[derive_where(Default)]
pub struct List<T>(pub Vec<T>);

impl<T> List<T> {
    #[must_use]
    pub fn into_inner(self) -> Vec<T> {
        self.0
    }
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<T> Pack for List<T>
where
    T: Pack,
{
    fn packed_size(&self) -> usize {
        let mut len = 0;
        for item in &self.0 {
            len += <bool as Pack>::packed_size(&true);
            len += item.packed_size();
        }
        len += <bool as Pack>::packed_size(&false);
        len
    }

    fn pack(&self, out: &mut Encoder) -> Result<()> {
        for item in &self.0 {
            true.pack(out)?;
            item.pack(out)?;
        }
        false.pack(out)
    }
}

impl<T> Unpack for List<T>
where
    T: Unpack,
{
    fn unpack(input: &mut Decoder) -> Result<Self> {
        let mut items = Vec::new();
        while bool::unpack(input)? {
            items.push(T::unpack(input)?);
        }
        Ok(Self(items))
    }
}

pub struct BoundedList<T> {
    list: List<T>,
    current_size: usize,
    max_size: usize,
}

impl<T> BoundedList<T>
where
    T: Pack,
{
    #[must_use]
    pub fn new(max_size: usize) -> Self {
        let list = List(Vec::new());
        let current_size = <List<T> as Pack>::packed_size(&list);
        Self {
            list,
            current_size,
            max_size,
        }
    }

    pub fn try_push(&mut self, item: T) -> std::result::Result<(), T> {
        let item_size = item.packed_size() + 4;
        if self.current_size + item_size > self.max_size {
            return Err(item);
        }

        self.list.0.push(item);
        self.current_size += item_size;
        Ok(())
    }

    #[must_use]
    pub fn into_inner(self) -> List<T> {
        self.list
    }
}
