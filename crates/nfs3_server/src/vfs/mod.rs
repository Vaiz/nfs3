//! The basic API to implement to provide an NFS file system
//!
//! Opaque FH
//! ---------
//! Files are only uniquely identified by a 64-bit file id. (basically an inode number)
//! We automatically produce internally the opaque filehandle which is comprised of
//!  - A 64-bit generation number derived from the server startup time (i.e. so the opaque file
//!    handle expires when the NFS server restarts)
//!  - The 64-bit file id
//!
//! readdir pagination
//! ------------------
//! We do not use cookie verifier. We just use the `start_after`.  The
//! implementation should allow startat to start at any position. That is,
//! the next query to readdir may be the last entry in the previous readdir
//! response.
//!
//! Other requirements
//! ------------------
//!  getattr needs to be fast. NFS uses that a lot
//!
//!  The 0 fileid is reserved and should not be used

pub mod adapter;
mod iterator;

use std::sync::LazyLock;
use std::time::SystemTime;

pub use iterator::*;
use nfs3_types::nfs3::{
    FSF3_CANSETTIME, FSF3_HOMOGENEOUS, FSF3_SYMLINK, FSINFO3resok as fsinfo3, cookieverf3, fattr3,
    fileid3, filename3, nfs_fh3, nfspath3, nfstime3, post_op_attr, sattr3,
};
use nfs3_types::xdr_codec::Opaque;

use crate::units::{GIBIBYTE, MEBIBYTE};

pub(crate) static DEFAULT_FH_CONVERTER: LazyLock<DefaultNfsFhConverter> =
    LazyLock::new(DefaultNfsFhConverter::new);

pub(crate) struct DefaultNfsFhConverter {
    generation_number: u64,
    generation_number_le: [u8; 8],
}

impl DefaultNfsFhConverter {
    const HANDLE_LENGTH: usize = 16;

    #[allow(clippy::cast_possible_truncation)] // it's ok to truncate the generation number
    pub(crate) fn new() -> Self {
        let generation_number = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("failed to get system time")
            .as_millis() as u64;

        Self {
            generation_number,
            generation_number_le: generation_number.to_le_bytes(),
        }
    }

    /// Converts the fileid to an opaque NFS file handle. Optional.
    pub(crate) fn id_to_fh(&self, id: fileid3) -> nfs_fh3 {
        let mut ret: Vec<u8> = Vec::with_capacity(Self::HANDLE_LENGTH);
        ret.extend_from_slice(&self.generation_number_le);
        ret.extend_from_slice(&id.to_le_bytes());
        nfs_fh3 {
            data: Opaque::owned(ret),
        }
    }
    /// Converts an opaque NFS file handle to a fileid.  Optional.
    pub(crate) fn fh_to_id(&self, id: &nfs_fh3) -> Result<fileid3, nfsstat3> {
        if id.data.len() != Self::HANDLE_LENGTH {
            return Err(nfsstat3::NFS3ERR_BADHANDLE);
        }

        if id.data[0..8] == self.generation_number_le {
            Ok(u64::from_le_bytes(
                id.data[8..16]
                    .try_into()
                    .map_err(|_| nfsstat3::NFS3ERR_BADHANDLE)?,
            ))
        } else {
            let id_gen = u64::from_le_bytes(
                id.data[0..8]
                    .try_into()
                    .map_err(|_| nfsstat3::NFS3ERR_BADHANDLE)?,
            );
            if id_gen < self.generation_number {
                Err(nfsstat3::NFS3ERR_STALE)
            } else {
                Err(nfsstat3::NFS3ERR_BADHANDLE)
            }
        }
    }
    pub(crate) const fn generation_number_le(&self) -> [u8; 8] {
        self.generation_number_le
    }
}

/// What capabilities are supported
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VFSCapabilities {
    ReadOnly,
    ReadWrite,
}

/// Read-only file system interface
///
/// This should be enough to implement a read-only NFS server.
/// If you want to implement a read-write server, you should implement
/// the [`NfsFileSystem`] trait too.
pub trait NfsReadFileSystem: Send + Sync {
    /// Returns the ID the of the root directory "/"
    fn root_dir(&self) -> fileid3;
    /// Look up the id of a path in a directory
    ///
    /// i.e. given a directory dir/ containing a file a.txt
    /// this may call `lookup(id_of("dir`/"), "a.txt")
    /// and this should return the id of the file "dir/a.txt"
    ///
    /// This method should be fast as it is used very frequently.
    fn lookup(
        &self,
        dirid: fileid3,
        filename: &filename3<'_>,
    ) -> impl Future<Output = Result<fileid3, nfsstat3>> + Send;

    /// Returns the attributes of an id.
    /// This method should be fast as it is used very frequently.
    fn getattr(&self, id: fileid3) -> impl Future<Output = Result<fattr3, nfsstat3>> + Send;

    /// Reads the contents of a file returning (bytes, EOF)
    /// Note that offset/count may go past the end of the file and that
    /// in that case, all bytes till the end of file are returned.
    /// EOF must be flagged if the end of the file is reached by the read.
    fn read(
        &self,
        id: fileid3,
        offset: u64,
        count: u32,
    ) -> impl Future<Output = Result<(Vec<u8>, bool), nfsstat3>> + Send;

    /// Simple version of readdir.
    /// Only need to return filename and id
    fn readdir(
        &self,
        dirid: fileid3,
        start_after: fileid3,
    ) -> impl Future<Output = Result<impl ReadDirIterator, nfsstat3>> + Send;

    /// Returns the contents of a directory with pagination.
    /// Directory listing should be deterministic.
    /// Up to `max_entries` may be returned, and `start_after` is used
    /// to determine where to start returning entries from.
    ///
    /// For instance if the directory has entry with ids `[1,6,2,11,8,9]`
    /// and `start_after=6`, readdir should returning 2,11,8,...
    fn readdirplus(
        &self,
        dirid: fileid3,
        start_after: fileid3,
    ) -> impl Future<Output = Result<impl ReadDirPlusIterator, nfsstat3>> + Send;

    /// Reads a symlink
    fn readlink(&self, id: fileid3) -> impl Future<Output = Result<nfspath3, nfsstat3>> + Send;

    /// Get static file system Information
    fn fsinfo(
        &self,
        root_fileid: fileid3,
    ) -> impl Future<Output = Result<fsinfo3, nfsstat3>> + Send {
        async move {
            let dir_attr = self
                .getattr(root_fileid)
                .await
                .map_or(post_op_attr::None, post_op_attr::Some);

            let res = fsinfo3 {
                obj_attributes: dir_attr,
                rtmax: MEBIBYTE,
                rtpref: MEBIBYTE,
                rtmult: MEBIBYTE,
                wtmax: MEBIBYTE,
                wtpref: MEBIBYTE,
                wtmult: MEBIBYTE,
                dtpref: MEBIBYTE,
                maxfilesize: 128u64 * GIBIBYTE,
                time_delta: nfstime3 {
                    seconds: 0,
                    nseconds: 1_000_000,
                },
                properties: FSF3_SYMLINK | FSF3_HOMOGENEOUS | FSF3_CANSETTIME,
            };
            Ok(res)
        }
    }

    /// Converts the fileid to an opaque NFS file handle. Optional.
    fn id_to_fh(&self, id: fileid3) -> nfs_fh3 {
        DEFAULT_FH_CONVERTER.id_to_fh(id)
    }
    /// Converts an opaque NFS file handle to a fileid.  Optional.
    fn fh_to_id(&self, id: &nfs_fh3) -> Result<fileid3, nfsstat3> {
        DEFAULT_FH_CONVERTER.fh_to_id(id)
    }
    /// Converts a complete path to a fileid.  Optional.
    /// The default implementation walks the directory structure with `lookup()`
    fn path_to_id(&self, path: &str) -> impl Future<Output = Result<fileid3, nfsstat3>> + Send {
        async move {
            let splits = path.split('/');
            let mut fid = self.root_dir();
            for component in splits {
                if component.is_empty() {
                    continue;
                }
                fid = self.lookup(fid, &component.as_bytes().into()).await?;
            }
            Ok(fid)
        }
    }

    fn serverid(&self) -> cookieverf3 {
        cookieverf3(DEFAULT_FH_CONVERTER.generation_number_le())
    }
}

/// Write file system interface
///
/// This is the interface to implement if you want to provide a writable NFS server.
pub trait NfsFileSystem: NfsReadFileSystem {
    /// Returns the set of capabilities supported
    fn capabilities(&self) -> VFSCapabilities {
        VFSCapabilities::ReadWrite
    }

    /// Sets the attributes of an id
    /// this should return `Err(nfsstat3::NFS3ERR_ROFS)` if readonly
    fn setattr(
        &self,
        id: fileid3,
        setattr: sattr3,
    ) -> impl Future<Output = Result<fattr3, nfsstat3>> + Send;

    /// Writes the contents of a file returning (bytes, EOF)
    /// Note that offset/count may go past the end of the file and that
    /// in that case, the file is extended.
    /// If not supported due to readonly file system
    /// this should return `Err(nfsstat3::NFS3ERR_ROFS)`
    ///
    /// # `NFS3ERR_INVAL`:
    ///
    /// Some NFS version 2 protocol server implementations
    /// incorrectly returned `NFSERR_ISDIR` if the file system
    /// object type was not a regular file. The correct return
    /// value for the NFS version 3 protocol is `NFS3ERR_INVAL`.
    fn write(
        &self,
        id: fileid3,
        offset: u64,
        data: &[u8],
    ) -> impl Future<Output = Result<fattr3, nfsstat3>> + Send;

    /// Creates a file with the following attributes.
    /// If not supported due to readonly file system
    /// this should return `Err(nfsstat3::NFS3ERR_ROFS)`
    fn create(
        &self,
        dirid: fileid3,
        filename: &filename3<'_>,
        attr: sattr3,
    ) -> impl Future<Output = Result<(fileid3, fattr3), nfsstat3>> + Send;

    /// Creates a file if it does not already exist
    /// this should return `Err(nfsstat3::NFS3ERR_ROFS)`
    fn create_exclusive(
        &self,
        dirid: fileid3,
        filename: &filename3<'_>,
    ) -> impl Future<Output = Result<fileid3, nfsstat3>> + Send;

    /// Makes a directory with the following attributes.
    /// If not supported dur to readonly file system
    /// this should return `Err(nfsstat3::NFS3ERR_ROFS)`
    fn mkdir(
        &self,
        dirid: fileid3,
        dirname: &filename3<'_>,
    ) -> impl Future<Output = Result<(fileid3, fattr3), nfsstat3>> + Send;

    /// Removes a file.
    /// If not supported due to readonly file system
    /// this should return `Err(nfsstat3::NFS3ERR_ROFS)`
    fn remove(
        &self,
        dirid: fileid3,
        filename: &filename3<'_>,
    ) -> impl Future<Output = Result<(), nfsstat3>> + Send;

    /// Removes a file.
    /// If not supported due to readonly file system
    /// this should return `Err(nfsstat3::NFS3ERR_ROFS)`
    fn rename<'a>(
        &self,
        from_dirid: fileid3,
        from_filename: &filename3<'a>,
        to_dirid: fileid3,
        to_filename: &filename3<'a>,
    ) -> impl Future<Output = Result<(), nfsstat3>> + Send;

    /// Makes a symlink with the following attributes.
    /// If not supported due to readonly file system
    /// this should return `Err(nfsstat3::NFS3ERR_ROFS)`
    fn symlink<'a>(
        &self,
        dirid: fileid3,
        linkname: &filename3<'a>,
        symlink: &nfspath3<'a>,
        attr: &sattr3,
    ) -> impl Future<Output = Result<(fileid3, fattr3), nfsstat3>> + Send;
}
