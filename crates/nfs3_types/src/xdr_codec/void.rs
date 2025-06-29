use super::{Error, Pack, Unpack};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Void;

impl Pack for Void {
    fn packed_size(&self) -> usize {
        0
    }

    fn pack(&self, _out: &mut impl std::io::Write) -> Result<usize, Error> {
        Ok(0)
    }
}

impl Unpack for Void {
    fn unpack(_input: &mut impl std::io::Read) -> Result<(Self, usize), Error> {
        Ok((Self, 0))
    }
}
