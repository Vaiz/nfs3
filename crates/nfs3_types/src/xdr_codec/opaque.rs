use std::borrow::Cow;
use std::io::{Read, Write};
use std::ops::Deref;

use crate::xdr_codec::util::{add_padding, get_padding, zero_padding};
use crate::xdr_codec::{Error, Pack, Unpack};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opaque<'a>(pub Cow<'a, [u8]>);

impl Opaque<'static> {
    /// Creates a new `Opaque` with owned data.
    #[must_use]
    pub const fn owned(data: Vec<u8>) -> Self {
        Opaque(Cow::Owned(data))
    }
}

impl<'a> Opaque<'a> {
    /// Creates a new `Opaque`.
    #[must_use]
    pub const fn new(data: Cow<'a, [u8]>) -> Self {
        Opaque(data)
    }

    /// Creates a new `Opaque` from a borrowed slice.
    #[must_use]
    pub const fn borrowed(data: &'a [u8]) -> Self {
        Opaque(Cow::Borrowed(data))
    }

    /// Creates a new `Opaque` from a `Vec<u8>`.
    #[must_use]
    pub const fn from_vec(data: Vec<u8>) -> Self {
        Opaque(Cow::Owned(data))
    }

    /// Returns the length of the opaque data.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns true if the opaque data is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Extracts the owned data.
    ///
    /// Clones the data if it is not already owned.
    #[must_use]
    pub fn into_owned(self) -> Vec<u8> {
        self.0.into_owned()
    }

    #[must_use]
    pub fn to_vec(&self) -> Vec<u8> {
        self.0.to_vec()
    }
}

impl Pack for Opaque<'_> {
    fn packed_size(&self) -> usize {
        4 + add_padding(self.0.len())
    }

    fn pack(&self, out: &mut impl Write) -> Result<usize, Error> {
        let mut bytes_written = 0;
        let len: u32 = self
            .0
            .len()
            .try_into()
            .map_err(|_| Error::ObjectTooLarge(self.0.len()))?;
        bytes_written += len.pack(out)?;

        out.write_all(&self.0).map_err(Error::Io)?;
        bytes_written += self.0.len();

        let padding = zero_padding(self.0.len());
        out.write_all(padding).map_err(Error::Io)?;
        bytes_written += padding.len();
        Ok(bytes_written)
    }
}

impl Unpack for Opaque<'static> {
    fn unpack(input: &mut impl Read) -> Result<(Self, usize), Error> {
        let (len, mut bytes_read) = u32::unpack(input)?;
        let len = len as usize;

        let mut buf = vec![0u8; len];
        input.read_exact(&mut buf).map_err(Error::Io)?;
        bytes_read += len;

        let len = get_padding(len);
        if len > 0 {
            let mut pad_buf = [0u8; 4];
            input.read_exact(&mut pad_buf[..len]).map_err(Error::Io)?;
            bytes_read += len;
        }

        Ok((Opaque(Cow::Owned(buf)), bytes_read))
    }
}

impl AsRef<[u8]> for Opaque<'_> {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl Deref for Opaque<'_> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<Vec<u8>> for Opaque<'static> {
    fn from(vec: Vec<u8>) -> Self {
        Opaque(Cow::Owned(vec))
    }
}

impl<'a> From<&'a [u8]> for Opaque<'a> {
    fn from(slice: &'a [u8]) -> Self {
        Opaque(Cow::Borrowed(slice))
    }
}
