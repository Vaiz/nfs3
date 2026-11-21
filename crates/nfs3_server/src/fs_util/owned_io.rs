//! Direct standard-file IO: requests (including their private leases) live
//! inside actual blocking jobs, even if the awaiting future is cancelled.
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use nfs3_types::nfs3::{nfsstat3, stable_how};

use crate::vfs::{OwnedRead, OwnedReadResult, OwnedWrite};

/// Fill supplied READ storage inside a blocking job that owns the complete request.
pub async fn read_owned_file<H: Send + 'static>(
    mut request: OwnedRead<H>,
    path: PathBuf,
) -> Result<OwnedReadResult, nfsstat3> {
    tokio::task::spawn_blocking(move || {
        let mut file = std::fs::File::open(path).map_err(|_io_error| nfsstat3::NFS3ERR_IO)?;
        let length = file
            .metadata()
            .map_err(|_io_error| nfsstat3::NFS3ERR_IO)?
            .len();
        if request.offset >= length || request.buffer().is_empty() {
            let eof = request.offset >= length;
            return request.complete(0, eof);
        }
        file.seek(SeekFrom::Start(request.offset))
            .map_err(|_io_error| nfsstat3::NFS3ERR_IO)?;
        let mut count = 0;
        // Fill the supplied initialized buffer, accepting short/Interrupted IO
        // without exposing its tail. Zero count still reports actual EOF.
        while count < request.buffer().len() {
            match file.read(&mut request.buffer()[count..]) {
                Ok(0) => break,
                Ok(n) => count += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => return Err(nfsstat3::NFS3ERR_IO),
            }
        }
        let len = file
            .metadata()
            .map_err(|_io_error| nfsstat3::NFS3ERR_IO)?
            .len();
        let eof = request.offset.saturating_add(count as u64) >= len;
        request.complete(count, eof)
    })
    .await
    .map_err(|_join_error| nfsstat3::NFS3ERR_SERVERFAULT)?
}

/// Write all segments and honor requested stability while retaining the whole
/// request/lease through completion of the blocking job, including error paths.
pub async fn write_owned_file<H: Send + 'static>(
    request: OwnedWrite<H>,
    path: PathBuf,
) -> Result<(std::fs::Metadata, stable_how), nfsstat3> {
    tokio::task::spawn_blocking(move || {
        let metadata =
            std::fs::symlink_metadata(&path).map_err(|_io_error| nfsstat3::NFS3ERR_IO)?;
        if !metadata.is_file() {
            return Err(nfsstat3::NFS3ERR_INVAL);
        }
        let result = (|| {
            let mut file = std::fs::OpenOptions::new().write(true).open(path)?;
            if !file.metadata()?.is_file() {
                return Err(std::io::ErrorKind::InvalidInput.into());
            }
            file.seek(SeekFrom::Start(request.offset))?;
            for chunk in request.data().chunks() {
                file.write_all(chunk)?;
            }
            match request.stable {
                stable_how::UNSTABLE => {}
                stable_how::DATA_SYNC => file.sync_data()?,
                stable_how::FILE_SYNC => file.sync_all()?,
            }
            Ok((file.metadata()?, request.stable))
        })()
        .map_err(|_io_error: std::io::Error| nfsstat3::NFS3ERR_IO);
        drop(request); // after all actual IO and synchronization
        result
    })
    .await
    .map_err(|_join_error| nfsstat3::NFS3ERR_SERVERFAULT)?
}

/// COMMIT flushes all dirty data/metadata for this file (a safe superset of range).
pub async fn commit_file(path: PathBuf) -> Result<(), nfsstat3> {
    tokio::task::spawn_blocking(move || {
        std::fs::OpenOptions::new()
            .write(true)
            .open(path)?
            .sync_all()
    })
    .await
    .map_err(|_join_error| nfsstat3::NFS3ERR_SERVERFAULT)?
    .map_err(|_io_error| nfsstat3::NFS3ERR_IO)
}
