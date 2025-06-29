#[derive(Debug)]
pub enum Error {
    /// An error occurred while reading or writing data.
    Io(std::io::Error),

    /// An invalid value was encountered for an enum/bool type.
    InvalidEnumValue(u32),

    /// An error occurred while unpacking an object.
    InvalidLength(usize),

    /// An error occurred while packing an object.
    ObjectTooLarge(usize),

    /// An error occurred while converting from UTF8
    Utf8(std::string::FromUtf8Error),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<std::string::FromUtf8Error> for Error {
    fn from(e: std::string::FromUtf8Error) -> Self {
        Self::Utf8(e)
    }
}
