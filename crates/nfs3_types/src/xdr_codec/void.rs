use super::{Decoder, Encoder, Pack, Result, Unpack};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Void;

impl Pack for Void {
    fn packed_size(&self) -> usize {
        0
    }

    fn pack(&self, _out: &mut Encoder) -> Result<()> {
        Ok(())
    }
}

impl Unpack for Void {
    fn unpack(_input: &mut Decoder) -> Result<Self> {
        Ok(Self)
    }
}
