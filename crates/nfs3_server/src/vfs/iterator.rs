pub use nfs3_types::nfs3::{entry3, entryplus3, nfsstat3};

#[async_trait::async_trait]
pub trait ReadDirIterator: Send + Sync {
    async fn next(&mut self) -> Result<entry3<'static>, nfsstat3>;
    fn eof(&self) -> bool;
}

#[async_trait::async_trait]
pub trait ReadDirPlusIterator: Send + Sync {
    async fn next(&mut self) -> Result<entryplus3<'static>, nfsstat3>;
    fn eof(&self) -> bool;
}

#[async_trait::async_trait]
impl<T: ReadDirPlusIterator> ReadDirIterator for T {
    async fn next(&mut self) -> Result<entry3<'static>, nfsstat3> {
        self.next().await.map(|entry| entry3 {
            fileid: entry.fileid,
            name: entry.name,
            cookie: entry.cookie,
        })
    }
    fn eof(&self) -> bool {
        self.eof()
    }
}
