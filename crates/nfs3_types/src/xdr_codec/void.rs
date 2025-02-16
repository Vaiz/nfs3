use super::{Pack, PackedSize, Unpack};

pub struct Void;

impl<Out: std::io::Write> Pack<Out> for Void {
    fn pack(&self, _buf: &mut Out) -> nfs3_types::xdr_codec::Result<usize> {
        Ok(0)
    }
}

impl PackedSize for Void {
    const PACKED_SIZE: Option<usize> = Some(0);

    fn count_packed_size(&self) -> usize {
        0
    }
}

impl<In: std::io::Read> Unpack<In> for Void {
    fn unpack(_buf: &mut In) -> nfs3_types::xdr_codec::Result<(Void, usize)> {
        Ok((Void, 0))
    }
}
