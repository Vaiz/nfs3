//! B0.5 bounded diagnostics, not timing or released-peer interoperability.
//! Run each named case in a separate process with an external watchdog.
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use anyhow::{Result, ensure};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

mod fixture;
mod scenarios;

pub use scenarios::{cases, run};

pub const WIRE_BOUND: usize = 1024 * 1024 + 4096;

/// Controls only outer socket reads. Internal server duplex reads may coalesce.
/// Initialized sub-buffers avoid unsafe code, allocation and copying per poll.
pub struct Chunks<IO> {
    pub io: IO,
    schedule: &'static [usize],
    step: usize,
}

impl<IO> Chunks<IO> {
    pub fn new(io: IO, schedule: &'static [usize]) -> Self {
        assert!(!schedule.is_empty() && schedule.iter().all(|n| *n > 0));
        Self {
            io,
            schedule,
            step: 0,
        }
    }
}

impl<IO: AsyncRead + Unpin> AsyncRead for Chunks<IO> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let n = buf
            .remaining()
            .min(self.schedule[self.step % self.schedule.len()]);
        if n == 0 {
            return Poll::Ready(Ok(()));
        }
        let mut sub = ReadBuf::new(buf.initialize_unfilled_to(n));
        let result = Pin::new(&mut self.io).poll_read(cx, &mut sub);
        let filled = sub.filled().len();
        if matches!(result, Poll::Ready(Ok(()))) && filled > 0 {
            self.step = self.step.wrapping_add(1);
            buf.advance(filled);
        }
        result
    }
}

impl<IO: AsyncWrite + Unpin> AsyncWrite for Chunks<IO> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        b: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.io).poll_write(cx, b)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_shutdown(cx)
    }
}

/// Explicit sender record boundaries, independent of receiver chunk schedules.
pub fn fragments(body: &[u8], cuts: &[usize]) -> Result<Vec<u8>> {
    ensure!(
        body.len() <= WIRE_BOUND && cuts.len() <= 8,
        "fragment bounds"
    );
    ensure!(
        body.len() + 4 * (cuts.len() + 1) <= WIRE_BOUND,
        "framed wire bound"
    );
    ensure!(cuts.windows(2).all(|p| p[0] <= p[1]), "unordered cuts");
    ensure!(cuts.iter().all(|n| *n <= body.len()), "cut outside body");
    let mut out = Vec::with_capacity(body.len() + 4 * (cuts.len() + 1));
    let mut start = 0;
    for (i, end) in cuts.iter().copied().chain([body.len()]).enumerate() {
        let marker = (end - start) as u32 | if i == cuts.len() { 0x8000_0000 } else { 0 };
        out.extend_from_slice(&marker.to_be_bytes());
        out.extend_from_slice(&body[start..end]);
        start = end;
    }
    Ok(out)
}

/// Independent bounded record model; returns logical bytes and marker bytes.
pub fn reassemble(wire: &[u8]) -> Result<(Vec<u8>, usize)> {
    ensure!(wire.len() <= WIRE_BOUND, "wire bound");
    let mut body = Vec::new();
    let mut offset = 0;
    let mut markers = 0;
    loop {
        ensure!(
            markers < 9 && wire.len().saturating_sub(offset) >= 4,
            "incomplete marker"
        );
        let marker = u32::from_be_bytes(wire[offset..offset + 4].try_into()?);
        offset += 4;
        markers += 1;
        let n = (marker & 0x7fff_ffff) as usize;
        ensure!(n <= wire.len() - offset, "incomplete body");
        body.extend_from_slice(&wire[offset..offset + n]);
        offset += n;
        if marker & 0x8000_0000 != 0 {
            ensure!(
                offset == wire.len() && !body.is_empty(),
                "empty/trailing record"
            );
            return Ok((body, markers * 4));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::task::Waker;

    use super::*;

    #[test]
    fn patterned_sink_detects_equal_length_reordering_without_uniform_hook() {
        use nfs3_client::nfs3_types::nfs3::stable_how;
        use nfs3_server::vfs::{FileHandleU64, NfsFileSystem};
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(async {
                let fs = fixture::Fixture::new(4096, 0, false);
                let mut data: Vec<_> = (0..4096).map(fixture::pattern).collect();
                fs.write(&FileHandleU64::new(2), 0, &data, stable_how::FILE_SYNC)
                    .await
                    .unwrap();
                data.swap(1, 2);
                assert!(
                    fs.write(&FileHandleU64::new(2), 0, &data, stable_how::FILE_SYNC)
                        .await
                        .is_err()
                );
                assert_eq!(fs.written.load(std::sync::atomic::Ordering::SeqCst), 4096);
            });
    }

    #[test]
    fn independent_fragment_model_includes_empty_edges_and_rejects_truncation() {
        let body: Vec<_> = (1..=40).collect();
        for split in 0..=body.len() {
            let wire = fragments(&body, &[split]).unwrap();
            assert_eq!(reassemble(&wire).unwrap(), (body.clone(), 8));
            assert!(reassemble(&wire[..wire.len() - 1]).is_err());
        }
        assert!(reassemble(&fragments(&[], &[0]).unwrap()).is_err());
        assert!(fragments(&body, &[41]).is_err());
        assert!(fragments(&body, &[3, 2]).is_err());
        assert!(reassemble(&[0x80, 0, 0, 9, 1]).is_err());
        assert!(fragments(&body, &[0; 9]).is_err());
        assert!(fragments(&vec![1; WIRE_BOUND], &[]).is_err());
        assert!(reassemble(&vec![1; WIRE_BOUND + 1]).is_err());
    }

    struct Script(usize);
    impl AsyncRead for Script {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
            b: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            self.0 += 1;
            match self.0 {
                1 => Poll::Pending,
                2 => Poll::Ready(Err(io::ErrorKind::Interrupted.into())),
                3 => Poll::Ready(Ok(())),
                _ => {
                    b.put_slice(&[7]);
                    Poll::Ready(Ok(()))
                }
            }
        }
    }
    #[test]
    fn chunks_advance_only_on_positive_ready_success_without_touching_prefix() {
        let mut cx = Context::from_waker(Waker::noop());
        let mut chunks = Chunks::new(Script(0), &[1, 3]);
        let mut storage = [9; 8];
        let mut b = ReadBuf::new(&mut storage);
        b.advance(2);
        for _ in 0..3 {
            let _ = Pin::new(&mut chunks).poll_read(&mut cx, &mut b);
            assert_eq!(chunks.step, 0);
            assert_eq!(b.filled(), [9, 9]);
        }
        assert!(Pin::new(&mut chunks).poll_read(&mut cx, &mut b).is_ready());
        assert_eq!(chunks.step, 1);
        assert_eq!(b.filled(), [9, 9, 7]);
    }
}
