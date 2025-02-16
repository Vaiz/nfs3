use std::io::{Read, Write};

use xdr_codec::{Pack, Unpack};

use crate::xdr_codec::PackedSize;

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

impl<T, Out> Pack<Out> for List<T>
where
    Out: Write,
    T: Pack<Out>,
{
    fn pack(&self, output: &mut Out) -> xdr_codec::Result<usize> {
        let mut len = 0;
        for item in &self.0 {
            len += true.pack(output)?;
            len += item.pack(output)?;
        }
        len += false.pack(output)?;
        Ok(len)
    }
}

impl<T> PackedSize for List<T>
where
    T: PackedSize,
{
    const PACKED_SIZE: Option<usize> = None;

    fn count_packed_size(&self) -> usize {
        if let Some(const_len) = T::PACKED_SIZE {
            return (4 + const_len) * self.0.len() + 4;
        }

        let mut len = 0;
        for item in &self.0 {
            len += true.packed_size();
            len += item.packed_size();
        }
        len += false.packed_size();
        len
    }
}

impl<T, In> Unpack<In> for List<T>
where
    In: Read,
    T: Unpack<In>,
{
    fn unpack(input: &mut In) -> xdr_codec::Result<(Self, usize)> {
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
        Ok((List(items), len))
    }
}

pub struct BoundedList<T> {
    list: List<T>,
    current_size: usize,
    max_size: usize,
}

impl<T> BoundedList<T>
where
    T: PackedSize,
{
    pub fn new(max_size: usize) -> Self {
        let list = List(Vec::new());
        let current_size = list.packed_size();
        BoundedList {
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

    pub fn into_inner(self) -> List<T> {
        self.list
    }
}
