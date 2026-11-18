use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use nfs3_client::nfs3_types::nfs3::*;
use nfs3_server::vfs::{FileHandleU64, NfsFileSystem, NfsReadFileSystem, ReadDirPlusIterator};

use crate::perf::{ListFs, WriteFs};

/// Nonzero, position-dependent bytes. No storage copy/allocation in the sink.
pub fn pattern(position: usize) -> u8 {
    ((position.wrapping_mul(37) ^ (position >> 8) ^ (position >> 16)) % 255 + 1) as u8
}

pub struct Fixture {
    pub size: usize,
    pub nonzero_mtime: bool,
    pub written: Arc<AtomicUsize>,
    list: ListFs,
    attrs: WriteFs,
}

impl Fixture {
    pub fn new(size: usize, entries: usize, nonzero_mtime: bool) -> Self {
        assert!(size <= 1024 * 1024 && entries <= 100);
        Self {
            size,
            nonzero_mtime,
            written: Arc::default(),
            list: ListFs::new(entries),
            attrs: WriteFs::new(size as u64),
        }
    }
}

impl NfsReadFileSystem for Fixture {
    type Handle = FileHandleU64;
    fn root_dir(&self) -> Self::Handle {
        self.list.root_dir()
    }
    async fn lookup(
        &self,
        dir: &Self::Handle,
        name: &filename3<'_>,
    ) -> Result<Self::Handle, nfsstat3> {
        self.list.lookup(dir, name).await
    }
    async fn getattr(&self, id: &Self::Handle) -> Result<fattr3, nfsstat3> {
        let mut attr = self.attrs.getattr(id).await?;
        if *id == self.root_dir() && self.nonzero_mtime {
            attr.mtime = nfstime3 {
                seconds: 17,
                nseconds: 29,
            };
        }
        Ok(attr)
    }
    async fn read(
        &self,
        _: &Self::Handle,
        offset: u64,
        count: u32,
    ) -> Result<(Vec<u8>, bool), nfsstat3> {
        let start = usize::try_from(offset).unwrap_or(usize::MAX).min(self.size);
        let end = start.saturating_add(count as usize).min(self.size);
        // Generated owned output, not a hidden backend-to-output storage copy.
        Ok(((start..end).map(pattern).collect(), end == self.size))
    }
    async fn readdirplus(
        &self,
        dir: &Self::Handle,
        cookie: u64,
    ) -> Result<impl ReadDirPlusIterator<Self::Handle>, nfsstat3> {
        self.list.readdirplus(dir, cookie).await
    }
    async fn readlink(&self, id: &Self::Handle) -> Result<nfspath3<'_>, nfsstat3> {
        self.list.readlink(id).await
    }
}

impl NfsFileSystem for Fixture {
    async fn setattr(&self, id: &Self::Handle, attr: sattr3) -> Result<fattr3, nfsstat3> {
        self.list.setattr(id, attr).await
    }
    async fn write(
        &self,
        _: &Self::Handle,
        offset: u64,
        data: &[u8],
        _: stable_how,
    ) -> Result<(fattr3, stable_how), nfsstat3> {
        let start = usize::try_from(offset).map_err(|_| nfsstat3::NFS3ERR_INVAL)?;
        if start
            .checked_add(data.len())
            .is_none_or(|end| end > self.size)
            || data
                .iter()
                .enumerate()
                .any(|(i, b)| *b != pattern(start + i))
        {
            return Err(nfsstat3::NFS3ERR_INVAL);
        }
        self.written.fetch_add(data.len(), Ordering::SeqCst);
        // Deliberately do not delegate to WriteFs::write (its hook assumes 0xAA).
        Ok((
            self.attrs.getattr(&FileHandleU64::new(2)).await?,
            stable_how::FILE_SYNC,
        ))
    }
    async fn create(
        &self,
        dir: &Self::Handle,
        name: &filename3<'_>,
        attr: sattr3,
    ) -> Result<(Self::Handle, fattr3), nfsstat3> {
        self.list.create(dir, name, attr).await
    }
    async fn create_exclusive(
        &self,
        dir: &Self::Handle,
        name: &filename3<'_>,
        verf: createverf3,
    ) -> Result<Self::Handle, nfsstat3> {
        self.list.create_exclusive(dir, name, verf).await
    }
    async fn mkdir(
        &self,
        dir: &Self::Handle,
        name: &filename3<'_>,
    ) -> Result<(Self::Handle, fattr3), nfsstat3> {
        self.list.mkdir(dir, name).await
    }
    async fn remove(&self, dir: &Self::Handle, name: &filename3<'_>) -> Result<(), nfsstat3> {
        self.list.remove(dir, name).await
    }
    async fn rename<'a>(
        &self,
        from: &Self::Handle,
        name: &filename3<'a>,
        to: &Self::Handle,
        new: &filename3<'a>,
    ) -> Result<(), nfsstat3> {
        self.list.rename(from, name, to, new).await
    }
    async fn symlink<'a>(
        &self,
        dir: &Self::Handle,
        name: &filename3<'a>,
        path: &nfspath3<'a>,
        attr: &sattr3,
    ) -> Result<(Self::Handle, fattr3), nfsstat3> {
        self.list.symlink(dir, name, path, attr).await
    }
    async fn commit(&self, id: &Self::Handle, offset: u64, count: u32) -> Result<(), nfsstat3> {
        self.list.commit(id, offset, count).await
    }
}
