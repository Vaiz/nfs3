use std::io::{Read, Write};

use super::error::Error;
use crate::xdr_codec::util::{add_padding, zero_padding};

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

impl<const N: usize> Pack for [u8; N]
{
    fn packed_size(&self) -> usize {
        add_padding(N)
    }

    fn pack(&self, out: &mut impl Write) -> Result<usize, Error> {
        let mut bytes_written = 0;
        out.write_all(self).map_err(Error::Io)?;
        bytes_written += N;

        let padding = zero_padding(N);
        out.write_all(&padding).map_err(Error::Io)?;
        bytes_written += padding.len();
        
        Ok(bytes_written)
    }
}

impl<const N: usize> Unpack for [u8; N] {
    fn unpack(input: &mut impl Read) -> Result<(Self, usize), Error> {
        let mut bytes_read = 0;
        let mut buf = [0u8; N];
        input.read_exact(&mut buf).map_err(Error::Io)?;
        bytes_read += N;

        let padding = add_padding(N);
        if padding > 0 {
            let mut pad_buf = [0u8; 4];
            input.read_exact(&mut pad_buf[..padding]).map_err(Error::Io)?;
            bytes_read += padding;
        }

        Ok((buf, bytes_read))
    }
}