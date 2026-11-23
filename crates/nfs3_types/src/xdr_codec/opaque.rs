use std::fmt;
use std::ops::Deref;

use bytes::Bytes;

use crate::xdr_codec::util::add_padding;
use crate::xdr_codec::{Decoder, Encoder, Error, Pack, Result, Unpack};

/// Variable-length XDR opaque data.
///
/// Borrows a slice, stores a short payload inline, or holds a reference-counted [`Bytes`].
/// Decoded payloads of at most 22 bytes are stored inline. Larger payloads
/// may share the decoder's input buffer (see [`Decoder`]).
#[derive(Clone)]
pub struct Opaque<'a>(Repr<'a>);

#[derive(Clone)]
enum Repr<'a> {
    Local(Local<'a>),
    Shared(Bytes),
}

// Group the two small variants so rustc can use the Bytes pointer niche for
// Local. On x86-64 the 24-byte Local fits alongside that niche in 32 bytes.
#[derive(Clone)]
enum Local<'a> {
    Borrowed(&'a [u8]),
    Inline {
        data: [u8; INLINE_CAPACITY],
        len: u8,
    },
}

const INLINE_CAPACITY: usize = 22;

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

    fn inline(data: [u8; INLINE_CAPACITY], len: usize) -> Self {
        let len = u8::try_from(len).expect("inline length fits in a u8");
        Self(Repr::Local(Local::Inline { data, len }))
    }

    fn unpack_payload(input: &mut Decoder, len: usize) -> Result<Self> {
        if len <= INLINE_CAPACITY {
            let mut data = [0; INLINE_CAPACITY];
            input.read_opaque_into(&mut data[..len])?;
            Ok(Self::inline(data, len))
        } else {
            input.read_opaque(len).map(Self::from_bytes)
        }
    }
}

impl<'a> Opaque<'a> {
    /// Creates an `Opaque` from a borrowed slice.
    #[must_use]
    pub const fn borrowed(data: &'a [u8]) -> Self {
        Self(Repr::Local(Local::Borrowed(data)))
    }

    /// Converts into an `Opaque` that owns its bytes.
    ///
    /// Copies borrowed data, storing up to 22 bytes inline. Owned data is
    /// moved as is.
    #[must_use]
    pub(crate) fn into_owned(self) -> Opaque<'static> {
        match self.0 {
            Repr::Local(Local::Borrowed(bytes)) if bytes.len() <= INLINE_CAPACITY => {
                let mut data = [0; INLINE_CAPACITY];
                data[..bytes.len()].copy_from_slice(bytes);
                Opaque::inline(data, bytes.len())
            }
            Repr::Local(Local::Borrowed(data)) => Opaque::from_bytes(Bytes::copy_from_slice(data)),
            Repr::Local(Local::Inline { data, len }) => {
                Opaque(Repr::Local(Local::Inline { data, len }))
            }
            Repr::Shared(data) => Opaque::from_bytes(data),
        }
    }

    /// Converts into [`Bytes`], copying if the data is borrowed or inline.
    #[must_use]
    pub fn into_bytes(self) -> Bytes {
        match self.0 {
            Repr::Local(Local::Borrowed(data)) => Bytes::copy_from_slice(data),
            Repr::Local(Local::Inline { data, len }) => {
                Bytes::copy_from_slice(&data[..usize::from(len)])
            }
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
            Repr::Local(_) => out.put_padded(self),
            Repr::Shared(data) => out.put_padded_shared(data),
        }
        Ok(())
    }
}

impl Unpack for Opaque<'static> {
    fn unpack(input: &mut Decoder) -> Result<Self> {
        let len = u32::unpack(input)? as usize;
        Self::unpack_payload(input, len)
    }
}

impl Deref for Opaque<'_> {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match &self.0 {
            Repr::Local(Local::Borrowed(data)) => data,
            Repr::Local(Local::Inline { data, len }) => &data[..usize::from(*len)],
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
/// Uses the same wire format as [`Opaque`]. Decoding checks the length before
/// reading the payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundedOpaque<'a, const N: usize>(Opaque<'a>);

impl<const N: usize> BoundedOpaque<'_, N> {
    pub(crate) const fn empty() -> Self {
        Self(Opaque::borrowed(&[]))
    }

    pub(crate) fn borrow(&self) -> BoundedOpaque<'_, N> {
        BoundedOpaque(Opaque::borrowed(self.as_ref()))
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
        BoundedOpaque(Opaque::borrowed(data))
    }

    /// Borrow a slice as a `BoundedOpaque`.
    pub fn from_slice(data: &[u8]) -> Result<BoundedOpaque<'_, N>> {
        Opaque::borrowed(data).try_into()
    }

    /// Converts into [`Bytes`], copying if the data is borrowed or inline.
    #[must_use]
    pub fn into_bytes(self) -> Bytes {
        self.0.into_bytes()
    }

    /// Converts into an owned `BoundedOpaque`, storing short borrowed data inline.
    #[must_use]
    pub fn into_owned(self) -> BoundedOpaque<'static, N> {
        BoundedOpaque(self.0.into_owned())
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
        Opaque::unpack_payload(input, len).map(Self)
    }
}
