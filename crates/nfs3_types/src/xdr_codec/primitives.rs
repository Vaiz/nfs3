use crate::xdr_codec::util::add_padding;
use crate::xdr_codec::{Decoder, Encoder, Error, Pack, Result, Unpack};

impl Pack for Vec<u32> {
    fn packed_size(&self) -> usize {
        4 + self.len() * 4
    }

    fn pack(&self, out: &mut Encoder) -> Result<()> {
        u32::try_from(self.len())
            .map_err(|_overflow| Error::ObjectTooLarge(self.len()))?
            .pack(out)?;
        for item in self {
            item.pack(out)?;
        }
        Ok(())
    }
}

impl Unpack for Vec<u32> {
    fn unpack(input: &mut Decoder) -> Result<Self> {
        let len = u32::unpack(input)? as usize;
        if len > input.remaining() / 4 {
            return Err(Error::InvalidLength(len));
        }
        (0..len).map(|_| u32::unpack(input)).collect()
    }
}

impl Pack for u32 {
    fn packed_size(&self) -> usize {
        4
    }

    fn pack(&self, out: &mut Encoder) -> Result<()> {
        out.put_u32(*self);
        Ok(())
    }
}

impl Unpack for u32 {
    fn unpack(input: &mut Decoder) -> Result<Self> {
        input.read_array().map(Self::from_be_bytes)
    }
}

impl Pack for u64 {
    fn packed_size(&self) -> usize {
        8
    }

    fn pack(&self, out: &mut Encoder) -> Result<()> {
        out.put_u64(*self);
        Ok(())
    }
}

impl Unpack for u64 {
    fn unpack(input: &mut Decoder) -> Result<Self> {
        input.read_array().map(Self::from_be_bytes)
    }
}

impl Pack for bool {
    fn packed_size(&self) -> usize {
        4
    }

    fn pack(&self, out: &mut Encoder) -> Result<()> {
        u32::from(*self).pack(out)
    }
}

impl Unpack for bool {
    fn unpack(input: &mut Decoder) -> Result<Self> {
        match u32::unpack(input)? {
            0 => Ok(false),
            1 => Ok(true),
            val => Err(Error::InvalidEnumValue(val)),
        }
    }
}

impl<const N: usize> Pack for [u8; N] {
    fn packed_size(&self) -> usize {
        add_padding(N)
    }

    fn pack(&self, out: &mut Encoder) -> Result<()> {
        out.put_padded(self);
        Ok(())
    }
}

impl<const N: usize> Unpack for [u8; N] {
    fn unpack(input: &mut Decoder) -> Result<Self> {
        let buf = input.read_array()?;
        input.skip_padding(N)?;
        Ok(buf)
    }
}
