pub use nfs3_types::nfs3::{
    cookie3, entry3, entryplus3, fileid3, filename3, nfsstat3, post_op_attr, post_op_fh3,
};

use crate::vfs::FileHandle;
use crate::vfs::handle::FileHandleConverter;

/// Same as `entry3`
pub type DirEntry = entry3<'static>;

/// Represents `entryplus3` with Handle instead of `nfs3_fh`
#[derive(Debug, Clone)]
pub struct DirEntryPlus<H: FileHandle> {
    pub fileid: fileid3,
    pub name: filename3<'static>,
    pub cookie: cookie3,
    pub name_attributes: post_op_attr,
    pub handle: Option<H>,
}

impl<H: FileHandle> DirEntryPlus<H> {
    pub(crate) fn into_entry(self, converter: &FileHandleConverter) -> entryplus3<'static> {
        entryplus3 {
            fileid: self.fileid,
            name: self.name,
            cookie: self.cookie,
            name_attributes: self.name_attributes,
            name_handle: self.handle.map_or(post_op_fh3::None, |h| {
                post_op_fh3::Some(converter.fh_to_nfs(&h))
            }),
        }
    }
}

/// Represents the result of `next()` in [`ReadDirIterator`] and [`ReadDirPlusIterator`].
pub enum NextResult<T> {
    /// The next entry in the directory. It's either [`DirEntry`] or [`DirEntryPlus`].
    Ok(T),
    /// The end of the directory has been reached. It is not an error.
    Eof,
    /// An error occurred while reading the directory.
    Err(nfsstat3),
}

/// Iterator for [`NfsReadFileSystem::readdir`](super::NfsReadFileSystem::readdir)
///
/// All [`ReadDirPlusIterator`] implementations automatically implement `ReadDirIterator`.
/// In general, there is no need to implement `ReadDirIterator` directly.
pub trait ReadDirIterator: Send + Sync {
    /// Returns the next entry in the directory.
    fn next(&mut self) -> impl Future<Output = NextResult<DirEntry>> + Send;
}

/// Iterator for [`NfsReadFileSystem::readdirplus`](super::NfsReadFileSystem::readdirplus)
pub trait ReadDirPlusIterator<H: FileHandle>: Send + Sync {
    /// Returns the next entry in the directory.
    fn next(&mut self) -> impl Future<Output = NextResult<DirEntryPlus<H>>> + Send;
}

/// Wrapper that transforms a [`ReadDirPlusIterator`] into a [`ReadDirIterator`]
/// by stripping the handle and attributes from directory entries.
pub struct ReadDirPlusToReadDirAdapter<H: FileHandle, I: ReadDirPlusIterator<H>> {
    inner: I,
    _phantom: std::marker::PhantomData<H>,
}

impl<H: FileHandle, I: ReadDirPlusIterator<H>> ReadDirPlusToReadDirAdapter<H, I> {
    /// Create a new adapter that wraps a [`ReadDirPlusIterator`]
    pub const fn new(inner: I) -> Self {
        Self {
            inner,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<I: ReadDirPlusIterator<H>, H: FileHandle> ReadDirIterator
    for ReadDirPlusToReadDirAdapter<H, I>
{
    async fn next(&mut self) -> NextResult<DirEntry> {
        match self.inner.next().await {
            NextResult::Ok(plus_entry) => NextResult::Ok(DirEntry {
                fileid: plus_entry.fileid,
                name: plus_entry.name,
                cookie: plus_entry.cookie,
            }),
            NextResult::Eof => NextResult::Eof,
            NextResult::Err(err) => NextResult::Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use nfs3_types::nfs3::{filename3, post_op_attr};

    use super::*;
    use crate::vfs::FileHandleU64;

    // Mock iterator for testing
    struct MockReadDirPlusIterator {
        entries: Vec<DirEntryPlus<FileHandleU64>>,
        index: usize,
    }

    impl MockReadDirPlusIterator {
        fn new(entries: Vec<DirEntryPlus<FileHandleU64>>) -> Self {
            Self { entries, index: 0 }
        }
    }

    impl ReadDirPlusIterator<FileHandleU64> for MockReadDirPlusIterator {
        async fn next(&mut self) -> NextResult<DirEntryPlus<FileHandleU64>> {
            if self.index >= self.entries.len() {
                NextResult::Eof
            } else {
                let entry = self.entries[self.index].clone();
                self.index += 1;
                NextResult::Ok(entry)
            }
        }
    }

    #[tokio::test]
    async fn test_readdir_plus_to_readdir_adapter() {
        let plus_entries = vec![
            DirEntryPlus {
                fileid: 1,
                name: filename3::from(b"file1.txt".to_vec()),
                cookie: 100,
                name_attributes: post_op_attr::None,
                handle: Some(42u64.into()),
            },
            DirEntryPlus {
                fileid: 2,
                name: filename3::from(b"file2.txt".to_vec()),
                cookie: 200,
                name_attributes: post_op_attr::None,
                handle: None,
            },
        ];

        let plus_iter = MockReadDirPlusIterator::new(plus_entries);
        let mut readdir_iter = ReadDirPlusToReadDirAdapter::new(plus_iter);

        // Test first entry
        match readdir_iter.next().await {
            NextResult::Ok(entry) => {
                assert_eq!(entry.fileid, 1);
                assert_eq!(&entry.name.0, &filename3::from(b"file1.txt".to_vec()).0);
                assert_eq!(entry.cookie, 100);
            }
            _ => panic!("Expected Ok result"),
        }

        // Test second entry
        match readdir_iter.next().await {
            NextResult::Ok(entry) => {
                assert_eq!(entry.fileid, 2);
                assert_eq!(&entry.name.0, &filename3::from(b"file2.txt".to_vec()).0);
                assert_eq!(entry.cookie, 200);
            }
            _ => panic!("Expected Ok result"),
        }

        // Test EOF
        match readdir_iter.next().await {
            NextResult::Eof => {}
            _ => panic!("Expected EOF"),
        }
    }
}
