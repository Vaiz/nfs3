use std::io::{Read, Write};

use crate::xdr_codec::{Pack, Unpack};

/// Represents a sequence of optional values in NFS3.
///
/// This struct is a wrapper around a `Vec<T>`, where `T` is a type that implements
/// the [`Pack`] and [`Unpack`] traits for serialization and deserialization.
#[derive(Debug)]
pub struct List<T>(pub Vec<T>);

impl<T> Default for List<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T> List<T> {
    #[must_use]
    pub fn into_inner(self) -> Vec<T> {
        self.0
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
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

    fn pack(&self, output: &mut impl Write) -> crate::xdr_codec::Result<usize> {
        let mut len = 0;
        for item in &self.0 {
            len += true.pack(output)?;
            len += item.pack(output)?;
        }
        len += false.pack(output)?;
        Ok(len)
    }
}

impl<T> Unpack for List<T>
where
    T: Unpack,
{
    fn unpack(input: &mut impl Read) -> crate::xdr_codec::Result<(Self, usize)> {
        let mut items = Vec::new();
        let mut len = 0;
        loop {
            let (more, more_len) = bool::unpack(input)?;
            len += more_len;
            if !more {
                break;
            }
            let (item, item_len) = T::unpack(input)?;
            len += item_len;
            items.push(item);
        }
        Ok((Self(items), len))
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

    pub fn try_push(&mut self, item: T) -> Result<(), T> {
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
