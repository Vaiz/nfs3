use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use nfs3_server::memfs::MemFs;
use nfs3_server::nfs3_types::nfs3::*;
use nfs3_server::nfs3_types::rpc::auth_unix;
use nfs3_server::vfs::*;

pub struct State {
    pub started: tokio::sync::Semaphore,
    pub complete: AtomicBool,
    pub reject_auth: AtomicBool,
    pub file: std::sync::OnceLock<std::path::PathBuf>,
    gate: (Mutex<bool>, Condvar),
}
impl State {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            started: tokio::sync::Semaphore::new(0),
            complete: AtomicBool::new(false),
            reject_auth: AtomicBool::new(false),
            file: std::sync::OnceLock::new(),
            gate: (Mutex::new(false), Condvar::new()),
        })
    }
    pub fn release(&self) {
        *self.gate.0.lock().unwrap() = true;
        self.gate.1.notify_all();
    }
}

pub struct Controlled {
    pub fs: Arc<MemFs>,
    pub state: Arc<State>,
    pub return_early: bool,
}

pub struct ReleaseOnDrop(pub Arc<State>);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.0.release();
    }
}

impl NfsReadFileSystem for Controlled {
    type Handle = FileHandleU64;
    fn root_dir(&self) -> Self::Handle {
        self.fs.root_dir()
    }
    fn authorize(&self, auth: Option<&auth_unix>) -> bool {
        !self.state.reject_auth.load(Ordering::Acquire) && auth.is_none_or(|a| a.uid == 42)
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
    async fn read(
        &self,
        mut request: OwnedRead<Self::Handle>,
    ) -> Result<OwnedReadResult, nfsstat3> {
        let pointer = request.buffer().as_ptr() as usize;
        let result = if let Some(path) = self.state.file.get() {
            nfs3_server::fs_util::read_owned_file(request, path.clone()).await?
        } else {
            self.fs.read(request).await?
        };
        if !result.payload().is_empty() {
            assert_eq!(
                result.payload().chunks().next().unwrap().as_ptr() as usize,
                pointer
            );
        }
        Ok(result)
    }
}
impl NfsFileSystem for Controlled {
    async fn write(
        &self,
        request: OwnedWrite<Self::Handle>,
    ) -> Result<(fattr3, stable_how), nfsstat3> {
        if let Some(path) = self.state.file.get() {
            let id = request.handle.as_u64();
            let (metadata, stable) =
                nfs3_server::fs_util::write_owned_file(request, path.clone()).await?;
            return Ok((
                nfs3_server::fs_util::metadata_to_fattr3(id, &metadata),
                stable,
            ));
        }
        let state = Arc::clone(&self.state);
        let fs = Arc::clone(&self.fs);
        let job = tokio::task::spawn_blocking(move || {
            state.started.add_permits(1);
            let ready = state.gate.0.lock().unwrap();
            drop(state.gate.1.wait_while(ready, |ready| !*ready).unwrap());
            let result = tokio::runtime::Handle::current().block_on(fs.write(request));
            state.complete.store(true, Ordering::Release);
            result
        });
        if self.return_early {
            drop(job);
            Err(nfsstat3::NFS3ERR_IO)
        } else {
            job.await.unwrap()
        }
    }
    async fn setattr(&self, id: &Self::Handle, setattr: sattr3) -> Result<fattr3, nfsstat3> {
        self.fs.setattr(id, setattr).await
    }
    async fn create(
        &self,
        dirid: &Self::Handle,
        filename: &filename3<'_>,
        attr: sattr3,
    ) -> Result<(Self::Handle, fattr3), nfsstat3> {
        self.fs.create(dirid, filename, attr).await
    }
    async fn create_exclusive(
        &self,
        dirid: &Self::Handle,
        filename: &filename3<'_>,
        createverf: createverf3,
    ) -> Result<Self::Handle, nfsstat3> {
        self.fs.create_exclusive(dirid, filename, createverf).await
    }
    async fn mkdir(
        &self,
        dirid: &Self::Handle,
        dirname: &filename3<'_>,
    ) -> Result<(Self::Handle, fattr3), nfsstat3> {
        self.fs.mkdir(dirid, dirname).await
    }
    async fn remove(&self, dirid: &Self::Handle, filename: &filename3<'_>) -> Result<(), nfsstat3> {
        self.fs.remove(dirid, filename).await
    }
    async fn rename<'a>(
        &self,
        from_dirid: &Self::Handle,
        from_filename: &filename3<'a>,
        to_dirid: &Self::Handle,
        to_filename: &filename3<'a>,
    ) -> Result<(), nfsstat3> {
        self.fs
            .rename(from_dirid, from_filename, to_dirid, to_filename)
            .await
    }
    async fn symlink<'a>(
        &self,
        dirid: &Self::Handle,
        linkname: &filename3<'a>,
        symlink: &nfspath3<'a>,
        attr: &sattr3,
    ) -> Result<(Self::Handle, fattr3), nfsstat3> {
        self.fs.symlink(dirid, linkname, symlink, attr).await
    }
    async fn commit(&self, id: &Self::Handle, offset: u64, count: u32) -> Result<(), nfsstat3> {
        if let Some(path) = self.state.file.get() {
            nfs3_server::fs_util::commit_file(path.clone()).await
        } else {
            self.fs.commit(id, offset, count).await
        }
    }
}
