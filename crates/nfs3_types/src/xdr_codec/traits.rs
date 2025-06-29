use std::io::{Read, Write};

use super::Result;

/// Packing trait
pub trait Pack {
    /// Returns the packed size of the type.
    ///
    /// This should include any padding that is required to align the data.
    /// This is used to determine how much space to allocate when packing the type.
    fn packed_size(&self) -> usize;

    /// Packs the type into a byte slice.
    fn pack(&self, out: &mut impl Write) -> Result<usize>;
}

/// Unpacking trait
pub trait Unpack: Sized {
    /// Unpacks the type from a byte slice.
    fn unpack(input: &mut impl Read) -> Result<(Self, usize)>;
}
