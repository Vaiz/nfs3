use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use super::budget::{BackingReservation, Ticket};
use super::{PayloadError, PayloadLimit};

#[cfg(test)]
thread_local! {
    static DESCRIPTOR_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Explicit per-view bounds; no product policy or defaults are implied.
///
/// Backing capacity sums Vec capacities per nonempty descriptor, conservatively
/// counting duplicate origins if present. It excludes descriptor/Arc overhead and
/// allocator usable size, and is not a globally unique or process-wide byte count.
/// Limits are checked before descriptor reservation and Arc publication, but
/// cannot bound allocations already made by the caller.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct PayloadLimits {
    /// Maximum visible byte count.
    pub max_logical_bytes: usize,
    /// Maximum retained backing Vec capacity sum.
    pub max_backing_capacity: usize,
    /// Maximum number of nonempty descriptors.
    pub max_segments: usize,
}

impl PayloadLimits {
    pub(super) fn check(
        self,
        len: usize,
        backing: usize,
        segments: usize,
    ) -> Result<(), PayloadError> {
        for (kind, actual, limit) in [
            (PayloadLimit::LogicalBytes, len, self.max_logical_bytes),
            (
                PayloadLimit::BackingCapacity,
                backing,
                self.max_backing_capacity,
            ),
            (PayloadLimit::Segments, segments, self.max_segments),
        ] {
            if actual > limit {
                return Err(PayloadError::LimitExceeded {
                    kind,
                    limit,
                    actual,
                });
            }
        }
        Ok(())
    }
}

struct Chunk {
    bytes: Vec<u8>,
    // Rust drops fields in declaration order: backing must drop before credit.
    _ticket: Option<Ticket>,
}

struct Segment {
    chunk: Arc<Chunk>,
    range: Range<usize>,
}

/// Immutable owned byte ranges, without implicit coalescing or infallible cloning.
///
/// Adopting Vecs, cloning and slicing do not copy payload bytes. Empty views own
/// no chunks. Descriptor Vec reservations are fallible; standard Arc control
/// allocation is not. Neither recoverable OOM nor a total memory cap is promised.
pub struct Payload {
    segments: Vec<Segment>,
    len: usize,
    backing: usize,
}

impl Payload {
    /// Checked reservation-work cost for `segments` selected nonempty descriptors.
    ///
    /// This is the exact charge for one shared decode/clone/slice, not backing
    /// capacity, allocator overhead or cumulative geometric builder growth.
    /// Zero descriptors cost zero; `None` indicates arithmetic overflow.
    #[must_use]
    pub const fn descriptor_reservation_cost(segments: usize) -> Option<usize> {
        segments.checked_mul(size_of::<Segment>())
    }

    /// An empty view with no allocation or backing owner.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            segments: Vec::new(),
            len: 0,
            backing: 0,
        }
    }

    /// Adopt concrete Vec inputs without shrinking or copying their bytes.
    ///
    /// Validates all inputs by reference before allocating descriptors or Arc
    /// owners. Empty inputs are discarded. On error all inputs are consumed/dropped.
    pub fn try_from_chunks(
        chunks: Vec<Vec<u8>>,
        limits: PayloadLimits,
    ) -> Result<Self, PayloadError> {
        let mut len = 0_usize;
        let mut backing = 0_usize;
        let mut count = 0_usize;
        for bytes in &chunks {
            if !bytes.is_empty() {
                len = len.checked_add(bytes.len()).ok_or(PayloadError::Overflow)?;
                backing = backing
                    .checked_add(bytes.capacity())
                    .ok_or(PayloadError::Overflow)?;
                count = count.checked_add(1).ok_or(PayloadError::Overflow)?;
            }
        }
        limits.check(len, backing, count)?;
        let mut segments = Vec::new();
        segments
            .try_reserve_exact(count)
            .map_err(|_allocation_error| PayloadError::Allocation)?;
        for bytes in chunks {
            if !bytes.is_empty() {
                let range = 0..bytes.len();
                segments.push(Segment {
                    chunk: Arc::new(Chunk {
                        bytes,
                        _ticket: None,
                    }),
                    range,
                });
            }
        }
        Ok(Self {
            segments,
            len,
            backing,
        })
    }

    /// Adopt one Vec without copying/shrinking; errors consume/drop the Vec.
    pub fn try_from_vec(bytes: Vec<u8>, limits: PayloadLimits) -> Result<Self, PayloadError> {
        if bytes.is_empty() {
            limits.check(0, 0, 0)?;
            return Ok(Self::empty());
        }
        let len = bytes.len();
        let backing = bytes.capacity();
        limits.check(len, backing, 1)?;
        let mut segments = Vec::new();
        segments
            .try_reserve_exact(1)
            .map_err(|_allocation_error| PayloadError::Allocation)?;
        segments.push(Segment {
            chunk: Arc::new(Chunk {
                bytes,
                _ticket: None,
            }),
            range: 0..len,
        });
        Ok(Self {
            segments,
            len,
            backing,
        })
    }

    /// Visible byte count (cached).
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether this view has no bytes or backing owners.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Number of nonempty range descriptors.
    #[must_use]
    pub const fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// Conservative per-descriptor backing Vec capacity sum, not unique bytes.
    #[must_use]
    pub const fn retained_capacity(&self) -> usize {
        self.backing
    }

    /// Borrow visible chunks in order without allocating or transferring owners.
    #[must_use]
    pub fn chunks(&self) -> impl ExactSizeIterator<Item = &[u8]> + DoubleEndedIterator + '_ {
        self.segments
            .iter()
            .map(|segment| &segment.chunk.bytes[segment.range.clone()])
    }

    /// A borrowed slice only when empty or represented by one descriptor.
    #[must_use]
    pub fn as_contiguous(&self) -> Option<&[u8]> {
        match self.segments.as_slice() {
            [] => Some(&[]),
            [segment] => Some(&segment.chunk.bytes[segment.range.clone()]),
            _ => None,
        }
    }

    /// Fallibly duplicate descriptors, sharing backing bytes.
    pub fn try_clone(&self) -> Result<Self, PayloadError> {
        self.try_slice(0..self.len)
    }

    /// Share a checked range, retaining only intersected chunks.
    ///
    /// This cannot increase logical length, backing capacity or segment count.
    /// Empty ranges retain no chunk; invalid ranges fail even when empty.
    pub fn try_slice(&self, range: Range<usize>) -> Result<Self, PayloadError> {
        self.slice_with_limits(range, None, 0, 0)
    }

    // Internal hint: start_segment/start_base identify a descriptor at or before
    // range.start (or EOF for an empty range). Cursor uses its cache; public slices
    // start at zero. The base is logical, independent of the descriptor's range.
    pub(super) fn slice_with_limits(
        &self,
        range: Range<usize>,
        limits: Option<PayloadLimits>,
        start_segment: usize,
        start_base: usize,
    ) -> Result<Self, PayloadError> {
        self.slice_charged(range, limits, start_segment, start_base, |_| Ok(()))
    }

    pub(super) fn slice_charged<E: From<PayloadError>>(
        &self,
        range: Range<usize>,
        limits: Option<PayloadLimits>,
        start_segment: usize,
        start_base: usize,
        charge: impl FnOnce(usize) -> Result<(), E>,
    ) -> Result<Self, E> {
        let Range { start, end } = range;
        if start > end || end > self.len {
            return Err(PayloadError::InvalidRange {
                start: range.start,
                end: range.end,
                len: self.len,
            }
            .into());
        }
        let len = range.end - range.start;
        let mut count = 0_usize;
        let mut backing = 0_usize;
        let mut base = start_base;
        for segment in &self.segments[start_segment..] {
            if base >= range.end || len == 0 {
                break;
            }
            #[cfg(test)]
            DESCRIPTOR_VISITS.with(|visits| visits.set(visits.get() + 1));
            let end = base + segment.range.len(); // bounded by validated self.len
            if range.start < end && base < range.end && len != 0 {
                count = count.checked_add(1).ok_or(PayloadError::Overflow)?;
                backing = backing
                    .checked_add(segment.chunk.bytes.capacity())
                    .ok_or(PayloadError::Overflow)?;
            }
            base = end;
        }
        if let Some(limits) = limits {
            limits.check(len, backing, count)?;
        }
        charge(Self::descriptor_reservation_cost(count).ok_or(PayloadError::Overflow)?)?;
        let mut segments = Vec::new();
        segments
            .try_reserve_exact(count)
            .map_err(|_allocation_error| PayloadError::Allocation)?;
        if len != 0 {
            let mut base = start_base;
            for segment in &self.segments[start_segment..] {
                if base >= range.end {
                    break;
                }
                #[cfg(test)]
                DESCRIPTOR_VISITS.with(|visits| visits.set(visits.get() + 1));
                let end = base + segment.range.len();
                if range.start < end && base < range.end {
                    let start_in_segment = range.start.saturating_sub(base);
                    let end_in_segment = range.end.min(end) - base;
                    segments.push(Segment {
                        chunk: Arc::clone(&segment.chunk),
                        range: segment.range.start + start_in_segment
                            ..segment.range.start + end_in_segment,
                    });
                }
                base = end;
            }
        }
        Ok(Self {
            segments,
            len,
            backing,
        })
    }

    /// Explicitly copy all visible bytes into a newly reserved contiguous Vec.
    ///
    /// Reserves for logical length, not retained backing capacity. The returned
    /// Vec is independent; its allocator-selected capacity is not a payload bound.
    pub fn try_to_vec(&self) -> Result<Vec<u8>, PayloadError> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(self.len)
            .map_err(|_allocation_error| PayloadError::Allocation)?;
        for chunk in self.chunks() {
            bytes.extend_from_slice(chunk);
        }
        Ok(bytes)
    }

    pub(super) fn segment(&self, index: usize) -> &[u8] {
        let segment = &self.segments[index];
        &segment.chunk.bytes[segment.range.clone()]
    }
}

impl fmt::Debug for Payload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Payload")
            .field("len", &self.len)
            .field("segments", &self.segments.len())
            .field("retained_capacity", &self.backing)
            .finish()
    }
}

/// Owned, safely initialized writable receive storage; no IO adapter is implied.
///
/// Allocation reserves fallibly, checks actual Vec capacity, then zero-fills the
/// requested length. Rejecting excess capacity does not bound transient allocation
/// or allocator usable size. Arc allocation during freeze remains infallible.
pub struct ReceiveBuffer {
    bytes: Vec<u8>,
    ticket: Option<Ticket>,
}

impl ReceiveBuffer {
    /// Reserve and zero-fill `len` writable bytes, rejecting capacity above the bound.
    pub fn try_new(len: usize, max_capacity: usize) -> Result<Self, PayloadError> {
        if len > max_capacity {
            return Err(PayloadError::LimitExceeded {
                kind: PayloadLimit::BackingCapacity,
                limit: max_capacity,
                actual: len,
            });
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(len)
            .map_err(|_allocation_error| PayloadError::Allocation)?;
        if bytes.capacity() > max_capacity {
            return Err(PayloadError::LimitExceeded {
                kind: PayloadLimit::BackingCapacity,
                limit: max_capacity,
                actual: bytes.capacity(),
            });
        }
        bytes.resize(len, 0);
        Ok(Self {
            bytes,
            ticket: None,
        })
    }

    /// Spend reserved backing before allocating initialized writable storage.
    ///
    /// Actual capacity must not exceed `len`. Rejected allocator over-allocation
    /// is transient and never published. Errors release the spent entitlement.
    pub fn try_new_charged(
        len: usize,
        reservation: &mut BackingReservation,
    ) -> Result<Self, PayloadError> {
        let ticket = reservation.take(len)?;
        let mut buffer = Self::try_new(len, len)?;
        buffer.ticket = Some(ticket);
        Ok(buffer)
    }

    /// Initialized writable storage, including zero-filled bytes not yet received.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.bytes
    }

    /// Actual backing Vec capacity, excluding allocator/control overhead.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.bytes.capacity()
    }

    /// Consume the buffer and publish only a checked prefix without copying/shrinking.
    ///
    /// Prefix zero releases all storage. Any error consumes/drops the buffer.
    /// Payload bounds are independent of the receive construction capacity bound.
    pub fn freeze(mut self, prefix: usize, limits: PayloadLimits) -> Result<Payload, PayloadError> {
        if prefix > self.bytes.len() {
            return Err(PayloadError::InvalidRange {
                start: 0,
                end: prefix,
                len: self.bytes.len(),
            });
        }
        let work = RecordBuilder::reservation_bound(1).ok_or(PayloadError::Overflow)?;
        let mut builder = RecordBuilder::new(limits, work);
        self.bytes.truncate(prefix);
        builder.push(self, prefix)?;
        Ok(builder.finish())
    }
}

/// Incremental consuming record construction, without rescanning earlier blocks.
///
/// Descriptor growth is geometric and checked before allocation; actual accepted
/// capacities and cumulative descriptor/control reservation work are bounded.
/// Arc allocator/header overhead and allocator transients are not exact bounds.
pub struct RecordBuilder {
    payload: Payload,
    limits: PayloadLimits,
    work: usize,
    max_work: usize,
}

impl RecordBuilder {
    /// Conservative checked work envelope for up to `blocks` nonempty origins.
    /// Includes geometric descriptor targets and known Chunk control layouts.
    #[must_use]
    pub fn reservation_bound(blocks: usize) -> Option<usize> {
        blocks.checked_mul(
            size_of::<Segment>()
                .checked_mul(4)?
                .checked_add(size_of::<Chunk>())?,
        )
    }

    /// Explicit record and cumulative descriptor/control reservation-work limits.
    #[must_use]
    pub const fn new(limits: PayloadLimits, max_reservation_bytes: usize) -> Self {
        Self {
            payload: Payload::empty(),
            limits,
            work: 0,
            max_work: max_reservation_bytes,
        }
    }

    fn charge_work(&mut self, bytes: usize) -> Result<(), PayloadError> {
        self.work = self.work.checked_add(bytes).ok_or(PayloadError::Overflow)?;
        if self.work > self.max_work {
            return Err(PayloadError::Allocation);
        }
        Ok(())
    }

    /// Move a received prefix and its charge; empty/error inputs release backing.
    pub fn push(&mut self, mut buffer: ReceiveBuffer, prefix: usize) -> Result<(), PayloadError> {
        if prefix > buffer.bytes.len() {
            return Err(PayloadError::InvalidRange {
                start: 0,
                end: prefix,
                len: buffer.bytes.len(),
            });
        }
        if prefix == 0 {
            return Ok(());
        }
        let len = self
            .payload
            .len
            .checked_add(prefix)
            .ok_or(PayloadError::Overflow)?;
        let backing = self
            .payload
            .backing
            .checked_add(buffer.capacity())
            .ok_or(PayloadError::Overflow)?;
        let count = self
            .payload
            .segments
            .len()
            .checked_add(1)
            .ok_or(PayloadError::Overflow)?;
        self.limits.check(len, backing, count)?;
        if count > self.payload.segments.capacity() {
            let capacity = count
                .checked_mul(2)
                .ok_or(PayloadError::Overflow)?
                .min(self.limits.max_segments);
            self.charge_work(
                capacity
                    .checked_mul(size_of::<Segment>())
                    .ok_or(PayloadError::Overflow)?,
            )?;
            self.payload
                .segments
                .try_reserve_exact(capacity - self.payload.segments.len())
                .map_err(|_allocation_error| PayloadError::Allocation)?;
            if self.payload.segments.capacity() > capacity {
                return Err(PayloadError::Allocation);
            }
        }
        self.charge_work(size_of::<Chunk>())?;
        buffer.bytes.truncate(prefix);
        self.payload.segments.push(Segment {
            chunk: Arc::new(Chunk {
                bytes: buffer.bytes,
                _ticket: buffer.ticket,
            }),
            range: 0..prefix,
        });
        self.payload.len = len;
        self.payload.backing = backing;
        Ok(())
    }

    /// Consume the builder, moving descriptors and charges without copying bytes.
    #[must_use]
    pub fn finish(self) -> Payload {
        self.payload
    }
}

impl fmt::Debug for ReceiveBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReceiveBuffer")
            .field("len", &self.bytes.len())
            .field("capacity", &self.bytes.capacity())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::xdr_codec::RecordCursor;

    #[test]
    fn opaque_slicing_visits_only_selected_descriptors() {
        let limits = PayloadLimits {
            max_logical_bytes: usize::MAX,
            max_backing_capacity: usize::MAX,
            max_segments: usize::MAX,
        };
        for prefix in [0, 32] {
            for tail in [0, 32] {
                for touched in [1, 3] {
                    let mut chunks = vec![vec![0; 8]; prefix];
                    if touched == 1 {
                        chunks.push(vec![0, 0, 0, 3, b'a', b'b', b'c', 0]);
                    } else {
                        chunks.extend([vec![0, 0, 0, 3, b'a'], vec![b'b'], vec![b'c', 0]]);
                    }
                    chunks.extend(vec![vec![0; 8]; tail]);
                    let record = Payload::try_from_chunks(chunks, limits).unwrap();
                    let mut cursor = RecordCursor::new(record);
                    for _ in 0..prefix {
                        assert_eq!(cursor.read_u64().unwrap(), 0);
                    }
                    DESCRIPTOR_VISITS.set(0);
                    let payload = cursor.read_opaque(limits).unwrap();
                    assert_eq!(
                        DESCRIPTOR_VISITS.get(),
                        2 * touched,
                        "prefix={prefix}, tail={tail}, touched={touched}"
                    );
                    assert_eq!(payload.try_to_vec().unwrap(), b"abc");
                    assert_eq!(cursor.position(), prefix * 8 + 8);
                }
            }
        }
    }

    #[test]
    fn public_slicing_stops_after_range_end() {
        let limits = PayloadLimits {
            max_logical_bytes: 128,
            max_backing_capacity: 128,
            max_segments: 128,
        };
        let payload =
            Payload::try_from_chunks((0..128).map(|byte| vec![byte]).collect(), limits).unwrap();
        DESCRIPTOR_VISITS.set(0);
        let selected = payload.try_slice(2..4).unwrap();
        assert_eq!(DESCRIPTOR_VISITS.get(), 8);
        assert_eq!(selected.try_to_vec().unwrap(), [2, 3]);
        DESCRIPTOR_VISITS.set(0);
        assert!(payload.try_slice(99..99).unwrap().is_empty());
        assert_eq!(DESCRIPTOR_VISITS.get(), 0);
    }

    #[test]
    fn selected_ranges_release_unselected_origins() {
        let limits = PayloadLimits {
            max_logical_bytes: 8,
            max_backing_capacity: 8,
            max_segments: 2,
        };
        let payload = Payload::try_from_chunks(vec![vec![1, 2], vec![3, 4]], limits).unwrap();
        let first = Arc::downgrade(&payload.segments[0].chunk);
        let second = Arc::downgrade(&payload.segments[1].chunk);
        let selected = payload.try_slice(2..3).unwrap();
        let empty = payload.try_slice(1..1).unwrap();
        drop(payload);
        assert!(first.upgrade().is_none());
        assert!(second.upgrade().is_some());
        drop(selected);
        assert!(second.upgrade().is_none());
        assert!(empty.segments.is_empty());
    }

    #[test]
    fn extracted_payload_outlives_cursor_without_unselected_chunks() {
        let limits = PayloadLimits {
            max_logical_bytes: 8,
            max_backing_capacity: 8,
            max_segments: 3,
        };
        let record =
            Payload::try_from_chunks(vec![vec![0, 0, 0, 3], b"abc".to_vec(), vec![0]], limits)
                .unwrap();
        let header = Arc::downgrade(&record.segments[0].chunk);
        let body = Arc::downgrade(&record.segments[1].chunk);
        let padding = Arc::downgrade(&record.segments[2].chunk);
        let mut cursor = RecordCursor::new(record);
        let payload = cursor.read_opaque(limits).unwrap();
        cursor.finish().unwrap();
        assert!(header.upgrade().is_none());
        assert!(padding.upgrade().is_none());
        assert!(body.upgrade().is_some());
        drop(payload);
        assert!(body.upgrade().is_none());
    }

    #[test]
    fn copied_metadata_releases_oversized_origin_after_finish() {
        let limits = PayloadLimits {
            max_logical_bytes: 8,
            max_backing_capacity: 256,
            max_segments: 1,
        };
        let mut bytes = Vec::with_capacity(256);
        bytes.extend_from_slice(&[0, 0, 0, 3, b'a', b'b', b'c', 0]);
        let record = Payload::try_from_vec(bytes, limits).unwrap();
        let origin = Arc::downgrade(&record.segments[0].chunk);
        let mut cursor = RecordCursor::new(record);
        cursor
            .read_opaque(PayloadLimits {
                max_backing_capacity: 8,
                ..limits
            })
            .unwrap_err();
        let mut output = [0; 3];
        cursor.read_opaque_into(&mut output, 3).unwrap();
        cursor.finish().unwrap();
        assert!(origin.upgrade().is_none());
        assert_eq!(&output, b"abc");
    }

    #[test]
    fn finish_error_and_abandoned_cursor_release_record() {
        let limits = PayloadLimits {
            max_logical_bytes: 4,
            max_backing_capacity: 4,
            max_segments: 1,
        };
        for finish in [false, true] {
            let record = Payload::try_from_vec(vec![0; 4], limits).unwrap();
            let origin = Arc::downgrade(&record.segments[0].chunk);
            let cursor = RecordCursor::new(record);
            if finish {
                let error = cursor.finish().unwrap_err();
                assert!(origin.upgrade().is_none());
                assert_eq!(
                    error,
                    PayloadError::Trailing {
                        offset: 0,
                        remaining: 4
                    }
                );
            } else {
                drop(cursor);
                assert!(origin.upgrade().is_none());
            }
        }
    }

    #[test]
    fn decoder_metadata_failure_and_abandonment_release_record() {
        use crate::xdr_codec::{DecodeLimits, Decoder, Opaque};
        let payload = PayloadLimits {
            max_logical_bytes: 32,
            max_backing_capacity: 256,
            max_segments: 8,
        };
        let policy = DecodeLimits {
            max_metadata_bytes: 8,
            max_collection_elements: 8,
            max_depth: 8,
            max_reservation_bytes: 1024,
            payload,
        };
        for finish in [false, true] {
            for fail in [false, true] {
                let mut bytes = Vec::with_capacity(256);
                bytes.extend_from_slice(&[0, 0, 0, 3, b'a', b'b', b'c', 0]);
                let record = Payload::try_from_vec(bytes, payload).unwrap();
                let origin = Arc::downgrade(&record.segments[0].chunk);
                let mut d = Decoder::new(record, policy);
                let value: Opaque<'_> = d.decode().unwrap();
                if fail {
                    d.decode::<u32>().unwrap_err();
                }
                assert!(origin.upgrade().is_some());
                if finish {
                    assert_eq!(d.finish().is_err(), fail);
                } else {
                    drop(d);
                }
                assert!(origin.upgrade().is_none());
                assert_eq!(value.as_ref(), b"abc");
            }
        }
    }

    #[test]
    fn send_plan_retains_only_selected_origins_and_releases_on_every_exit() {
        use crate::xdr_codec::{EncodeError, EncodeLimits, Encoder};
        let payload_limits = PayloadLimits {
            max_logical_bytes: 32,
            max_backing_capacity: 32,
            max_segments: 3,
        };
        let policy = EncodeLimits {
            max_wire_bytes: 128,
            max_metadata_bytes: 128,
            max_chunks: 16,
            max_retained_capacity: 32,
            max_depth: 8,
            max_reservation_bytes: 4096,
        };
        for outcome in 0..4 {
            let record = Payload::try_from_chunks(
                vec![vec![0; 4], b"abc".to_vec(), vec![0; 4]],
                payload_limits,
            )
            .unwrap();
            let origins: Vec<_> = record
                .segments
                .iter()
                .map(|s| Arc::downgrade(&s.chunk))
                .collect();
            let selected = record.try_slice(4..7).unwrap();
            let pointer = selected.chunks().next().unwrap().as_ptr();
            let mut e = Encoder::new(policy);
            e.encode(&selected).unwrap();
            drop(selected);
            drop(record);
            assert!(origins[0].upgrade().is_none());
            assert!(origins[1].upgrade().is_some());
            assert!(origins[2].upgrade().is_none());
            match outcome {
                0 => {
                    let plan = e.finish().unwrap();
                    assert_eq!(plan.chunks().nth(1).unwrap().as_ptr(), pointer);
                    assert!(origins[1].upgrade().is_some());
                    drop(plan);
                }
                1 => drop(e), // Abandonment/cancellation.
                2 => {
                    let error = e
                        .fail::<()>(EncodeError::InvalidRepresentation)
                        .unwrap_err();
                    assert_eq!(e.finish().err(), Some(error));
                    // Keeping the error does not keep the backing.
                    assert_eq!(error, EncodeError::InvalidRepresentation);
                }
                _ => {
                    let _ = e.fail::<()>(EncodeError::InvalidRepresentation);
                    drop(e);
                }
            }
            assert!(origins[1].upgrade().is_none());
        }
    }
}
