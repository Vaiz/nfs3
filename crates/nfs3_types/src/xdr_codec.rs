pub use ::xdr_codec::*;
pub use nfs3_macros::XdrCodec;

#[derive(Debug)]
pub struct List<T>(pub Vec<T>);

impl<T, Out> Pack<Out> for List<T>
where
    Out: Write,
    T: Pack<Out>,
{
    fn pack(&self, output: &mut Out) -> Result<usize> {
        let mut len = 0;
        for item in &self.0 {
            len += true.pack(output)?;
            len += item.pack(output)?;
        }
        len += false.pack(output)?;
        Ok(len)
    }
}

impl<T, In> Unpack<In> for List<T>
where
    In: Read,
    T: Unpack<In>,
{
    fn unpack(input: &mut In) -> Result<(Self, usize)> {
        let mut items = Vec::new();
        let mut len = 0;
        loop {
            let (more, more_len) = bool::unpack(input)?;
            len += more_len;
            if !more {
                break;
            }
            let (item, item_len) = T::unpack(input)?;
            len += item_len;
            items.push(item);
        }
        Ok((List(items), len))
    }
}