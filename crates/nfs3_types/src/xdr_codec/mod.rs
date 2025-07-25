//! This module is mainly a reexport of the `xdr_codec` crate, with some additional types and
//! traits.
//!
//! > NOTE: `xdr_codec` crate has been updated in a long time, so it might be replaced in the
//! > future.

pub(crate) mod error;
pub(crate) mod list;
pub(crate) mod opaque;
pub(crate) mod primitives;
pub(crate) mod traits;
pub(crate) mod util;
pub(crate) mod void;

pub use nfs3_macros::XdrCodec;

pub use self::error::Error;
pub use self::list::{BoundedList, List};
pub use self::opaque::Opaque;
pub use self::traits::{Pack, Unpack};
pub use self::void::Void;

pub type Result<T> = std::result::Result<T, Error>;
