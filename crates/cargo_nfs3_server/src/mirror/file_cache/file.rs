use std::path::PathBuf;
use std::sync::Arc;

use nfs3_server::nfs3_types::nfs3::nfsstat3;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};
use tokio::sync::RwLock;
use tracing::warn;

use crate::mirror::map_io_error;

#[derive(Debug)]
pub enum FlushMarker {
    FlushAndSync,
    DropWithoutFlush,
}

/// A file handle with its access mode. The file object is contained within the enum variant.
#[derive(Debug)]
pub enum FileHandle {
    /// File is open for reading only.
    Read(File),
    /// File is open for both reading and writing.
    ReadWrite(File, FlushMarker),
    /// Drop
    DropStub,
}

impl FileHandle {
    /// Returns whether this handle is in read-write mode.
    const fn is_read_write(&self) -> bool {
        matches!(self, Self::ReadWrite(_, _))
    }

    /// Gets a mutable reference to the underlying file.
    fn file_mut(&mut self) -> &mut File {
        match self {
            Self::Read(f) | Self::ReadWrite(f, _) => f,
            Self::DropStub => unreachable!("invalid file handle state"),
        }
    }

    async fn flush(&mut self) -> std::io::Result<()> {
        let Self::ReadWrite(file, _) = self else {
            return Ok(());
        };

        file.flush().await?;
        file.sync_all().await
    }

    fn dont_flush_on_drop(&mut self) {
        if let Self::ReadWrite(_, flush_marker) = self {
            *flush_marker = FlushMarker::DropWithoutFlush;
        }
    }
}

/// Spawn a task to flush and sync the file asynchronously on drop.
impl Drop for FileHandle {
    fn drop(&mut self) {
        let mut this = std::mem::replace(self, FileHandle::DropStub);
        if let Self::ReadWrite(_, FlushMarker::FlushAndSync) = this {
            _ = tokio::spawn(async move {
                if let Err(e) = this.flush().await {
                    warn!("failed to flush file on drop: {e}");
                }
            });
        }
    }
}

/// A cached file handle with its current access mode.
#[derive(Debug)]
pub struct CachedFile {
    path: PathBuf,
    handle: RwLock<FileHandle>,
}

impl CachedFile {
    /// Creates a new cached file opened in read mode.
    pub async fn open_read(path: PathBuf) -> std::io::Result<Arc<Self>> {
        let file = File::open(&path).await?;
        Ok(Arc::new(Self {
            path,
            handle: RwLock::new(FileHandle::Read(file)),
        }))
    }

    /// Creates a new cached file opened in read-write mode.
    pub async fn open_read_write(path: PathBuf) -> std::io::Result<Arc<Self>> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .await?;
        Ok(Arc::new(Self {
            path,
            handle: RwLock::new(FileHandle::ReadWrite(file, FlushMarker::FlushAndSync)),
        }))
    }

    /// Returns whether this file is in read-write mode.
    pub async fn is_read_write(&self) -> bool {
        self.handle.read().await.is_read_write()
    }

    /// Ensures the file is open for reading and performs a read operation.
    ///
    /// Returns the data read and whether EOF was reached.
    pub async fn read(&self, offset: u64, count: u32) -> Result<(Vec<u8>, bool), nfsstat3> {
        async {
            let mut handle_guard = self.handle.write().await;
            let file = handle_guard.file_mut();

            let len = file.metadata().await?.len();
            if offset >= len || count == 0 {
                return Ok((Vec::new(), u64::from(count) >= len));
            }

            let count = u64::from(count).min(len - offset);
            file.seek(SeekFrom::Start(offset)).await?;

            let mut buf = vec![0; usize::try_from(count).unwrap_or(0)];
            file.read_exact(&mut buf).await?;

            Ok((buf, offset + count >= len))
        }
        .await
        .map_err(|e: std::io::Error| {
            warn!("read failed: {} - {}", self.path.display(), e);
            map_io_error(e)
        })
    }

    /// Ensures the file is open for writing and performs a write operation.
    ///
    /// If the file is currently open for read only, it will be reopened
    /// with read-write access.
    pub async fn write(&self, offset: u64, data: &[u8]) -> Result<(), nfsstat3> {
        async {
            // Check if we need to upgrade to read-write mode
            if !self.handle.read().await.is_read_write() {
                self.upgrade_to_read_write().await?;
            }

            let mut handle_guard = self.handle.write().await;
            let file = handle_guard.file_mut();

            file.seek(SeekFrom::Start(offset)).await?;
            file.write_all(data).await?;
            file.flush().await?;

            Ok(())
        }
        .await
        .map_err(|e: std::io::Error| {
            warn!("write failed: {} - {}", self.path.display(), e);
            map_io_error(e)
        })
    }

    /// Upgrades the file from read mode to read-write mode.
    pub async fn upgrade_to_read_write(&self) -> std::io::Result<()> {
        let mut handle_guard = self.handle.write().await;

        // Check if already in read-write mode
        if handle_guard.is_read_write() {
            return Ok(());
        }

        // Reopen with read-write access
        let new_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.path)
            .await?;

        *handle_guard = FileHandle::ReadWrite(new_file, FlushMarker::FlushAndSync);

        Ok(())
    }

    pub async fn dont_flush_on_drop(&self) {
        self.handle.write().await.dont_flush_on_drop();
    }

    pub async fn flush(&self) -> std::io::Result<()> {
        self.handle.write().await.flush().await
    }
}
