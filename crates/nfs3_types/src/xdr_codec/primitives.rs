use crate::xdr_codec::{Decoder, Encoder, Error, Pack, Result, Unpack};

impl Pack for u32 {
    fn packed_size(&self) -> usize {
        4
    }

    fn pack(&self, out: &mut Encoder) -> Result<()> {
        out.put_array(&self.to_be_bytes());
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
        out.put_array(&self.to_be_bytes());
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
