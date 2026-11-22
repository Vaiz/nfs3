use std::fmt;
use std::ops::Deref;

use bytes::Bytes;

use crate::xdr_codec::util::add_padding;
use crate::xdr_codec::{Decoder, Encoder, Error, Pack, Result, Unpack};

/// Variable-length XDR opaque data.
///
/// Either borrows a slice or holds a reference-counted [`Bytes`]. Decoded values
/// hold `Bytes` that either shares the decoder's input buffer or is a copy (see
/// [`Decoder`]).
#[derive(Clone)]
pub struct Opaque<'a>(Repr<'a>);

#[derive(Clone)]
enum Repr<'a> {
    Borrowed(&'a [u8]),
    Shared(Bytes),
}

impl Opaque<'static> {
    /// Creates an `Opaque` that owns `data` without copying it.
    #[must_use]
    pub fn owned(data: Vec<u8>) -> Self {
        Self::from_bytes(Bytes::from(data))
    }

    /// Creates an `Opaque` from reference-counted bytes without copying them.
    #[must_use]
    pub const fn from_bytes(data: Bytes) -> Self {
        Self(Repr::Shared(data))
    }
}

impl<'a> Opaque<'a> {
    /// Creates an `Opaque` from a borrowed slice.
    #[must_use]
    pub const fn borrowed(data: &'a [u8]) -> Self {
        Self(Repr::Borrowed(data))
    }

    /// Converts into an `Opaque` that owns its bytes.
    ///
    /// Copies borrowed data; `Bytes` data is moved as is.
    #[must_use]
    pub(crate) fn into_owned(self) -> Opaque<'static> {
        Opaque::from_bytes(self.into_bytes())
    }

    /// Converts into [`Bytes`], copying only if the data is borrowed.
    #[must_use]
    pub fn into_bytes(self) -> Bytes {
        match self.0 {
            Repr::Borrowed(data) => Bytes::copy_from_slice(data),
            Repr::Shared(data) => data,
        }
    }
}

impl Pack for Opaque<'_> {
    fn packed_size(&self) -> usize {
        4 + add_padding(self.len())
    }

    fn pack(&self, out: &mut Encoder) -> Result<()> {
        u32::try_from(self.len())
            .map_err(|_overflow| Error::ObjectTooLarge(self.len()))?
            .pack(out)?;
        match &self.0 {
            Repr::Borrowed(data) => out.put_padded(data),
            Repr::Shared(data) => out.put_padded_shared(data),
        }
        Ok(())
    }
}

impl Unpack for Opaque<'static> {
    fn unpack(input: &mut Decoder) -> Result<Self> {
        let len = u32::unpack(input)? as usize;
        input.read_opaque(len).map(Self::from_bytes)
    }
}

impl Deref for Opaque<'_> {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match &self.0 {
            Repr::Borrowed(data) => data,
            Repr::Shared(data) => data,
        }
    }
}

impl AsRef<[u8]> for Opaque<'_> {
    fn as_ref(&self) -> &[u8] {
        self
    }
}

impl fmt::Debug for Opaque<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Opaque").field(&&**self).finish()
    }
}

impl PartialEq for Opaque<'_> {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl Eq for Opaque<'_> {}

/// Variable-length XDR opaque data limited to `N` bytes.
///
/// Uses the same wire format and storage as [`Opaque`]. Decoding checks the
/// length before reading or copying the payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundedOpaque<'a, const N: usize>(Opaque<'a>);

impl<const N: usize> BoundedOpaque<'_, N> {
    pub(crate) const fn empty() -> Self {
        Self(Opaque::borrowed(&[]))
    }

    pub(crate) fn borrow(&self) -> BoundedOpaque<'_, N> {
        BoundedOpaque(Opaque::borrowed(self.as_ref()))
    }

    /// Converts into [`Bytes`], copying only if the data is borrowed.
    #[must_use]
    pub fn into_bytes(self) -> Bytes {
        self.0.into_bytes()
    }
}

impl<'a, const N: usize> TryFrom<Opaque<'a>> for BoundedOpaque<'a, N> {
    type Error = Error;

    fn try_from(data: Opaque<'a>) -> Result<Self> {
        if data.len() > N {
            return Err(Error::InvalidLength(data.len()));
        }
        Ok(Self(data))
    }
}

impl<const N: usize> AsRef<[u8]> for BoundedOpaque<'_, N> {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl<const N: usize> Deref for BoundedOpaque<'_, N> {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        self.as_ref()
    }
}

impl<const N: usize> Pack for BoundedOpaque<'_, N> {
    fn packed_size(&self) -> usize {
        self.0.packed_size()
    }

    fn pack(&self, out: &mut Encoder) -> Result<()> {
        self.0.pack(out)
    }
}

impl<const N: usize> Unpack for BoundedOpaque<'static, N> {
    fn unpack(input: &mut Decoder) -> Result<Self> {
        let len = u32::unpack(input)? as usize;
        if len > N {
            return Err(Error::InvalidLength(len));
        }
        Ok(Self(Opaque::from_bytes(input.read_opaque(len)?)))
    }
}
