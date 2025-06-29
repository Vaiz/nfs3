use std::borrow::Cow;
use std::io::{Read, Write};

use super::error::Error;
use crate::xdr_codec::util::{add_padding, zero_padding};

const PADDING: [u8; 4] = [0, 0, 0, 0];

pub trait Pack {
    /// Returns the packed size of the type.
    fn packed_size(&self) -> usize;

    /// Packs the type into a byte slice.
    fn pack(&self, out: &mut impl Write) -> Result<usize, Error>;
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
