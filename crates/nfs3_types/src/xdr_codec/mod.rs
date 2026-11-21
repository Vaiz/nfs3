//! Checked owner-aware [`Decode`]/[`Encode`] codecs.
//!
//! [`Decoder`] consumes complete segmented records under explicit [`DecodeLimits`].
//! Metadata copies into independent storage; [`Payload`] fields share bulk bytes.
//! [`Encoder`] builds an owned segmented [`SendPlan`] under explicit [`EncodeLimits`].
//! No transport switch or implicit bulk coalescing is provided.

mod budget;
mod cursor;
mod decode;
mod encode;
pub(crate) mod list;
pub(crate) mod opaque;
mod payload;
mod payload_error;
pub(crate) mod void;

pub use nfs3_macros::{XdrCodec, XdrDecode, XdrEncode};

pub use self::budget::{BackingBudget, BackingReservation};
pub(crate) use self::cursor::RecordCursor;
pub use self::decode::{Decode, DecodeError, DecodeLimit, DecodeLimits, DecodeResult, Decoder};
pub use self::encode::{
    Encode, EncodeError, EncodeLimit, EncodeLimits, EncodeResult, Encoder, SendPlan,
};
pub use self::list::List;
pub use self::opaque::Opaque;
pub(crate) use self::payload::RecordBuilder;
pub use self::payload::{Payload, PayloadLimits, ReceiveBuffer};
pub use self::payload_error::{PayloadError, PayloadLimit};
pub use self::void::Void;

#[cfg(test)]
mod backing_budget_tests;
