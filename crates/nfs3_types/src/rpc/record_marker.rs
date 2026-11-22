//! The 4-byte marker in front of each fragment of an RPC record
//! (RFC 5531, section 11).

use nfs3_macros::XdrCodec;

use crate::xdr_codec;

/// Record marker: a 32-bit integer holding the fragment length and a flag
/// marking the last fragment of a record.
#[derive(Default, Copy, Clone, Debug, Eq, PartialEq, XdrCodec)]
pub struct fragment_header {
    pub header: u32,
}

impl fragment_header {
    pub const EOF_FLAG: u32 = 0x8000_0000;
    pub const MASK: u32 = 0x7FFF_FFFF;

    /// Creates the header of a `length`-byte fragment; `eof` marks the last
    /// fragment of a record.
    ///
    /// Fails if `length` exceeds [`MASK`](Self::MASK).
    pub fn new(length: usize, eof: bool) -> xdr_codec::Result<Self> {
        let mut header = u32::try_from(length)
            .ok()
            .filter(|length| *length <= Self::MASK)
            .ok_or(xdr_codec::Error::ObjectTooLarge(length))?;
        if eof {
            header |= Self::EOF_FLAG;
        }
        Ok(Self { header })
    }
    #[must_use]
    pub const fn eof(&self) -> bool {
        self.header & Self::EOF_FLAG != 0
    }
    #[must_use]
    pub const fn fragment_length(&self) -> u32 {
        self.header & Self::MASK
    }
    #[must_use]
    pub const fn into_xdr_buf(self) -> [u8; 4] {
        self.header.to_be_bytes()
    }
}

impl From<[u8; 4]> for fragment_header {
    fn from(bytes: [u8; 4]) -> Self {
        let header = u32::from_be_bytes(bytes);
        Self { header }
    }
}

#[cfg(test)]
mod tests {
    use super::fragment_header;
    use crate::xdr_codec::Error;

    #[test]
    fn fragment_length_is_limited_to_31_bits() {
        let header = fragment_header::new(0x7fff_ffff, true).unwrap();
        assert_eq!(header.into_xdr_buf(), [0xff; 4]);
        assert_eq!(
            fragment_header::new(0x8000_0000, true).unwrap_err(),
            Error::ObjectTooLarge(0x8000_0000)
        );
    }
}
