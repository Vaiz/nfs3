pub use nfs3_types::nfs3::{entry3, entryplus3, nfsstat3};

/// Represents the result of `next()` in [`ReadDirIterator`] and [`ReadDirPlusIterator`].
pub enum NextResult<T> {
    /// The next entry in the directory. It's either [`entry3`] or [`entryplus3`].
    Ok(T),
    Eof,
    Err(nfsstat3),
}

/// Iterator for [`NFSFileSystem::readdir`](super::NFSFileSystem::readdir)
///
/// All [`ReadDirPlusIterator`] implementations automatically implement `ReadDirIterator`.
/// In general, there is no need to implement `ReadDirIterator` directly.
#[async_trait::async_trait]
pub trait ReadDirIterator: Send + Sync {
    /// Returns the next entry in the directory.
    async fn next(&mut self) -> NextResult<entry3<'static>>;
}

/// Iterator for [`NFSFileSystem::readdirplus`](super::NFSFileSystem::readdirplus)
#[async_trait::async_trait]
pub trait ReadDirPlusIterator: Send + Sync {
    /// Returns the next entry in the directory.
    ///
    /// If `entryplus3::name_handle` field is `None`, it will be filled automatically using
    /// [`NFSFileSystem::id_to_fh`](super::NFSFileSystem::id_to_fh).
    async fn next(&mut self) -> NextResult<entryplus3<'static>>;
}

#[async_trait::async_trait]
impl<T: ReadDirPlusIterator> ReadDirIterator for T {
    async fn next(&mut self) -> NextResult<entry3<'static>> {
        match self.next().await {
            NextResult::Ok(entry) => NextResult::Ok(entry3 {
                fileid: entry.fileid,
                name: entry.name,
                cookie: entry.cookie,
            }),
            NextResult::Eof => NextResult::Eof,
            NextResult::Err(err) => NextResult::Err(err),
        }
    }
}
