use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bytes::Bytes;
use nfs3_client::nfs3_types::nfs3::{FSINFO3resok, fattr3, filename3, nfspath3, nfsstat3};
use nfs3_server::memfs::MemFs;
use nfs3_server::vfs::{FileHandleU64, NfsReadFileSystem, ReadDirPlusIterator};

pub(super) struct CustomReadLimit {
    pub(super) fs: MemFs,
    pub(super) rtmax: Result<u32, nfsstat3>,
    pub(super) fsinfo_calls: Arc<AtomicUsize>,
}

impl NfsReadFileSystem for CustomReadLimit {
    type Handle = FileHandleU64;

    fn root_dir(&self) -> Self::Handle {
        self.fs.root_dir()
    }

    async fn lookup(
        &self,
        dirid: &Self::Handle,
        filename: &filename3<'_>,
    ) -> Result<Self::Handle, nfsstat3> {
        self.fs.lookup(dirid, filename).await
    }

    async fn getattr(&self, id: &Self::Handle) -> Result<fattr3, nfsstat3> {
        self.fs.getattr(id).await
    }

    async fn read(
        &self,
        id: &Self::Handle,
        offset: u64,
        count: u32,
    ) -> Result<(Bytes, bool), nfsstat3> {
        self.fs.read(id, offset, count).await
    }

    async fn readdirplus(
        &self,
        dirid: &Self::Handle,
        cookie: u64,
    ) -> Result<impl ReadDirPlusIterator<Self::Handle>, nfsstat3> {
        self.fs.readdirplus(dirid, cookie).await
    }

    async fn readlink(&self, id: &Self::Handle) -> Result<nfspath3<'_>, nfsstat3> {
        self.fs.readlink(id).await
    }

    async fn fsinfo(&self, root_fileid: &Self::Handle) -> Result<FSINFO3resok, nfsstat3> {
        if *root_fileid != self.root_dir() {
            return Err(nfsstat3::NFS3ERR_INVAL);
        }
        self.fsinfo_calls.fetch_add(1, Ordering::Relaxed);
        let mut info = self.fs.fsinfo(root_fileid).await?;
        info.rtmax = self.rtmax?;
        info.rtpref = info.rtmax;
        info.rtmult = 1;
        Ok(info)
    }
}
