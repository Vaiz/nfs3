//! Asynchronous I/O traits for reading and writing bytes.

/// Trait to read bytes asynchronously.
#[async_trait::async_trait(?Send)]
pub trait AsyncRead {
    /// Read bytes from the stream into the provided buffer.
    async fn async_read(&mut self, buf: &mut [u8]) -> std::io::Result<usize>;

    /// Read exactly the number of bytes into the buffer.
    async fn async_read_exact(&mut self, buf: &mut [u8]) -> std::io::Result<()> {
        let mut buf = buf;
        while !buf.is_empty() {
            let n = self.async_read(buf).await?;
            buf = &mut buf[n..];
        }
        Ok(())
    }
}

/// Trait to write bytes asynchronously.
#[async_trait::async_trait(?Send)]
pub trait AsyncWrite {
    /// Write bytes to the stream from the provided buffer.
    async fn async_write(&mut self, buf: &[u8]) -> std::io::Result<usize>;

    /// Write all bytes to the stream from the provided buffer.
    async fn async_write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        let mut buf = buf;
        while !buf.is_empty() {
            let n = self.async_write(buf).await?;
            buf = &buf[n..];
        }
        Ok(())
    }
}
