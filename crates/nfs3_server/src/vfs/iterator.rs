pub use nfs3_types::nfs3::{entry3, entryplus3, nfsstat3};

/// Represents the result of `next()` in [`ReadDirIterator`] and [`ReadDirPlusIterator`].
pub enum NextResult<T> {
    /// The next entry in the directory. It's either [`entry3`] or [`entryplus3`].
    Ok(T),
    /// The end of the directory has been reached. It is not an error.
    Eof,
    /// An error occurred while reading the directory.
    Err(nfsstat3),
}

/// Iterator for [`NfsFileSystem::readdir`](super::NfsFileSystem::readdir)
///
/// All [`ReadDirPlusIterator`] implementations automatically implement `ReadDirIterator`.
/// In general, there is no need to implement `ReadDirIterator` directly.
pub trait ReadDirIterator: Send + Sync {
    /// Returns the next entry in the directory.
    fn next(&mut self) -> impl Future<Output = NextResult<entry3<'static>>> + Send;
}

/// Iterator for [`NfsFileSystem::readdirplus`](super::NfsFileSystem::readdirplus)
pub trait ReadDirPlusIterator: Send + Sync {
    /// Returns the next entry in the directory.
    ///
    /// If `entryplus3::name_handle` field is `None`, it will be filled automatically using
    /// [`NfsFileSystem::id_to_fh`](super::NfsFileSystem::id_to_fh).
    fn next(&mut self) -> impl Future<Output = NextResult<entryplus3<'static>>> + Send;
}

impl<T: ReadDirPlusIterator> ReadDirIterator for T {
    async fn next(&mut self) -> NextResult<entry3<'static>> {
        match ReadDirPlusIterator::next(self).await {
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
