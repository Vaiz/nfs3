use std::ops::Mul;

const MEBIBYTE: u32 = 1024 * 1024;
const GIBIBYTE: u64 = 1024 * 1024 * 1024;

pub(crate) struct Mebibyte;

impl<T> Mul<T> for Mebibyte
where
    T: Mul + From<u32>,
{
    type Output = <T as Mul>::Output;
    fn mul(self, val: T) -> Self::Output {
        T::from(MEBIBYTE) * val
    }
}

impl Into<u32> for Mebibyte {
    fn into(self) -> u32 {
        MEBIBYTE
    }
}

impl Into<u64> for Mebibyte {
    fn into(self) -> u64 {
        MEBIBYTE as u64
    }
}

pub(crate) struct Gibibyte;

impl<T> Mul<T> for Gibibyte
where
    T: Mul + From<u64>,
{
    type Output = <T as Mul>::Output;
    fn mul(self, val: T) -> Self::Output {
        T::from(GIBIBYTE) * val
    }
}

impl Mul<Gibibyte> for u64 {
    type Output = u64;

    fn mul(self, _: Gibibyte) -> Self::Output {
        self * GIBIBYTE
    }
}

impl Into<u64> for Gibibyte {
    fn into(self) -> u64 {
        GIBIBYTE as u64
    }
}
