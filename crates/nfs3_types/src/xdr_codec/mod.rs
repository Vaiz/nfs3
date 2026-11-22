//! XDR encoding and decoding.
//!
//! Values are decoded with [`Decoder`] and encoded with [`Encoder`]. Large
//! opaque data, such as READ and WRITE payloads, is passed as reference-counted
//! [`Bytes`](bytes::Bytes) sharing the network buffers instead of being copied.

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

pub use self::decoder::Decoder;
pub(crate) use self::encoder::encode;
pub use self::encoder::{Encoded, Encoder};
pub use self::error::Error;
pub use self::list::{BoundedList, List};
pub use self::opaque::Opaque;
pub use self::traits::{Pack, Unpack};
pub use self::void::Void;

pub type Result<T> = std::result::Result<T, Error>;
