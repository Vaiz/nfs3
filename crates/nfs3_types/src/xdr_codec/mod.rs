//! Legacy streaming [`Pack`]/[`Unpack`] and additive checked [`Decode`]/[`Encode`] codecs.
//!
//! [`Decoder`] consumes complete segmented records under explicit [`DecodeLimits`].
//! Metadata copies into independent storage; [`Payload`] fields share bulk bytes.
//! [`Encoder`] builds an owned segmented [`SendPlan`] under explicit [`EncodeLimits`].
//! No transport switch or implicit bulk coalescing is provided.

mod budget;
mod cursor;
mod decode;
mod encode;
pub(crate) mod error;
pub(crate) mod list;
pub(crate) mod opaque;
mod payload;
mod payload_error;
pub(crate) mod primitives;
pub(crate) mod traits;
pub(crate) mod util;
pub(crate) mod void;

pub use nfs3_macros::{XdrCodec, XdrDecode, XdrEncode};

pub use self::budget::{BackingBudget, BackingReservation};
pub use self::cursor::RecordCursor;
pub use self::decode::{Decode, DecodeError, DecodeLimit, DecodeLimits, DecodeResult, Decoder};
pub use self::encode::{
    Encode, EncodeError, EncodeLimit, EncodeLimits, EncodeResult, Encoder, SendChunks, SendPlan,
};
pub use self::error::Error;
pub use self::list::{BoundedList, List};
pub use self::opaque::Opaque;
pub use self::payload::{Payload, PayloadLimits, ReceiveBuffer, RecordBuilder};
pub use self::payload_error::{PayloadError, PayloadLimit};
pub use self::traits::{Pack, Unpack};
pub use self::void::Void;

pub type Result<T> = std::result::Result<T, Error>;
