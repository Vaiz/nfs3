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
        let mut items = Self::with_capacity(len);
        for _ in 0..len {
            items.push(u32::unpack(input)?);
        }
        Ok(items)
    }
}

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
