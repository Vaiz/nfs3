//! This module is mainly a reexport of the `xdr_codec` crate, with some additional types and traits.
//!
//! > NOTE: `xdr_codec` crate has been updated in a long time, so it might be replaced in the future.

pub use ::xdr_codec::*;
/// Derive macro that implements [`Pack`] and [`Unpack`] traits.
pub use nfs3_macros::XdrCodec;

/// Represents a sequence of optional values in NFS3.
///
/// This struct is a wrapper around a `Vec<T>`, where `T` is a type that implements
/// the [`Pack`] and [`Unpack`] traits for serialization and deserialization.
#[derive(Debug)]
pub struct List<T>(pub Vec<T>);

impl<T, Out> Pack<Out> for List<T>
where
    Out: Write,
    T: Pack<Out>,
{
    fn pack(&self, output: &mut Out) -> Result<usize> {
        let mut len = 0;
        for item in &self.0 {
            len += true.pack(output)?;
            len += item.pack(output)?;
        }
        len += false.pack(output)?;
        Ok(len)
    }
}

impl<T, In> Unpack<In> for List<T>
where
    In: Read,
    T: Unpack<In>,
{
    fn unpack(input: &mut In) -> Result<(Self, usize)> {
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
