use std::io::{Read, Write};
use crate::xdr_codec::util::{add_padding, zero_padding};

use super::error::Error;

const PADDING: [u8; 4] = [0, 0, 0, 0];


pub trait Pack {
    /// Returns the packed size of the type.
    fn packed_size(&self) -> usize;

    /// Packs the type into a byte slice.
    fn pack(&self, out: &mut impl Write) -> Result<usize    , Error>;
}

pub trait Unpack: Sized {
    /// Unpacks the type from a byte slice.
    fn unpack(input: &mut impl Read) -> Result<(Self, usize), Error>;
}

impl Pack for bool {
    fn packed_size(&self) -> usize {
        4
    }

    fn pack(&self, out: &mut impl Write) -> Result<usize, Error> {
        let value = if *self { 1u32 } else { 0u32 };
        value.pack(out)
    }
}

impl Unpack for bool {
    fn unpack(input: &mut impl Read) -> Result<(Self, usize), Error> {
        let (value, bytes_read) = u32::unpack(input)?;
        match value {
            0 => Ok((false, bytes_read)),
            1 => Ok((true, bytes_read)),
            _ => Err(Error::InvalidEnumValue(value)),
        }
    }
}

impl Pack for u32 {
    fn packed_size(&self) -> usize {
        4
    }

    fn pack(&self, out: &mut impl Write) -> Result<usize, Error> {
        out.write_all(&self.to_le_bytes()).map_err(Error::Io)?;
        Ok(4)
    }
}

impl Unpack for u32 {
    fn unpack(input: &mut impl Read) -> Result<(Self, usize), Error> {
        let mut buf = [0u8; 4];
        input.read_exact(&mut buf).map_err(Error::Io)?;
        Ok((u32::from_le_bytes(buf), 4))
    }
}

impl Pack for u64 {
    fn packed_size(&self) -> usize {
        8
    }

    fn pack(&self, out: &mut impl Write) -> Result<usize, Error> {
        out.write_all(&self.to_le_bytes()).map_err(Error::Io)?;
        Ok(8)
    }
}

impl Unpack for u64 {
    fn unpack(input: &mut impl Read) -> Result<(Self, usize), Error> {
        let mut buf = [0u8; 8];
        input.read_exact(&mut buf).map_err(Error::Io)?;
        Ok((u64::from_le_bytes(buf), 8))
    }
}

impl Pack for [u8] {
    fn packed_size(&self) -> usize {
        4 + add_padding(self.len())
    }
    fn pack(&self, out: &mut impl Write) -> Result<usize, Error> {
        let mut bytes_written = 0;
        let len: u32 = self.len().try_into().map_err(|_| Error::ObjectTooLarge(self.len()))?;
        bytes_written += len.pack(out)?;

        out.write_all(self).map_err(Error::Io)?;
        bytes_written += self.len();

        let padding = zero_padding(self.len());
        out.write_all(&padding).map_err(Error::Io)?;
        bytes_written += padding.len();
        Ok(bytes_written)
    }
}

