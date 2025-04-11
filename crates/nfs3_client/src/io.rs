//! Asynchronous I/O traits for reading and writing bytes.

/// Trait to read bytes asynchronously.
pub trait AsyncRead: Send {
    /// Read bytes from the stream into the provided buffer.
    fn async_read(&mut self, buf: &mut [u8])
    -> impl Future<Output = std::io::Result<usize>> + Send;

    /// Read exactly the number of bytes into the buffer.
    fn async_read_exact(
        &mut self,
        buf: &mut [u8],
    ) -> impl Future<Output = std::io::Result<()>> + Send {
        async move {
            let mut buf = buf;
            while !buf.is_empty() {
                let n = self.async_read(buf).await?;
                buf = &mut buf[n..];
            }
            Ok(())
        }
    }
}

/// Trait to write bytes asynchronously.
pub trait AsyncWrite: Send {
    /// Write bytes to the stream from the provided buffer.
    fn async_write(&mut self, buf: &[u8]) -> impl Future<Output = std::io::Result<usize>> + Send;

    /// Write all bytes to the stream from the provided buffer.
    fn async_write_all(&mut self, buf: &[u8]) -> impl Future<Output = std::io::Result<()>> + Send {
        async move {
            let mut buf = buf;
            while !buf.is_empty() {
                let n = self.async_write(buf).await?;
                buf = &buf[n..];
            }
            Ok(())
        }
    }
}
