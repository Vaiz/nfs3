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

    fn pack(&self, out: &mut impl Write) -> crate::xdr_codec::Result<usize> {
        let mut len = 0;
        for item in &self.0 {
            len += true.pack(out)?;
            len += item.pack(out)?;
        }
        len += false.pack(out)?;
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
    max_entries: Option<usize>,
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
            max_entries: None,
        }
    }

    /// Also bound host entries and use explicit fallible geometric growth.
    /// Reservation work/old-new overlap must be reserved by the caller.
    #[must_use]
    pub fn with_entry_limit(max_size: usize, max_entries: usize) -> Self {
        Self {
            max_entries: Some(max_entries),
            ..Self::new(max_size)
        }
    }

    pub fn try_push(&mut self, item: T) -> Result<(), T> {
        let Some(item_size) = item.packed_size().checked_add(4) else {
            return Err(item);
        };
        let Some(next_size) = self.current_size.checked_add(item_size) else {
            return Err(item);
        };
        if next_size > self.max_size {
            return Err(item);
        }

        if let Some(max) = self.max_entries {
            let entries = &mut self.list.0;
            if entries.len() >= max {
                return Err(item);
            }
            if entries.len() == entries.capacity() {
                let target = entries
                    .capacity()
                    .checked_mul(2)
                    .unwrap_or(max)
                    .max(1)
                    .min(max);
                let mut grown = Vec::new();
                if grown.try_reserve_exact(target).is_err() {
                    return Err(item);
                }
                if grown.capacity() > target {
                    // Reject allocator excess before publishing a bounded list.
                    // Keep the accepted page unchanged. Old+new overlap is part
                    // of the caller's envelope; usable-size overhead is not.
                    return Err(item);
                }
                grown.append(entries);
                *entries = grown;
            }
        }
        self.list.0.push(item);
        self.current_size = next_size;
        Ok(())
    }

    #[must_use]
    pub fn into_inner(self) -> List<T> {
        self.list
    }
}
