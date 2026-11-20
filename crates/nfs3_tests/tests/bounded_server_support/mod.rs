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
        !self.state.reject_auth.load(Ordering::Acquire)
            && (auth.is_none() || auth.is_some_and(|a| a.uid == 42))
    }
    async fn lookup(
        &self,
        id: &Self::Handle,
        name: &filename3<'_>,
    ) -> Result<Self::Handle, nfsstat3> {
        self.fs.lookup(id, name).await
    }
    async fn getattr(&self, id: &Self::Handle) -> Result<fattr3, nfsstat3> {
        self.fs.getattr(id).await
    }
    async fn read(&self, _: &Self::Handle, _: u64, _: u32) -> Result<(Vec<u8>, bool), nfsstat3> {
        panic!("legacy read");
    }
    async fn readdirplus(
        &self,
        id: &Self::Handle,
        cookie: u64,
    ) -> Result<impl ReadDirPlusIterator<Self::Handle>, nfsstat3> {
        self.fs.readdirplus(id, cookie).await
    }
    async fn readlink(&self, id: &Self::Handle) -> Result<nfspath3<'_>, nfsstat3> {
        self.fs.readlink(id).await
    }
}

impl NfsOwnedRead for Controlled {
    async fn read_owned(
        &self,
        mut request: OwnedRead<Self::Handle>,
    ) -> Result<OwnedReadResult, nfsstat3> {
        let pointer = request.buffer().as_ptr() as usize;
        let result = if let Some(path) = self.state.file.get() {
            nfs3_server::fs_util::read_owned_file(request, path.clone()).await?
        } else {
            self.fs.read_owned(request).await?
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
impl NfsOwnedIo for Controlled {
    async fn write_owned(
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
            {
                let ready = state.gate.0.lock().unwrap();
                drop(state.gate.1.wait_while(ready, |ready| !*ready).unwrap());
            }
            let result = tokio::runtime::Handle::current().block_on(async {
                let mut result = (fs.getattr(&request.handle).await?, stable_how::FILE_SYNC);
                let mut offset = request.offset;
                for chunk in request.data.chunks() {
                    result = fs
                        .write(&request.handle, offset, chunk, request.stable)
                        .await?;
                    offset += chunk.len() as u64;
                }
                Ok(result)
            });
            state.complete.store(true, Ordering::Release);
            drop(request); // private lease survives all of this actual worker
            result
        });
        if self.return_early {
            drop(job);
            Err(nfsstat3::NFS3ERR_IO)
        } else {
            job.await.unwrap()
        }
    }
}
impl NfsFileSystem for Controlled {
    async fn setattr(&self, id: &Self::Handle, a: sattr3) -> Result<fattr3, nfsstat3> {
        self.fs.setattr(id, a).await
    }
    async fn write(
        &self,
        _: &Self::Handle,
        _: u64,
        _: &[u8],
        _: stable_how,
    ) -> Result<(fattr3, stable_how), nfsstat3> {
        panic!("legacy write");
    }
    async fn create(
        &self,
        id: &Self::Handle,
        n: &filename3<'_>,
        a: sattr3,
    ) -> Result<(Self::Handle, fattr3), nfsstat3> {
        self.fs.create(id, n, a).await
    }
    async fn create_exclusive(
        &self,
        id: &Self::Handle,
        n: &filename3<'_>,
        v: createverf3,
    ) -> Result<Self::Handle, nfsstat3> {
        self.fs.create_exclusive(id, n, v).await
    }
    async fn mkdir(
        &self,
        id: &Self::Handle,
        n: &filename3<'_>,
    ) -> Result<(Self::Handle, fattr3), nfsstat3> {
        self.fs.mkdir(id, n).await
    }
    async fn remove(&self, id: &Self::Handle, n: &filename3<'_>) -> Result<(), nfsstat3> {
        self.fs.remove(id, n).await
    }
    async fn rename<'a>(
        &self,
        id: &Self::Handle,
        n: &filename3<'a>,
        to: &Self::Handle,
        t: &filename3<'a>,
    ) -> Result<(), nfsstat3> {
        self.fs.rename(id, n, to, t).await
    }
    async fn symlink<'a>(
        &self,
        id: &Self::Handle,
        n: &filename3<'a>,
        p: &nfspath3<'a>,
        a: &sattr3,
    ) -> Result<(Self::Handle, fattr3), nfsstat3> {
        self.fs.symlink(id, n, p, a).await
    }
    async fn commit(&self, id: &Self::Handle, o: u64, c: u32) -> Result<(), nfsstat3> {
        if let Some(path) = self.state.file.get() {
            nfs3_server::fs_util::commit_file(path.clone()).await
        } else {
            self.fs.commit(id, o, c).await
        }
    }
}
