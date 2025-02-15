//! This module is mainly a reexport of the `xdr_codec` crate, with some additional types and traits.
//!
//! > NOTE: `xdr_codec` crate has been updated in a long time, so it might be replaced in the future.

pub(crate) mod list;
pub(crate) mod packed_size;

/// Derive macro that implements [`Pack`] and [`Unpack`] traits.
pub use nfs3_macros::XdrCodec;
pub use ::xdr_codec::*;


pub use self::list::List;
pub use self::packed_size::PackedSize;
