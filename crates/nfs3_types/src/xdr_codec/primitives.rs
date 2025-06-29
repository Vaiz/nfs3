use std::io::{Read, Write};

use crate::xdr_codec::util::{add_padding, get_padding, zero_padding};
use crate::xdr_codec::{Error, Pack, Result, Unpack};

impl Pack for Vec<u32> {
    fn packed_size(&self) -> usize {
        4 + self.len() * 4
    }

    fn pack(&self, out: &mut impl Write) -> Result<usize> {
        let mut bytes_written = 0;

        bytes_written += u32::try_from(self.len())
            .map_err(|_| Error::ObjectTooLarge(self.len()))?
            .pack(out)?;

        for item in self {
            bytes_written += item.pack(out)?;
        }

        Ok(bytes_written)
    }
}

impl Unpack for Vec<u32> {
    fn unpack(input: &mut impl Read) -> Result<(Self, usize)> {
        let mut bytes_read = 0;

        let (len, len_bytes) = u32::unpack(input)?;
        bytes_read += len_bytes;

        let mut vec = Self::with_capacity(len as usize);

        for _ in 0..len {
            let (item, item_bytes) = u32::unpack(input)?;
            bytes_read += item_bytes;
            vec.push(item);
        }

        Ok((vec, bytes_read))
    }
}

impl Pack for u32 {
    fn packed_size(&self) -> usize {
        4
    }

    fn pack(&self, out: &mut impl Write) -> Result<usize> {
        let bytes = self.to_be_bytes();
        out.write_all(&bytes).map_err(Error::Io)?;
        Ok(4)
    }
}

impl Unpack for u32 {
    fn unpack(input: &mut impl Read) -> Result<(Self, usize)> {
        let mut bytes = [0u8; 4];
        input.read_exact(&mut bytes).map_err(Error::Io)?;
        Ok((Self::from_be_bytes(bytes), 4))
    }
}

impl Pack for u64 {
    fn packed_size(&self) -> usize {
        8
    }

    fn pack(&self, out: &mut impl Write) -> Result<usize> {
        let bytes = self.to_be_bytes();
        out.write_all(&bytes).map_err(Error::Io)?;
        Ok(8)
    }
}

impl Unpack for u64 {
    fn unpack(input: &mut impl Read) -> Result<(Self, usize)> {
        let mut bytes = [0u8; 8];
        input.read_exact(&mut bytes).map_err(Error::Io)?;
        Ok((Self::from_be_bytes(bytes), 8))
    }
}

impl Pack for bool {
    fn packed_size(&self) -> usize {
        4
    }

    #[expect(clippy::bool_to_int_with_if, reason = "we want to be explicit")]
    fn pack(&self, out: &mut impl Write) -> Result<usize> {
        let val = if *self { 1u32 } else { 0u32 };
        val.pack(out)
    }
}

impl Unpack for bool {
    fn unpack(input: &mut impl Read) -> Result<(Self, usize)> {
        let (val, bytes_read) = u32::unpack(input)?;
        match val {
            0 => Ok((false, bytes_read)),
            1 => Ok((true, bytes_read)),
            _ => Err(Error::InvalidEnumValue(val)),
        }
    }
}

impl<const N: usize> Pack for [u8; N] {
    fn packed_size(&self) -> usize {
        add_padding(N)
    }

    fn pack(&self, out: &mut impl Write) -> Result<usize> {
        let mut bytes_written = 0;
        out.write_all(self).map_err(Error::Io)?;
        bytes_written += N;

        let padding = zero_padding(N);
        out.write_all(padding).map_err(Error::Io)?;
        bytes_written += padding.len();

        Ok(bytes_written)
    }
}

impl<const N: usize> Unpack for [u8; N] {
    fn unpack(input: &mut impl Read) -> Result<(Self, usize)> {
        let mut bytes_read = 0;
        let mut buf = [0u8; N];
        input.read_exact(&mut buf).map_err(Error::Io)?;
        bytes_read += N;

        let padding = get_padding(N);
        if padding > 0 {
            let mut pad_buf = [0u8; 4];
            input
                .read_exact(&mut pad_buf[..padding])
                .map_err(Error::Io)?;
            bytes_read += padding;
        }

        Ok((buf, bytes_read))
    }
}
