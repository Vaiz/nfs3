use std::fmt;
use std::ops::Range;

use super::{Payload, PayloadError, PayloadLimit, PayloadLimits};

#[cfg(test)]
#[path = "cursor_tests.rs"]
mod tests;

#[derive(Copy, Clone, Default)]
struct Position {
    byte: usize,
    segment: usize,
    offset: usize,
}

/// Checked big-endian XDR reads from an owned, complete segmented record.
///
/// Every failed read leaves the position and cached traversal state unchanged.
/// Reads never coalesce the record; scalar reads use at most eight stack bytes.
/// The cursor retains the full record until finish/drop. Extracted payloads retain
/// only selected chunks and may outlive the cursor. This additive API rejects
/// nonzero XDR padding; legacy codecs and transports are unchanged.
pub struct RecordCursor {
    record: Payload,
    position: Position,
}

impl RecordCursor {
    /// Own a complete record, initially positioned at byte zero.
    #[must_use]
    pub fn new(record: Payload) -> Self {
        Self {
            record,
            position: Position::default(),
        }
    }

    /// Number of successfully consumed bytes.
    #[must_use]
    pub const fn position(&self) -> usize {
        self.position.byte
    }

    /// Number of unconsumed bytes.
    #[must_use]
    pub const fn remaining(&self) -> usize {
        self.record.len() - self.position.byte
    }

    /// Read a big-endian XDR unsigned integer.
    pub fn read_u32(&mut self) -> Result<u32, PayloadError> {
        let (value, next) = self.peek_u32()?;
        self.position = next;
        Ok(value)
    }

    /// Read a big-endian XDR unsigned hyper integer.
    pub fn read_u64(&mut self) -> Result<u64, PayloadError> {
        self.require(self.position, 8)?;
        let mut bytes = [0; 8];
        let next = self.copy_at(self.position, &mut bytes);
        self.position = next;
        Ok(u64::from_be_bytes(bytes))
    }

    /// Read a strict XDR boolean (only zero and one are valid).
    pub fn read_bool(&mut self) -> Result<bool, PayloadError> {
        let (value, next) = self.peek_u32()?;
        let result = match value {
            0 => false,
            1 => true,
            _ => {
                return Err(PayloadError::InvalidBool {
                    offset: self.position.byte,
                    value,
                });
            }
        };
        self.position = next;
        Ok(result)
    }

    /// Copy exactly the output slice length; errors leave output untouched.
    #[cfg(test)]
    pub fn read_exact(&mut self, output: &mut [u8]) -> Result<(), PayloadError> {
        self.require(self.position, output.len())?;
        self.position = self.copy_at(self.position, output);
        Ok(())
    }

    /// Extract an owned shared XDR variable opaque value, without copying bytes.
    ///
    /// Checks length, full wire availability, zero padding and all per-view limits
    /// before descriptor allocation or committing progress. Small values in large
    /// backing chunks may exceed the backing bound; no copying fallback occurs.
    #[cfg(test)]
    pub fn read_opaque(&mut self, limits: PayloadLimits) -> Result<Payload, PayloadError> {
        self.read_opaque_charged(limits, |_| Ok(()))
    }

    pub(super) fn read_opaque_charged<E: From<PayloadError>>(
        &mut self,
        limits: PayloadLimits,
        charge: impl FnOnce(usize) -> Result<(), E>,
    ) -> Result<Payload, E> {
        let (range, next) = self.opaque_range(limits.max_logical_bytes)?;
        let data_position = self.advance(self.position, 4);
        let payload = self.record.slice_charged(
            range,
            Some(limits),
            data_position.segment,
            data_position.byte - data_position.offset,
            charge,
        )?;
        self.position = next;
        Ok(payload)
    }

    pub(super) fn opaque_len(&self, max: usize) -> Result<usize, PayloadError> {
        self.opaque_range(max).map(|(range, _)| range.len())
    }

    pub(super) fn read_fixed(&mut self, output: &mut [u8]) -> Result<(), PayloadError> {
        let padding = (4 - output.len() % 4) % 4;
        let total = output
            .len()
            .checked_add(padding)
            .ok_or(PayloadError::Overflow)?;
        self.require(self.position, total)?;
        let pad_position = self.advance(self.position, output.len());
        let mut bytes = [0; 3];
        let next = self.copy_at(pad_position, &mut bytes[..padding]);
        for (index, &value) in bytes[..padding].iter().enumerate() {
            if value != 0 {
                return Err(PayloadError::NonzeroPadding {
                    offset: pad_position.byte + index,
                    value,
                });
            }
        }
        self.copy_at(self.position, output);
        self.position = next;
        Ok(())
    }

    /// Explicitly copy bounded opaque metadata into caller storage.
    ///
    /// Returns bytes written into the output prefix; the rest is unchanged.
    /// Checks logical length, output capacity, wire availability and zero padding
    /// before modifying output or progress. No source owner escapes.
    pub fn read_opaque_into(
        &mut self,
        output: &mut [u8],
        max_logical_bytes: usize,
    ) -> Result<usize, PayloadError> {
        let (range, next) = self.opaque_range(max_logical_bytes)?;
        let len = range.len();
        if len > output.len() {
            return Err(PayloadError::BufferTooSmall {
                needed: len,
                available: output.len(),
            });
        }
        let data_position = self.advance(self.position, 4);
        self.copy_at(data_position, &mut output[..len]);
        self.position = next;
        Ok(len)
    }

    /// Require exact consumption and drop the record on either outcome.
    ///
    /// An error contains only the trailing offset/count, never the record.
    pub fn finish(self) -> Result<(), PayloadError> {
        if self.remaining() == 0 {
            Ok(())
        } else {
            Err(PayloadError::Trailing {
                offset: self.position.byte,
                remaining: self.remaining(),
            })
        }
    }

    const fn require(&self, position: Position, count: usize) -> Result<(), PayloadError> {
        let remaining = self.record.len() - position.byte;
        if count > remaining {
            return Err(PayloadError::Truncated {
                offset: position.byte,
                needed: count,
                remaining,
            });
        }
        Ok(())
    }

    fn peek_u32(&self) -> Result<(u32, Position), PayloadError> {
        self.require(self.position, 4)?;
        let mut bytes = [0; 4];
        let next = self.copy_at(self.position, &mut bytes);
        Ok((u32::from_be_bytes(bytes), next))
    }

    fn opaque_range(
        &self,
        max_logical_bytes: usize,
    ) -> Result<(Range<usize>, Position), PayloadError> {
        let (wire_len, data_position) = self.peek_u32()?;
        let len = usize::try_from(wire_len).map_err(|_overflow| PayloadError::Overflow)?;
        if len > max_logical_bytes {
            return Err(PayloadError::LimitExceeded {
                kind: PayloadLimit::LogicalBytes,
                limit: max_logical_bytes,
                actual: len,
            });
        }
        let padding = (4 - len % 4) % 4;
        let padded_len = len.checked_add(padding).ok_or(PayloadError::Overflow)?;
        self.require(data_position, padded_len)?;
        let end = data_position
            .byte
            .checked_add(len)
            .ok_or(PayloadError::Overflow)?;
        let padding_position = self.advance(data_position, len);
        let mut bytes = [0; 3];
        let next = self.copy_at(padding_position, &mut bytes[..padding]);
        for (index, &value) in bytes[..padding].iter().enumerate() {
            if value != 0 {
                return Err(PayloadError::NonzeroPadding {
                    offset: end + index,
                    value,
                });
            }
        }
        Ok((data_position.byte..end, next))
    }

    // Internal traversal only follows ranges already checked against record length.
    // Cache is normalized to the next descriptor at exact boundaries, including EOF.
    fn advance(&self, mut position: Position, mut count: usize) -> Position {
        while count != 0 {
            let chunk = self.record.segment(position.segment);
            let step = count.min(chunk.len() - position.offset);
            position.byte += step;
            position.offset += step;
            count -= step;
            if position.offset == chunk.len() {
                position.segment += 1;
                position.offset = 0;
            }
        }
        position
    }

    fn copy_at(&self, mut position: Position, mut output: &mut [u8]) -> Position {
        while !output.is_empty() {
            let chunk = self.record.segment(position.segment);
            let step = output.len().min(chunk.len() - position.offset);
            output[..step].copy_from_slice(&chunk[position.offset..position.offset + step]);
            output = &mut output[step..];
            position.byte += step;
            position.offset += step;
            if position.offset == chunk.len() {
                position.segment += 1;
                position.offset = 0;
            }
        }
        position
    }
}

impl fmt::Debug for RecordCursor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecordCursor")
            .field("position", &self.position.byte)
            .field("remaining", &self.remaining())
            .field("segments", &self.record.segment_count())
            .finish()
    }
}
