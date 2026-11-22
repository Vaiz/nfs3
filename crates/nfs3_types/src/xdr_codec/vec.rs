use super::{Decoder, Encoder, Error, Pack, Result, Unpack};

/// An XDR counted array with at most `N` elements.
///
/// Construct it from a vector with [`TryFrom`], or use [`Default`] for an empty array.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundedVec<T, const N: usize>(Vec<T>);

impl<T, const N: usize> Default for BoundedVec<T, N> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T, const N: usize> TryFrom<Vec<T>> for BoundedVec<T, N> {
    type Error = Error;

    fn try_from(items: Vec<T>) -> Result<Self> {
        if items.len() > N {
            return Err(Error::InvalidLength(items.len()));
        }
        Ok(Self(items))
    }
}

impl<T, const N: usize> AsRef<[T]> for BoundedVec<T, N> {
    fn as_ref(&self) -> &[T] {
        &self.0
    }
}

impl<T: Pack, const N: usize> Pack for BoundedVec<T, N> {
    fn packed_size(&self) -> usize {
        4 + self.0.iter().map(Pack::packed_size).sum::<usize>()
    }

    fn pack(&self, out: &mut Encoder) -> Result<()> {
        u32::try_from(self.0.len())
            .map_err(|_overflow| Error::ObjectTooLarge(self.0.len()))?
            .pack(out)?;
        for item in &self.0 {
            item.pack(out)?;
        }
        Ok(())
    }
}

impl<T: Unpack, const N: usize> Unpack for BoundedVec<T, N> {
    fn unpack(input: &mut Decoder) -> Result<Self> {
        let len = u32::unpack(input)? as usize;
        if len > N {
            return Err(Error::InvalidLength(len));
        }

        let mut items = Vec::with_capacity(len);
        for _ in 0..len {
            items.push(T::unpack(input)?);
        }
        Ok(Self(items))
    }
}
