#[cfg(feature = "tokio")]
pub mod tokio;

/// Read bytes asynchronously.
#[async_trait::async_trait(?Send)]
pub trait AsyncRead {
    async fn async_read(&mut self, buf: &mut [u8]) -> std::io::Result<usize>;

    async fn async_read_exact(&mut self, buf: &mut [u8]) -> std::io::Result<()> {
        let mut buf = buf;
        while !buf.is_empty() {
            let n = self.async_read(buf).await?;
            buf = &mut buf[n..];
        }
        Ok(())
    }
}

#[async_trait::async_trait(?Send)]
pub trait AsyncWrite {
    async fn async_write(&mut self, buf: &[u8]) -> std::io::Result<usize>;

    async fn async_write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        let mut buf = buf;
        while !buf.is_empty() {
            let n = self.async_write(buf).await?;
            buf = &buf[n..];
        }
        Ok(())
    }
}
