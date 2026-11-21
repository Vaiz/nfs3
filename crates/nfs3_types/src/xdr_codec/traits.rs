use super::{Decoder, Encoder, Result};

/// Packing trait
pub trait Pack {
    /// Returns the packed size of the type.
    ///
    /// This should include any padding that is required to align the data.
    fn packed_size(&self) -> usize;

    /// Appends the XDR representation of the value to `out`.
    fn pack(&self, out: &mut Encoder) -> Result<()>;
}

/// Unpacking trait
pub trait Unpack: Sized {
    /// Decodes a value from the front of `input`.
    fn unpack(input: &mut Decoder) -> Result<Self>;
}
