//! XDR encoding and decoding.
//!
//! Values are decoded from a complete in-memory buffer ([`Decoder`]) and encoded
//! into a head plus at most one shared payload ([`Encoder`]), so bulk data can
//! be shared with the network buffers instead of being copied.

pub(crate) mod decoder;
pub(crate) mod encoder;
pub(crate) mod error;
pub(crate) mod list;
pub(crate) mod opaque;
pub(crate) mod primitives;
pub(crate) mod traits;
pub(crate) mod util;
pub(crate) mod void;

pub use nfs3_macros::XdrCodec;

pub use self::decoder::{Decoder, decode};
pub use self::encoder::{Encoded, Encoder, encode};
pub use self::error::Error;
pub use self::list::{BoundedList, List};
pub use self::opaque::Opaque;
pub use self::traits::{Pack, Unpack};
pub use self::void::Void;

pub type Result<T> = std::result::Result<T, Error>;
