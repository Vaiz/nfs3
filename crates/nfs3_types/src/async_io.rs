//! Runtime-neutral byte IO and bounded ONC RPC record framing.

use std::io::{ErrorKind, IoSlice};

use crate::xdr_codec::{
    BackingReservation, Payload, PayloadError, PayloadLimits, ReceiveBuffer, RecordBuilder,
    SendPlan,
};

/// Runtime adapters must report actual progress, including zero and short IO.
pub trait AsyncRead: Send {
    /// Read into initialized storage, returning actual progress (zero means EOF).
    fn async_read(&mut self, buf: &mut [u8])
    -> impl Future<Output = std::io::Result<usize>> + Send;
    /// Fill the buffer, retrying interruptions; zero progress is `UnexpectedEof`.
    fn async_read_exact(
        &mut self,
        buf: &mut [u8],
    ) -> impl Future<Output = std::io::Result<()>> + Send {
        async move { read_exact(self, buf).await }
    }
}

/// Runtime-neutral writes. Implementations must not report more bytes than offered.
pub trait AsyncWrite: Send {
    /// Write some bytes, reporting actual progress.
    fn async_write(&mut self, buf: &[u8]) -> impl Future<Output = std::io::Result<usize>> + Send;
    /// Write an ordered prefix of the slices. The default uses the first nonempty
    /// slice without copying; runtime adapters may override with vectored IO.
    fn async_write_vectored(
        &mut self,
        bufs: &[IoSlice<'_>],
    ) -> impl Future<Output = std::io::Result<usize>> + Send {
        async move {
            match bufs.iter().find(|buf| !buf.is_empty()) {
                Some(buf) => self.async_write(buf).await,
                None => Ok(0),
            }
        }
    }
    /// Write every byte, retrying interruptions; zero progress is `WriteZero`.
    fn async_write_all(
        &mut self,
        mut buf: &[u8],
    ) -> impl Future<Output = std::io::Result<()>> + Send {
        async move {
            while !buf.is_empty() {
                match self.async_write(buf).await {
                    Ok(0) => return Err(ErrorKind::WriteZero.into()),
                    Ok(n) if n <= buf.len() => buf = &buf[n..],
                    Ok(_) => return Err(ErrorKind::InvalidData.into()),
                    Err(e) if e.kind() == ErrorKind::Interrupted => {}
                    Err(e) => return Err(e),
                }
            }
            Ok(())
        }
    }
}

#[derive(Debug)]
pub enum FramingError {
    Io(std::io::Error),
    Limit,
    Storage(PayloadError),
}

impl std::fmt::Display for FramingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "record IO: {error}"),
            Self::Limit => f.write_str("record framing limit exceeded or invalid policy"),
            Self::Storage(error) => write!(f, "record storage: {error}"),
        }
    }
}

impl std::error::Error for FramingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Storage(error) => Some(error),
            Self::Limit => None,
        }
    }
}

/// Explicit framing policy; validate before starting IO.
#[derive(Debug, Copy, Clone)]
pub struct RecordLimits {
    pub block_bytes: usize,
    pub max_record_bytes: usize,
    pub max_fragments: usize,
}

/// Checked descriptor/control reservation work for up to `blocks` received origins.
/// Shared by framing admission and owned-buffer completion; allocates no storage.
#[must_use]
pub fn record_reservation_bound(blocks: usize) -> Option<usize> {
    RecordBuilder::reservation_bound(blocks)
}

impl RecordLimits {
    #[must_use]
    pub fn validate(self) -> bool {
        self.block_bytes != 0
            && self.max_record_bytes != 0
            && self.max_record_bytes <= 0x7fff_ffff
            && self.max_fragments != 0
            && self.builder_reservation_bound().is_some()
    }
    /// Checked descriptor/control work for incremental record construction.
    /// This is reservation entitlement, not an eager allocation or process-memory bound.
    #[must_use]
    pub fn builder_reservation_bound(self) -> Option<usize> {
        if self.block_bytes == 0 {
            return None;
        }
        record_reservation_bound(self.max_record_bytes.div_ceil(self.block_bytes))
    }

    const fn payload(self) -> PayloadLimits {
        PayloadLimits {
            max_logical_bytes: self.max_record_bytes,
            max_backing_capacity: self.max_record_bytes,
            max_segments: self.max_record_bytes.div_ceil(self.block_bytes),
        }
    }
}

async fn read_exact<IO: AsyncRead + ?Sized>(
    io: &mut IO,
    mut bytes: &mut [u8],
) -> std::io::Result<()> {
    while !bytes.is_empty() {
        match io.async_read(bytes).await {
            Ok(0) => return Err(ErrorKind::UnexpectedEof.into()),
            Ok(n) if n <= bytes.len() => bytes = &mut bytes[n..],
            Ok(_) => return Err(ErrorKind::InvalidData.into()),
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Reads only the first marker. Boundary EOF is clean; partial EOF is an error.
pub async fn first_marker<IO: AsyncRead>(io: &mut IO) -> std::io::Result<Option<u32>> {
    let mut marker = [0; 4];
    loop {
        match io.async_read(&mut marker).await {
            Ok(0) => return Ok(None),
            Ok(n) if n <= 4 => {
                read_exact(io, &mut marker[n..]).await?;
                return Ok(Some(u32::from_be_bytes(marker)));
            }
            Ok(_) => return Err(ErrorKind::InvalidData.into()),
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}

/// Validate policy before reading any marker bytes, then receive one record.
/// Storage grows incrementally under the supplied backing reservation.
pub async fn receive<IO: AsyncRead>(
    io: &mut IO,
    limits: RecordLimits,
    reservation: &mut BackingReservation,
) -> Result<Payload, FramingError> {
    if !limits.validate() {
        return Err(FramingError::Limit);
    }
    let marker = first_marker(io)
        .await
        .map_err(FramingError::Io)?
        .ok_or_else(|| FramingError::Io(ErrorKind::UnexpectedEof.into()))?;
    receive_with_marker(io, marker, limits, reservation).await
}

/// Continue after admission without rereading the marker or reading ahead.
pub async fn receive_with_marker<IO: AsyncRead>(
    io: &mut IO,
    mut marker: u32,
    limits: RecordLimits,
    reservation: &mut BackingReservation,
) -> Result<Payload, FramingError> {
    if !limits.validate() {
        return Err(FramingError::Limit);
    }
    let work = limits
        .builder_reservation_bound()
        .ok_or(FramingError::Limit)?;
    let mut builder = RecordBuilder::new(limits.payload(), work);
    let mut buffer: Option<ReceiveBuffer> = None;
    let mut filled = 0;
    let mut allocated = 0;
    let mut total = 0_usize;
    let mut fragments = 0_usize;
    loop {
        fragments = fragments.checked_add(1).ok_or(FramingError::Limit)?;
        let final_fragment = marker & 0x8000_0000 != 0;
        let mut remaining = (marker & 0x7fff_ffff) as usize;
        total = total.checked_add(remaining).ok_or(FramingError::Limit)?;
        if total > limits.max_record_bytes || fragments > limits.max_fragments {
            return Err(FramingError::Limit);
        }
        while remaining != 0 {
            if buffer.is_none() {
                let mut len = limits.block_bytes.min(limits.max_record_bytes - allocated);
                if final_fragment {
                    len = len.min(remaining);
                }
                let next = ReceiveBuffer::try_new_charged(len, reservation)
                    .map_err(FramingError::Storage)?;
                #[cfg(test)]
                RECEIVE_ALLOCATIONS.with(|counter| {
                    let (count, bytes) = counter.get();
                    counter.set((count + 1, bytes + next.capacity()));
                });
                allocated += next.capacity();
                buffer = Some(next);
            }
            if let Some(current) = &mut buffer {
                let writable = current.as_mut_slice();
                let count = remaining.min(writable.len() - filled);
                read_exact(io, &mut writable[filled..filled + count])
                    .await
                    .map_err(FramingError::Io)?;
                filled += count;
                remaining -= count;
                if filled == writable.len() {
                    if let Some(full) = buffer.take() {
                        builder.push(full, filled).map_err(FramingError::Storage)?;
                    }
                    filled = 0;
                }
            }
        }
        if final_fragment {
            if let Some(last) = buffer.take() {
                builder.push(last, filled).map_err(FramingError::Storage)?;
            }
            return Ok(builder.finish());
        }
        let mut bytes = [0; 4];
        read_exact(io, &mut bytes).await.map_err(FramingError::Io)?;
        marker = u32::from_be_bytes(bytes);
    }
}

/// Fixed stack batch, cached iterator and exact offsets. Bulk is never copied.
pub async fn send<IO: AsyncWrite>(io: &mut IO, plan: &SendPlan) -> std::io::Result<()> {
    let mut prefix = [0_u8; 1024];
    let len = u32::try_from(plan.len()).map_err(|_overflow| ErrorKind::InvalidInput)?;
    if len > 0x7fff_ffff {
        return Err(ErrorKind::InvalidInput.into());
    }
    prefix[..4].copy_from_slice(&(len | 0x8000_0000).to_be_bytes());
    let metadata = plan.leading_metadata();
    let copied = metadata.len().min(prefix.len() - 4);
    prefix[4..4 + copied].copy_from_slice(&metadata[..copied]);
    let mut chunks = plan.chunks();
    let mut next = chunks.next();
    if copied != 0 {
        next = next.map(|chunk| &chunk[copied..]);
        if next.is_some_and(<[u8]>::is_empty) {
            next = chunks.next();
        }
    }
    let mut current = Some(&prefix[..4 + copied]);
    let mut offset = 0;
    loop {
        let mut batch = [IoSlice::new(&[]); 16];
        let mut count = 0;
        let mut lookahead = chunks.clone();
        if let Some(bytes) = current {
            batch[count] = IoSlice::new(&bytes[offset..]);
            count += 1;
        }
        if let Some(bytes) = next {
            batch[count] = IoSlice::new(bytes);
            count += 1;
        }
        while count < batch.len() {
            let Some(bytes) = lookahead.next() else {
                break;
            };
            batch[count] = IoSlice::new(bytes);
            count += 1;
        }
        if count == 0 {
            return Ok(());
        }
        let offered: usize = batch[..count].iter().map(|b| b.len()).sum();
        let mut written = match io.async_write_vectored(&batch[..count]).await {
            Ok(0) => return Err(ErrorKind::WriteZero.into()),
            Ok(n) if n <= offered => n,
            Ok(_) => return Err(ErrorKind::InvalidData.into()),
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        while let Some(bytes) = current {
            let available = bytes.len() - offset;
            if written < available {
                offset += written;
                break;
            }
            written -= available;
            current = next;
            next = chunks.next();
            offset = 0;
            if written == 0 {
                break;
            }
        }
    }
}

#[cfg(test)]
thread_local! {
    static RECEIVE_ALLOCATIONS: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
}
#[cfg(test)]
#[path = "async_io_tests.rs"]
mod tests;
