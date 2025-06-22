pub enum Error {
    /// An error occurred while reading or writing data.
    Io(std::io::Error),

    /// An invalid value was encountered for an enum/bool type.
    InvalidEnumValue(u32),

    /// An error occurred while packing an object.
    ObjectTooLarge(usize),
}