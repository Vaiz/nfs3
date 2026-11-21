use std::fmt;
use std::ops::Range;

use super::{List, Opaque, Payload, PayloadError, Void};

/// Caller-selected encoding bounds. There is deliberately no default policy.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct EncodeLimits {
    pub max_wire_bytes: usize,
    /// All appended metadata, including scalars, tags, lengths and zero padding.
    pub max_metadata_bytes: usize,
    /// Nonempty metadata parts plus nonempty bulk descriptors.
    pub max_chunks: usize,
    /// Sum of bulk backing Vec capacities per descriptor, including duplicates.
    pub max_retained_capacity: usize,
    /// Simultaneously active `Encoder::encode` calls.
    pub max_depth: usize,
    /// Monotonic full-target requested vector reservation work, not live bytes.
    pub max_reservation_bytes: usize,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum EncodeLimit {
    WireBytes,
    MetadataBytes,
    Chunks,
    RetainedCapacity,
    Depth,
    ReservationBytes,
    FieldBytes,
    FieldElements,
}

/// Value-only first failure; never owns input or partially encoded output.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum EncodeError {
    UnrepresentableLength(usize),
    UnrepresentableCount(usize),
    InvalidRepresentation,
    Limit {
        kind: EncodeLimit,
        limit: usize,
        actual: usize,
    },
    Overflow,
    Allocation,
}

impl From<PayloadError> for EncodeError {
    fn from(error: PayloadError) -> Self {
        match error {
            PayloadError::Overflow => Self::Overflow,
            PayloadError::Allocation => Self::Allocation,
            _ => Self::InvalidRepresentation,
        }
    }
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for EncodeError {}

pub type EncodeResult<T> = Result<T, EncodeError>;

/// Checked segmented XDR encoding.
///
/// Nest through `Encoder::encode`. Independent allocations and direct calls to
/// another implementation's `Encode::encode` bypass the resource guarantee.
pub trait Encode {
    fn encode(&self, encoder: &mut Encoder) -> EncodeResult<()>;
}

impl<T: Encode + ?Sized> Encode for &T {
    fn encode(&self, encoder: &mut Encoder) -> EncodeResult<()> {
        T::encode(self, encoder)
    }
}

enum Part {
    Metadata(Range<usize>),
    Bulk(Payload),
}

/// Owned wire output, excluding RPC record markers/fragmentation.
///
/// Bulk bytes are shared; metadata is copied. No borrowed input escapes.
/// Only successful `Encoder::finish` exposes output.
pub struct SendPlan {
    metadata: Vec<u8>,
    parts: Vec<Part>,
    len: usize,
    chunks: usize,
}

impl fmt::Debug for SendPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SendPlan")
            .field("len", &self.len)
            .field("metadata_bytes", &self.metadata.len())
            .field("metadata_capacity", &self.metadata.capacity())
            .field("chunks", &self.chunks)
            .finish_non_exhaustive()
    }
}

impl SendPlan {
    /// Leading metadata only (never bulk), suitable for bounded stack batching.
    #[must_use]
    pub(crate) fn leading_metadata(&self) -> &[u8] {
        match self.parts.first() {
            Some(Part::Metadata(range)) => &self.metadata[range.clone()],
            _ => &[],
        }
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Borrow ordered nonempty chunks without allocation or materialization.
    ///
    /// The iterator caches part/chunk position. A writer can keep it and a byte
    /// offset across short writes, rather than restart a whole-plan scan.
    #[must_use]
    pub const fn chunks(
        &self,
    ) -> impl ExactSizeIterator<Item = &[u8]> + std::iter::FusedIterator + Clone {
        SendChunks {
            plan: self,
            part: 0,
            chunk: 0,
            remaining: self.chunks,
        }
    }
}

/// Resumable, allocation-free borrowed traversal of a `SendPlan`.
#[derive(Clone)]
struct SendChunks<'a> {
    plan: &'a SendPlan,
    part: usize,
    chunk: usize,
    remaining: usize,
}

impl<'a> Iterator for SendChunks<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        let part = self.plan.parts.get(self.part)?;
        let bytes = match part {
            Part::Metadata(range) => {
                self.part += 1;
                &self.plan.metadata[range.clone()]
            }
            Part::Bulk(payload) => {
                let bytes = payload.segment(self.chunk);
                self.chunk += 1;
                if self.chunk == payload.segment_count() {
                    self.part += 1;
                    self.chunk = 0;
                }
                bytes
            }
        };
        self.remaining -= 1;
        Some(bytes)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}
impl ExactSizeIterator for SendChunks<'_> {}
impl std::iter::FusedIterator for SendChunks<'_> {}

/// Fail-closed builder with incremental checked wire length.
///
/// Reservation work charges full geometric targets (1, 2, 4, ... or the larger
/// immediate requirement) for metadata/part vectors and exact Payload descriptor
/// clone layouts, before allocation. Logical growth targets are tracked separately
/// from allocator-selected Vec capacity. Arc increments allocate nothing.
/// Allocator usable size/overhead is excluded; this is not a process memory cap.
/// Failed builders retain partial storage until drop/finish, never expose it.
pub struct Encoder {
    plan: SendPlan,
    limits: EncodeLimits,
    metadata_target: usize,
    parts_target: usize,
    retained: usize,
    reservation: usize,
    depth: usize,
    error: Option<EncodeError>,
}

const fn check(actual: usize, limit: usize, kind: EncodeLimit) -> EncodeResult<()> {
    if actual > limit {
        return Err(EncodeError::Limit {
            kind,
            limit,
            actual,
        });
    }
    Ok(())
}

fn opaque_length(len: usize) -> EncodeResult<u32> {
    u32::try_from(len).map_err(|_overflow| EncodeError::UnrepresentableLength(len))
}

fn charge(counter: &mut usize, amount: usize, limit: usize, kind: EncodeLimit) -> EncodeResult<()> {
    let actual = counter.checked_add(amount).ok_or(EncodeError::Overflow)?;
    check(actual, limit, kind)?;
    *counter = actual;
    Ok(())
}

fn grow<T>(
    items: &mut Vec<T>,
    target: &mut usize,
    required: usize,
    reservation: &mut usize,
    limit: usize,
) -> EncodeResult<()> {
    if required > *target {
        let next = if *target == 0 {
            1
        } else {
            target.checked_mul(2).ok_or(EncodeError::Overflow)?
        }
        .max(required);
        let bytes = next
            .checked_mul(size_of::<T>())
            .ok_or(EncodeError::Overflow)?;
        charge(reservation, bytes, limit, EncodeLimit::ReservationBytes)?;
        items
            .try_reserve_exact(next - items.len())
            .map_err(|_allocation_error| EncodeError::Allocation)?;
        *target = next;
    }
    Ok(())
}

impl Encoder {
    /// Conservative reservation work for a metadata-only plan, independent of
    /// append sizes. Each growth at least doubles the previous target; the last
    /// target is less than twice the final length. Includes the sole part.
    /// This does not allocate and is not a bound for plans containing bulk.
    #[must_use]
    pub fn metadata_reservation_bound(bytes: usize) -> Option<usize> {
        if bytes == 0 {
            return Some(0);
        }
        bytes.checked_mul(4)?.checked_add(size_of::<Part>())
    }

    #[must_use]
    pub const fn new(limits: EncodeLimits) -> Self {
        Self {
            plan: SendPlan {
                metadata: Vec::new(),
                parts: Vec::new(),
                len: 0,
                chunks: 0,
            },
            limits,
            metadata_target: 0,
            parts_target: 0,
            retained: 0,
            reservation: 0,
            depth: 0,
            error: None,
        }
    }

    fn ready(&self) -> EncodeResult<()> {
        self.error.map_or(Ok(()), Err)
    }

    fn remember<T>(&mut self, result: EncodeResult<T>) -> EncodeResult<T> {
        if self.error.is_none() {
            if let Err(error) = &result {
                self.error = Some(*error);
            }
        }
        self.ready()?;
        result
    }

    /// Record a sticky validation failure.
    pub fn fail<T>(&mut self, error: EncodeError) -> EncodeResult<T> {
        self.remember(Err(error))
    }

    /// Encode a nested value under aggregate depth and sticky-error enforcement.
    pub fn encode<T: Encode + ?Sized>(&mut self, value: &T) -> EncodeResult<()> {
        self.ready()?;
        let result = (|| {
            charge(
                &mut self.depth,
                1,
                self.limits.max_depth,
                EncodeLimit::Depth,
            )?;
            let result = value.encode(self);
            self.depth -= 1;
            result
        })();
        self.remember(result)
    }

    /// Consume the builder, releasing partial storage on failure.
    pub fn finish(self) -> EncodeResult<SendPlan> {
        self.ready()?;
        Ok(self.plan)
    }

    /// Append copied metadata (without a length prefix or implicit padding).
    pub fn metadata(&mut self, bytes: &[u8]) -> EncodeResult<()> {
        self.ready()?;
        let result = (|| {
            if bytes.is_empty() {
                return Ok(());
            }
            charge(
                &mut self.plan.len,
                bytes.len(),
                self.limits.max_wire_bytes,
                EncodeLimit::WireBytes,
            )?;
            let end = self
                .plan
                .metadata
                .len()
                .checked_add(bytes.len())
                .ok_or(EncodeError::Overflow)?;
            check(
                end,
                self.limits.max_metadata_bytes,
                EncodeLimit::MetadataBytes,
            )?;
            let new_part = !matches!(self.plan.parts.last(), Some(Part::Metadata(_)));
            if new_part {
                charge(
                    &mut self.plan.chunks,
                    1,
                    self.limits.max_chunks,
                    EncodeLimit::Chunks,
                )?;
                self.reserve_part()?;
            }
            grow(
                &mut self.plan.metadata,
                &mut self.metadata_target,
                end,
                &mut self.reservation,
                self.limits.max_reservation_bytes,
            )?;
            let start = self.plan.metadata.len();
            self.plan.metadata.extend_from_slice(bytes);
            if let Some(Part::Metadata(range)) = self.plan.parts.last_mut() {
                range.end = end;
            } else {
                self.plan.parts.push(Part::Metadata(start..end));
            }
            Ok(())
        })();
        self.remember(result)
    }

    fn reserve_part(&mut self) -> EncodeResult<()> {
        let required = self
            .plan
            .parts
            .len()
            .checked_add(1)
            .ok_or(EncodeError::Overflow)?;
        grow(
            &mut self.plan.parts,
            &mut self.parts_target,
            required,
            &mut self.reservation,
            self.limits.max_reservation_bytes,
        )
    }

    fn padding(&mut self, len: usize) -> EncodeResult<()> {
        self.metadata(&[0; 3][..(4 - len % 4) % 4])
    }

    /// Copy an opaque field with a protocol byte bound, length and zero padding.
    pub fn opaque(&mut self, bytes: &[u8], max: usize) -> EncodeResult<()> {
        self.ready()?;
        let result = (|| {
            let len = opaque_length(bytes.len())?;
            check(bytes.len(), max, EncodeLimit::FieldBytes)?;
            self.encode(&len)?;
            self.metadata(bytes)?;
            self.padding(bytes.len())
        })();
        self.remember(result)
    }

    /// Encode a u32-counted collection, retaining its exact structural count.
    pub fn collection<T: Encode>(&mut self, items: &[T], max: usize) -> EncodeResult<()> {
        self.ready()?;
        let result = (|| {
            let count = u32::try_from(items.len())
                .map_err(|_overflow| EncodeError::UnrepresentableCount(items.len()))?;
            check(items.len(), max, EncodeLimit::FieldElements)?;
            self.encode(&count)?;
            for item in items {
                self.encode(item)?;
            }
            Ok(())
        })();
        self.remember(result)
    }

    /// Append one length-prefixed bulk field, sharing only its selected owners.
    pub fn payload(&mut self, payload: &Payload) -> EncodeResult<()> {
        self.ready()?;
        let result = (|| {
            let len = opaque_length(payload.len())?;
            self.encode(&len)?;
            if !payload.is_empty() {
                charge(
                    &mut self.plan.len,
                    payload.len(),
                    self.limits.max_wire_bytes,
                    EncodeLimit::WireBytes,
                )?;
                charge(
                    &mut self.plan.chunks,
                    payload.segment_count(),
                    self.limits.max_chunks,
                    EncodeLimit::Chunks,
                )?;
                charge(
                    &mut self.retained,
                    payload.retained_capacity(),
                    self.limits.max_retained_capacity,
                    EncodeLimit::RetainedCapacity,
                )?;
                self.reserve_part()?;
                let owned = payload.slice_charged::<EncodeError>(
                    0..payload.len(),
                    None,
                    0,
                    0,
                    |bytes| {
                        charge(
                            &mut self.reservation,
                            bytes,
                            self.limits.max_reservation_bytes,
                            EncodeLimit::ReservationBytes,
                        )
                    },
                )?;
                self.plan.parts.push(Part::Bulk(owned));
            }
            self.padding(payload.len())
        })();
        self.remember(result)
    }
}

impl Encode for u32 {
    fn encode(&self, encoder: &mut Encoder) -> EncodeResult<()> {
        encoder.metadata(&self.to_be_bytes())
    }
}
impl Encode for u64 {
    fn encode(&self, encoder: &mut Encoder) -> EncodeResult<()> {
        encoder.metadata(&self.to_be_bytes())
    }
}
impl Encode for bool {
    fn encode(&self, encoder: &mut Encoder) -> EncodeResult<()> {
        encoder.encode(&u32::from(*self))
    }
}
impl<const N: usize> Encode for [u8; N] {
    fn encode(&self, encoder: &mut Encoder) -> EncodeResult<()> {
        encoder.metadata(self)?;
        encoder.padding(N)
    }
}
impl Encode for Opaque<'_> {
    fn encode(&self, encoder: &mut Encoder) -> EncodeResult<()> {
        encoder.opaque(self.as_ref(), usize::MAX)
    }
}
impl Encode for Payload {
    fn encode(&self, encoder: &mut Encoder) -> EncodeResult<()> {
        encoder.payload(self)
    }
}
impl<T: Encode> Encode for Vec<T> {
    fn encode(&self, encoder: &mut Encoder) -> EncodeResult<()> {
        encoder.collection(self, usize::MAX)
    }
}
impl<T: Encode> Encode for List<T> {
    fn encode(&self, encoder: &mut Encoder) -> EncodeResult<()> {
        for item in &self.0 {
            encoder.encode(&true)?;
            encoder.encode(item)?;
        }
        encoder.encode(&false)
    }
}
impl Encode for Void {
    fn encode(&self, encoder: &mut Encoder) -> EncodeResult<()> {
        encoder.ready()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> EncodeLimits {
        EncodeLimits {
            max_wire_bytes: usize::MAX,
            max_metadata_bytes: usize::MAX,
            max_chunks: usize::MAX,
            max_retained_capacity: usize::MAX,
            max_depth: 64,
            max_reservation_bytes: usize::MAX,
        }
    }

    #[test]
    fn geometric_growth_and_full_target_charges() {
        let mut e = Encoder::new(limits());
        let mut previous = 0;
        for (len, target) in [(1, 1), (2, 2), (3, 4), (4, 4), (5, 8), (9, 16), (40, 40)] {
            e.metadata(&vec![0; len - previous]).unwrap();
            assert_eq!(e.metadata_target, target);
            previous = len;
        }
        assert_eq!(e.reservation, size_of::<Part>() + 1 + 2 + 4 + 8 + 16 + 40);
        assert_eq!(e.plan.parts.len(), 1);
        let mut counter = usize::MAX;
        assert_eq!(
            charge(&mut counter, 1, usize::MAX, EncodeLimit::WireBytes),
            Err(EncodeError::Overflow)
        );
        let mut items = Vec::<u8>::new();
        let mut target = usize::MAX;
        assert_eq!(
            grow(&mut items, &mut target, usize::MAX, &mut 0, usize::MAX),
            Ok(())
        );
        target = usize::MAX / 2 + 1;
        assert_eq!(
            grow(&mut items, &mut target, usize::MAX, &mut 0, usize::MAX),
            Err(EncodeError::Overflow)
        );
    }

    #[test]
    fn descriptor_charge_and_failure_before_clone() {
        let payload = Payload::try_from_vec(
            vec![1; 4],
            super::super::PayloadLimits {
                max_logical_bytes: 4,
                max_backing_capacity: 4,
                max_segments: 1,
            },
        )
        .unwrap();
        let mut descriptor_bytes = 0;
        payload
            .slice_charged::<EncodeError>(0..4, None, 0, 0, |bytes| {
                descriptor_bytes = bytes;
                Ok(())
            })
            .unwrap();
        let prefix_work = size_of::<Part>() + 4;
        let part_work = 2 * size_of::<Part>();
        let required = prefix_work + part_work + descriptor_bytes;
        let mut policy = limits();
        policy.max_reservation_bytes = required - 1;
        let mut e = Encoder::new(policy);
        assert_eq!(
            e.encode(&payload),
            Err(EncodeError::Limit {
                kind: EncodeLimit::ReservationBytes,
                limit: required - 1,
                actual: required,
            })
        );
        assert_eq!(e.plan.parts.len(), 1);
        assert_eq!(e.reservation, prefix_work + part_work);
        e.finish().unwrap_err();
        policy.max_reservation_bytes = required;
        let mut e = Encoder::new(policy);
        e.encode(&payload).unwrap();
        assert_eq!(e.reservation, required);
        assert_eq!(e.finish().unwrap().len(), 8);
    }

    #[test]
    fn part_growth_charges_full_targets_and_metadata_coalesces() {
        let payload = Payload::try_from_vec(
            vec![1; 4],
            super::super::PayloadLimits {
                max_logical_bytes: 4,
                max_backing_capacity: 4,
                max_segments: 1,
            },
        )
        .unwrap();
        let mut e = Encoder::new(limits());
        e.encode(&payload).unwrap();
        assert_eq!(e.parts_target, 2);
        let first = e.reservation;
        e.encode(&payload).unwrap();
        assert_eq!(e.parts_target, 4);
        // Metadata doubles to 8, part target doubles to 4, one descriptor clone.
        let mut descriptor_bytes = 0;
        payload
            .slice_charged::<EncodeError>(0..4, None, 0, 0, |bytes| {
                descriptor_bytes = bytes;
                Ok(())
            })
            .unwrap();
        assert_eq!(
            e.reservation - first,
            8 + 4 * size_of::<Part>() + descriptor_bytes
        );
        let second = e.reservation;
        e.encode(&payload).unwrap();
        assert_eq!(e.parts_target, 8);
        assert_eq!(
            e.reservation - second,
            16 + 8 * size_of::<Part>() + descriptor_bytes
        );
        let third = e.reservation;
        e.metadata(&[]).unwrap();
        assert_eq!(e.reservation, third);
        assert_eq!(e.plan.parts.len(), 6);
        assert_eq!(e.finish().unwrap().chunks().len(), 6);
    }

    #[test]
    fn arithmetic_and_allocation_errors_are_value_only_and_sticky() {
        fn value_only<T: Copy>() {}

        let mut e = Encoder::new(limits());
        e.plan.len = usize::MAX;
        assert_eq!(e.metadata(&[1]), Err(EncodeError::Overflow));
        assert_eq!(e.finish().err(), Some(EncodeError::Overflow));
        let mut bytes = Vec::<u8>::new();
        let mut reservation = 0;
        assert_eq!(
            grow(&mut bytes, &mut 0, usize::MAX, &mut reservation, usize::MAX),
            Err(EncodeError::Allocation),
        ); // Vec capacity overflow: deterministic, no huge allocation.
        assert_eq!(reservation, usize::MAX);
        assert!(bytes.is_empty());
        assert_eq!(opaque_length(u32::MAX as usize), Ok(u32::MAX));
        #[cfg(target_pointer_width = "64")]
        assert_eq!(
            opaque_length(u32::MAX as usize + 1),
            Err(EncodeError::UnrepresentableLength(u32::MAX as usize + 1)),
        );
        value_only::<EncodeError>();
    }
}
