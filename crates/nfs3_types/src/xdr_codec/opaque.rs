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

// Enough for both NFS and MOUNT file handles.
const INLINE_CAPACITY: usize = 64;

#[derive(Clone)]
enum BoundedRepr<'a> {
    Opaque(Opaque<'a>),
    Inline {
        data: [u8; INLINE_CAPACITY],
        len: usize,
    },
}

/// Variable-length XDR opaque data limited to `N` bytes.
///
/// Uses the same wire format as [`Opaque`]. Decoding checks the length before
/// reading the payload. For `N <= 64`, decoding and owning a borrowed slice
/// copy the payload inline instead of allocating.
#[derive(Clone)]
pub struct BoundedOpaque<'a, const N: usize>(BoundedRepr<'a>);

impl<const N: usize> BoundedOpaque<'_, N> {
    pub(crate) const fn empty() -> Self {
        Self(BoundedRepr::Opaque(Opaque::borrowed(&[])))
    }

    pub(crate) fn borrow(&self) -> BoundedOpaque<'_, N> {
        BoundedOpaque(BoundedRepr::Opaque(Opaque::borrowed(self.as_ref())))
    }

    /// Borrows an array whose length is checked against `N` at compile time.
    ///
    /// ```compile_fail
    /// use nfs3_types::xdr_codec::BoundedOpaque;
    ///
    /// const _: BoundedOpaque<'static, 3> = BoundedOpaque::from_array(b"four");
    /// ```
    #[must_use]
    pub const fn from_array<const LEN: usize>(data: &[u8; LEN]) -> BoundedOpaque<'_, N> {
        const { assert!(LEN <= N, "array exceeds the XDR opaque length limit") };
        BoundedOpaque(BoundedRepr::Opaque(Opaque::borrowed(data)))
    }

    /// Borrow a slice as a `BoundedOpaque`.
    pub fn from_slice(data: &[u8]) -> Result<BoundedOpaque<'_, N>> {
        Opaque::borrowed(data).try_into()
    }

    /// Converts into [`Bytes`], copying if the data is borrowed or inline.
    #[must_use]
    pub fn into_bytes(self) -> Bytes {
        match self.0 {
            BoundedRepr::Opaque(data) => data.into_bytes(),
            BoundedRepr::Inline { data, len } => Bytes::copy_from_slice(&data[..len]),
        }
    }

    /// Converts into an owned `BoundedOpaque`.
    #[must_use]
    pub fn into_owned(self) -> BoundedOpaque<'static, N> {
        BoundedOpaque(match self.0 {
            BoundedRepr::Opaque(Opaque(Repr::Borrowed(bytes))) if N <= INLINE_CAPACITY => {
                let mut data = [0; INLINE_CAPACITY];
                let len = bytes.len();
                data[..len].copy_from_slice(bytes);
                BoundedRepr::Inline { data, len }
            }
            BoundedRepr::Opaque(data) => BoundedRepr::Opaque(data.into_owned()),
            BoundedRepr::Inline { data, len } => BoundedRepr::Inline { data, len },
        })
    }
}

impl<const N: usize> BoundedOpaque<'static, N> {
    /// Wraps owned bytes without copying.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidLength`] if the input is longer than `N`.
    pub fn try_from_bytes(data: Bytes) -> Result<Self> {
        Opaque::from_bytes(data).try_into()
    }
}

impl<'a, const N: usize> TryFrom<Opaque<'a>> for BoundedOpaque<'a, N> {
    type Error = Error;

    fn try_from(data: Opaque<'a>) -> Result<Self> {
        if data.len() > N {
            return Err(Error::InvalidLength(data.len()));
        }
        Ok(Self(BoundedRepr::Opaque(data)))
    }
}

impl<const N: usize> AsRef<[u8]> for BoundedOpaque<'_, N> {
    fn as_ref(&self) -> &[u8] {
        match &self.0 {
            BoundedRepr::Opaque(data) => data,
            BoundedRepr::Inline { data, len } => &data[..*len],
        }
    }
}

impl<const N: usize> Deref for BoundedOpaque<'_, N> {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        self.as_ref()
    }
}

impl<const N: usize> fmt::Debug for BoundedOpaque<'_, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("BoundedOpaque")
            .field(&Opaque::borrowed(self))
            .finish()
    }
}

impl<const N: usize> PartialEq for BoundedOpaque<'_, N> {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl<const N: usize> Eq for BoundedOpaque<'_, N> {}

impl<const N: usize> Pack for BoundedOpaque<'_, N> {
    fn packed_size(&self) -> usize {
        4 + add_padding(self.len())
    }

    fn pack(&self, out: &mut Encoder) -> Result<()> {
        match &self.0 {
            BoundedRepr::Opaque(data) => data.pack(out),
            BoundedRepr::Inline { data, len } => Opaque::borrowed(&data[..*len]).pack(out),
        }
    }
}

impl<const N: usize> Unpack for BoundedOpaque<'static, N> {
    fn unpack(input: &mut Decoder) -> Result<Self> {
        let len = u32::unpack(input)? as usize;
        if len > N {
            return Err(Error::InvalidLength(len));
        }
        let repr = if N <= INLINE_CAPACITY {
            let mut data = [0; INLINE_CAPACITY];
            input.read_opaque_into(&mut data[..len])?;
            BoundedRepr::Inline { data, len }
        } else {
            BoundedRepr::Opaque(Opaque::from_bytes(input.read_opaque(len)?))
        };
        Ok(Self(repr))
    }
}
