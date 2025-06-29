#[inline]
pub const fn add_padding(sz: usize) -> usize {
    (sz + 3) & !3
}

#[inline]
#[allow(clippy::cast_sign_loss, clippy::cast_possible_wrap)]
pub const fn get_padding(len: usize) -> usize {
    (-(len as isize) & 3) as usize
}

#[inline]
pub fn zero_padding(len: usize) -> &'static [u8] {
    const ZEROES: [u8; 3] = [0, 0, 0];
    let pad = get_padding(len);
    &ZEROES[..pad]
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_add_padding() {
        assert_eq!(add_padding(0), 0);
        assert_eq!(add_padding(1), 4);
        assert_eq!(add_padding(2), 4);
        assert_eq!(add_padding(3), 4);
        assert_eq!(add_padding(4), 4);
        assert_eq!(add_padding(5), 8);
        assert_eq!(add_padding(6), 8);
        assert_eq!(add_padding(7), 8);
        assert_eq!(add_padding(8), 8);
    }

    #[test]
    fn test_zero_padding() {
        assert_eq!(zero_padding(0), &[]);
        assert_eq!(zero_padding(1), &[0, 0, 0]);
        assert_eq!(zero_padding(2), &[0, 0]);
        assert_eq!(zero_padding(3), &[0]);
        assert_eq!(zero_padding(4), &[]);
        assert_eq!(zero_padding(5), &[0, 0, 0]);
        assert_eq!(zero_padding(6), &[0, 0]);
        assert_eq!(zero_padding(7), &[0]);
    }
}
