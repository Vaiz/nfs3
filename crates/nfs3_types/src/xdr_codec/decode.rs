use std::borrow::Cow;
use std::fmt;

use super::{List, Opaque, Payload, PayloadError, PayloadLimits, RecordCursor, Void};

/// Caller-selected policy. No defaults or transport policy are implied.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct DecodeLimits {
    /// Aggregate bytes copied into independent opaque metadata.
    pub max_metadata_bytes: usize,
    /// Aggregate number of decoded collection elements.
    pub max_collection_elements: usize,
    /// Maximum simultaneously active `Decoder::decode` calls.
    pub max_depth: usize,
    /// Monotonic requested reservation work, not live/peak or allocator usable bytes.
    pub max_reservation_bytes: usize,
    /// Per-field shared bulk bounds.
    pub payload: PayloadLimits,
}

/// Aggregate decoder resource being charged.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DecodeLimit {
    MetadataBytes,
    CollectionElements,
    Depth,
    ReservationBytes,
    FieldElements,
}

/// Value-only first failure; never retains packet owners.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DecodeError {
    Wire(PayloadError),
    Limit {
        kind: DecodeLimit,
        limit: usize,
        actual: usize,
    },
    InvalidTag(u32),
    Overflow,
    Allocation,
}

impl From<PayloadError> for DecodeError {
    fn from(error: PayloadError) -> Self {
        Self::Wire(error)
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for DecodeError {}

pub type DecodeResult<T> = Result<T, DecodeError>;

/// Owner-aware XDR decoding. Implementations must nest through `Decoder::decode`.
///
/// Independent user allocations or direct calls to `T::decode` bypass resource
/// guarantees. Metadata always owns independent bytes, for every output lifetime.
pub trait Decode: Sized {
    fn decode(decoder: &mut Decoder) -> DecodeResult<Self>;
}

/// Owns a complete record and one fail-closed aggregate budget.
///
/// Every operation checks the first error, including errors swallowed by custom
/// code. Failure retains the record until drop/finish at latest; finish consumes
/// it on either outcome. There is no raw cursor escape.
pub struct Decoder {
    cursor: RecordCursor,
    limits: DecodeLimits,
    metadata: usize,
    elements: usize,
    reservation: usize,
    depth: usize,
    error: Option<DecodeError>,
}

fn charge(counter: &mut usize, amount: usize, limit: usize, kind: DecodeLimit) -> DecodeResult<()> {
    let actual = counter.checked_add(amount).ok_or(DecodeError::Overflow)?;
    if actual > limit {
        return Err(DecodeError::Limit {
            kind,
            limit,
            actual,
        });
    }
    *counter = actual;
    Ok(())
}

impl Decoder {
    #[must_use]
    pub fn new(record: Payload, limits: DecodeLimits) -> Self {
        Self {
            cursor: RecordCursor::new(record),
            limits,
            metadata: 0,
            elements: 0,
            reservation: 0,
            depth: 0,
            error: None,
        }
    }

    fn ready(&self) -> DecodeResult<()> {
        self.error.map_or(Ok(()), Err)
    }

    fn remember<T>(&mut self, result: DecodeResult<T>) -> DecodeResult<T> {
        if self.error.is_none() {
            if let Err(error) = &result {
                self.error = Some(*error);
            }
        }
        self.ready()?;
        result
    }

    /// Record a validation error, preserving any earlier failure.
    pub fn fail<T>(&mut self, error: DecodeError) -> DecodeResult<T> {
        self.remember(Err(error))
    }

    /// Decode one value, charging active depth and preserving nested failures.
    pub fn decode<T: Decode>(&mut self) -> DecodeResult<T> {
        self.ready()?;
        let result = (|| {
            charge(
                &mut self.depth,
                1,
                self.limits.max_depth,
                DecodeLimit::Depth,
            )?;
            let result = T::decode(self);
            self.depth -= 1;
            result
        })();
        self.remember(result)
    }

    /// Consumed wire bytes; unavailable after failure.
    pub fn position(&self) -> DecodeResult<usize> {
        self.ready()?;
        Ok(self.cursor.position())
    }

    /// Remaining wire bytes; unavailable after failure.
    pub fn remaining(&self) -> DecodeResult<usize> {
        self.ready()?;
        Ok(self.cursor.remaining())
    }

    /// Require exact consumption and release the record on either outcome.
    pub fn finish(self) -> DecodeResult<()> {
        self.ready()?;
        self.cursor.finish().map_err(Into::into)
    }

    /// Decode independent opaque metadata with an additional protocol field bound.
    pub fn opaque(&mut self, max: usize) -> DecodeResult<Opaque<'static>> {
        self.ready()?;
        let result = (|| {
            let len = self.cursor.opaque_len(max)?;
            charge(
                &mut self.metadata,
                len,
                self.limits.max_metadata_bytes,
                DecodeLimit::MetadataBytes,
            )?;
            charge(
                &mut self.reservation,
                len,
                self.limits.max_reservation_bytes,
                DecodeLimit::ReservationBytes,
            )?;
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(len)
                .map_err(|_| DecodeError::Allocation)?;
            bytes.resize(len, 0);
            self.cursor.read_opaque_into(&mut bytes, max)?;
            Ok(Opaque(Cow::Owned(bytes)))
        })();
        self.remember(result)
    }

    /// Decode a counted collection without reserving from its advertised length.
    pub fn collection<T: Decode>(&mut self, max: usize) -> DecodeResult<Vec<T>> {
        self.ready()?;
        let result = (|| {
            let count =
                usize::try_from(self.decode::<u32>()?).map_err(|_| DecodeError::Overflow)?;
            if count > max {
                return Err(DecodeError::Limit {
                    kind: DecodeLimit::FieldElements,
                    limit: max,
                    actual: count,
                });
            }
            let mut items = Vec::new();
            let mut target = 0;
            for _ in 0..count {
                charge(
                    &mut self.elements,
                    1,
                    self.limits.max_collection_elements,
                    DecodeLimit::CollectionElements,
                )?;
                let item = self.decode()?;
                self.push(&mut items, &mut target, item)?;
            }
            Ok(items)
        })();
        self.remember(result)
    }

    fn push<T>(&mut self, items: &mut Vec<T>, target: &mut usize, item: T) -> DecodeResult<()> {
        if items.len() == *target {
            let next = if *target == 0 {
                1
            } else {
                target.checked_mul(2).ok_or(DecodeError::Overflow)?
            };
            let bytes = next
                .checked_mul(size_of::<T>())
                .ok_or(DecodeError::Overflow)?;
            charge(
                &mut self.reservation,
                bytes,
                self.limits.max_reservation_bytes,
                DecodeLimit::ReservationBytes,
            )?;
            items
                .try_reserve_exact(next - items.len())
                .map_err(|_| DecodeError::Allocation)?;
            *target = next;
        }
        items.push(item);
        Ok(())
    }
}

macro_rules! scalar {
    ($ty:ty, $method:ident) => {
        impl Decode for $ty {
            fn decode(d: &mut Decoder) -> DecodeResult<Self> {
                d.ready()?;
                let result = d.cursor.$method().map_err(Into::into);
                d.remember(result)
            }
        }
    };
}
scalar!(u32, read_u32);
scalar!(u64, read_u64);
scalar!(bool, read_bool);

impl<const N: usize> Decode for [u8; N] {
    fn decode(d: &mut Decoder) -> DecodeResult<Self> {
        d.ready()?;
        let mut bytes = [0; N];
        let result = d
            .cursor
            .read_fixed(&mut bytes)
            .map(|()| bytes)
            .map_err(Into::into);
        d.remember(result)
    }
}

impl Decode for Opaque<'_> {
    fn decode(d: &mut Decoder) -> DecodeResult<Self> {
        d.opaque(usize::MAX)
    }
}
impl Decode for Payload {
    fn decode(d: &mut Decoder) -> DecodeResult<Self> {
        d.ready()?;
        let result = d.cursor.read_opaque_charged(d.limits.payload, |bytes| {
            charge(
                &mut d.reservation,
                bytes,
                d.limits.max_reservation_bytes,
                DecodeLimit::ReservationBytes,
            )
        });
        d.remember(result)
    }
}
impl<T: Decode> Decode for Vec<T> {
    fn decode(d: &mut Decoder) -> DecodeResult<Self> {
        d.collection(usize::MAX)
    }
}
impl<T: Decode> Decode for List<T> {
    fn decode(d: &mut Decoder) -> DecodeResult<Self> {
        d.ready()?;
        let result = (|| {
            let mut items = Vec::new();
            let mut target = 0;
            while d.decode::<bool>()? {
                charge(
                    &mut d.elements,
                    1,
                    d.limits.max_collection_elements,
                    DecodeLimit::CollectionElements,
                )?;
                let item = d.decode()?;
                d.push(&mut items, &mut target, item)?;
            }
            Ok(Self(items))
        })();
        d.remember(result)
    }
}
impl Decode for Void {
    fn decode(d: &mut Decoder) -> DecodeResult<Self> {
        d.ready()?;
        Ok(Self)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn limits() -> DecodeLimits {
        DecodeLimits {
            max_metadata_bytes: 32,
            max_collection_elements: 8,
            max_depth: 8,
            max_reservation_bytes: 1024,
            payload: PayloadLimits {
                max_logical_bytes: 32,
                max_backing_capacity: 32,
                max_segments: 8,
            },
        }
    }

    #[test]
    fn exact_descriptors_charged_before_cursor_progress() {
        let chunks = vec![vec![0, 0, 0, 3], vec![1], vec![2], vec![3], vec![0]];
        let mut observed = 0;
        // Obtain the private descriptor size through the same P1 preflight hook.
        let mut cursor =
            RecordCursor::new(Payload::try_from_chunks(chunks.clone(), limits().payload).unwrap());
        cursor
            .read_opaque_charged::<PayloadError>(limits().payload, |bytes| {
                observed = bytes;
                Ok(())
            })
            .unwrap();
        assert!(observed > 0);
        for budget in [observed - 1, observed] {
            let mut policy = limits();
            policy.max_reservation_bytes = budget;
            let record = Payload::try_from_chunks(chunks.clone(), policy.payload).unwrap();
            let mut d = Decoder::new(record, policy);
            let result = d.decode::<Payload>();
            if budget < observed {
                assert!(result.is_err());
                assert_eq!(d.cursor.position(), 0);
                assert_eq!(d.reservation, 0);
            } else {
                assert_eq!(result.unwrap().segment_count(), 3);
                assert_eq!(d.cursor.position(), 8);
                assert_eq!(d.reservation, observed);
            }
        }
    }

    #[test]
    fn metadata_preflight_and_monotonic_work() {
        for bytes in [vec![0, 0, 0, 99], vec![0, 0, 0, 1, 1, 0, 0, 2]] {
            let record = Payload::try_from_vec(bytes, limits().payload).unwrap();
            let mut d = Decoder::new(record, limits());
            assert!(d.decode::<Opaque<'_>>().is_err());
            assert_eq!(d.cursor.position(), 0);
            assert_eq!(d.reservation, 0);
            assert_eq!(d.metadata, 0);
        }
        let bytes = [0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0];
        let mut d = Decoder::new(
            Payload::try_from_vec(bytes.to_vec(), limits().payload).unwrap(),
            limits(),
        );
        drop(d.decode::<Opaque<'_>>().unwrap());
        assert_eq!(d.reservation, 1);
        d.decode::<Payload>().unwrap(); // empty descriptors cost zero
        assert_eq!(d.reservation, 1);
        d.finish().unwrap();
        let mut counter = usize::MAX;
        assert_eq!(
            charge(&mut counter, 1, usize::MAX, DecodeLimit::ReservationBytes),
            Err(DecodeError::Overflow)
        );
    }
}
