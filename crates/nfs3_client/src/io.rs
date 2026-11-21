//! Asynchronous I/O traits for reading and writing bytes.

use std::io::{self, IoSlice};

use bytes::BytesMut;

/// Trait to read bytes asynchronously.
pub trait AsyncRead: Send {
    /// Read bytes from the stream into the provided buffer.
    fn async_read(&mut self, buf: &mut [u8]) -> impl Future<Output = io::Result<usize>> + Send;

    /// Read exactly the number of bytes into the buffer.
    ///
    /// Fails with [`io::ErrorKind::UnexpectedEof`] if the stream ends first.
    fn async_read_exact(&mut self, buf: &mut [u8]) -> impl Future<Output = io::Result<()>> + Send {
        async move {
            let mut buf = buf;
            while !buf.is_empty() {
                let n = self.async_read(buf).await?;
                if n == 0 {
                    return Err(io::ErrorKind::UnexpectedEof.into());
                }
                buf = &mut buf[n..];
            }
            Ok(())
        }
    }

    /// Read exactly `len` bytes, appending them to `buf`.
    ///
    /// Fails with [`io::ErrorKind::UnexpectedEof`] if the stream ends first; on
    /// error the contents appended to `buf` are unspecified.
    /// The default implementation zero-fills `len` bytes of `buf` once and calls
    /// [`async_read_exact`](Self::async_read_exact); override it to read into
    /// spare capacity directly.
    fn async_read_exact_buf(
        &mut self,
        buf: &mut BytesMut,
        len: usize,
    ) -> impl Future<Output = io::Result<()>> + Send {
        async move {
            let start = buf.len();
            buf.resize(start + len, 0);
            self.async_read_exact(&mut buf[start..]).await
        }
    }
}

/// Trait to write bytes asynchronously.
pub trait AsyncWrite: Send {
    /// Write bytes to the stream from the provided buffer.
    fn async_write(&mut self, buf: &[u8]) -> impl Future<Output = io::Result<usize>> + Send;

    /// Write all bytes to the stream from the provided buffer.
    fn async_write_all(&mut self, buf: &[u8]) -> impl Future<Output = io::Result<()>> + Send {
        async move {
            let mut buf = buf;
            while !buf.is_empty() {
                let n = self.async_write(buf).await?;
                if n == 0 {
                    return Err(io::ErrorKind::WriteZero.into());
                }
                buf = &buf[n..];
            }
            Ok(())
        }
    }

    /// Write bytes from several buffers, in order.
    ///
    /// The default implementation writes the first non-empty buffer.
    fn async_write_vectored(
        &mut self,
        bufs: &[IoSlice<'_>],
    ) -> impl Future<Output = io::Result<usize>> + Send {
        let buf = bufs
            .iter()
            .find(|buf| !buf.is_empty())
            .map_or(&[][..], |buf| &**buf);
        self.async_write(buf)
    }

    /// Write all bytes from several buffers, in order.
    fn async_write_all_vectored(
        &mut self,
        mut bufs: &mut [IoSlice<'_>],
    ) -> impl Future<Output = io::Result<()>> + Send {
        async move {
            IoSlice::advance_slices(&mut bufs, 0);
            while !bufs.is_empty() {
                let n = self.async_write_vectored(bufs).await?;
                if n == 0 {
                    return Err(io::ErrorKind::WriteZero.into());
                }
                IoSlice::advance_slices(&mut bufs, n);
            }
            Ok(())
        }
    }
}
