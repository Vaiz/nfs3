//! This module is mainly a reexport of the `xdr_codec` crate, with some additional types and
//! traits.
//!
//! > NOTE: `xdr_codec` crate has been updated in a long time, so it might be replaced in the
//! > future.

pub(crate) mod list;
pub(crate) mod packed_size;
pub(crate) mod void;
pub(crate) mod traits;
pub(crate) mod error;
pub(crate) mod util;

pub use ::xdr_codec::*;
/// Derive macro that implements [`Pack`] and [`Unpack`] traits.
pub use nfs3_macros::XdrCodec;

pub use self::list::{BoundedList, List};
pub use self::packed_size::PackedSize;
pub use self::void::Void;
