use super::{Decoder, Error, Result, Unpack};

/// An XDR counted array with at most `N` elements.
pub(crate) struct BoundedVec<T, const N: usize>(Vec<T>);

impl<T, const N: usize> BoundedVec<T, N> {
    pub(crate) fn into_inner(self) -> Vec<T> {
        self.0
    }
}

impl<T: Unpack, const N: usize> Unpack for BoundedVec<T, N> {
    fn unpack(input: &mut Decoder) -> Result<Self> {
        let len = u32::unpack(input)? as usize;
        if len > N {
            return Err(Error::InvalidLength(len));
        }

        // Grow only after decoding an item; the count alone does not prove it is present.
        let mut items = Vec::new();
        for _ in 0..len {
            items.push(T::unpack(input)?);
        }
        Ok(Self(items))
    }
}
