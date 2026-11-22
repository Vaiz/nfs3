mod basic_types;
mod collections;
mod nfs3_types;
mod rpc_mount;
mod xdr_utils;

use bytes::Bytes;

use crate::xdr_codec::{Encoder, Pack, Result};

/// Encodes `value` into one contiguous buffer.
fn encode<T: Pack + ?Sized>(value: &T) -> Result<Bytes> {
    let mut out = Encoder::new(value.packed_size());
    value.pack(&mut out)?;
    Ok(out.into_bytes())
}
