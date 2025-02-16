use tokio::io::{AsyncRead as TokioAsyncRead, AsyncWrite as TokioAsyncWrite};

use super::{AsyncRead, AsyncWrite};

pub struct TokioIo<T>(T);

impl<T> TokioIo<T> {
    pub fn new(inner: T) -> Self {
        Self(inner)
    }
}

#[async_trait::async_trait(?Send)]
impl<T> AsyncRead for TokioIo<T>
where
    T: TokioAsyncRead + Unpin,
{
    async fn async_read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        tokio::io::AsyncReadExt::read(&mut self.0, buf).await
    }
}

#[async_trait::async_trait(?Send)]
impl<T> AsyncWrite for TokioIo<T>
where
    T: TokioAsyncWrite + Unpin,
{
    async fn async_write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        tokio::io::AsyncWriteExt::write(&mut self.0, buf).await
    }
}
