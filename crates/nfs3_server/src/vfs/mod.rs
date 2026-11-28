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

pub mod adapters;
pub(crate) mod handle;
mod iterator;

use bytes::Bytes;
pub use handle::{FileHandle, FileHandleBytes, FileHandleU64};
pub use iterator::*;

use crate::nfs3_types::nfs3::{
    ACCESS3_DELETE, ACCESS3_EXECUTE, ACCESS3_EXTEND, ACCESS3_LOOKUP, ACCESS3_MODIFY, ACCESS3_READ,
    FSF3_CANSETTIME, FSF3_HOMOGENEOUS, FSF3_SYMLINK, FSINFO3resok as fsinfo3, createverf3, fattr3,
    filename3, ftype3, nfspath3, nfsstat3, nfstime3, post_op_attr, sattr3, stable_how,
};
use crate::nfs3_types::rpc::auth_unix;
use crate::units::{GIBIBYTE, MEBIBYTE};
use crate::vfs::adapters::ReadDirPlusToReadDir;

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
    /// Type that can be used to indentify a file or folder in the file system.
    ///
    /// For more information, see [`FileHandle`].
    type Handle: FileHandle;

    /// Returns the ID the of the root directory "/"
    fn root_dir(&self) -> Self::Handle;

    /// Returns whether to serve a call with this `AUTH_UNIX` credential
    /// (`None` for other flavors). Refused calls get `AUTH_TOOWEAK`.
    fn authorize(&self, auth: Option<&auth_unix>) -> bool {
        let _ = auth;
        true
    }

    /// Computes POSIX access bits for `auth` on `id`, returning post-op attributes and granted
    /// bits. Errors from resolving `id` are returned rather than swallowed.
    fn access(
        &self,
        id: &Self::Handle,
        auth: Option<&auth_unix>,
        requested: u32,
        capabilities: VFSCapabilities,
    ) -> impl Future<Output = Result<(fattr3, u32), nfsstat3>> + Send {
        async move {
            let attr = self.getattr(id).await?;
            let granted = posix_access(&attr, auth, requested, capabilities);
            Ok((attr, granted))
        }
    }

    /// Look up `filename` in `dirid`, returning `(child_handle, child_attr, dir_attr)`.
    fn lookup_with_attr(
        &self,
        dirid: &Self::Handle,
        filename: &filename3<'_>,
        auth: Option<&auth_unix>,
    ) -> impl Future<
        Output = Result<(Self::Handle, post_op_attr, post_op_attr), (nfsstat3, post_op_attr)>,
    > + Send {
        let _ = auth;
        async move {
            let dir_attr = self
                .getattr(dirid)
                .await
                .map_or(post_op_attr::None, post_op_attr::Some);
            match self.lookup(dirid, filename).await {
                Ok(fid) => {
                    let obj_attr = self
                        .getattr(&fid)
                        .await
                        .map_or(post_op_attr::None, post_op_attr::Some);
                    Ok((fid, obj_attr, dir_attr))
                }
                Err(stat) => Err((stat, dir_attr)),
            }
        }
    }

    /// Reads bytes and post-op attributes for `id` in a single request.
    fn read_with_attr(
        &self,
        id: &Self::Handle,
        offset: u64,
        count: u32,
        auth: Option<&auth_unix>,
    ) -> impl Future<Output = Result<(post_op_attr, Bytes, bool), (nfsstat3, post_op_attr)>> + Send
    {
        let _ = auth;
        async move {
            let attr = self
                .getattr(id)
                .await
                .map_or(post_op_attr::None, post_op_attr::Some);
            match self.read(id, offset, count).await {
                Ok((bytes, eof)) => Ok((attr, bytes, eof)),
                Err(stat) => Err((stat, attr)),
            }
        }
    }

    /// Reads a symlink target and post-op attributes for `id` in a single request.
    fn readlink_with_attr(
        &self,
        id: &Self::Handle,
        auth: Option<&auth_unix>,
    ) -> impl Future<Output = Result<(post_op_attr, nfspath3<'_>), (nfsstat3, post_op_attr)>> + Send
    {
        let _ = auth;
        async move {
            let attr = self
                .getattr(id)
                .await
                .map_or(post_op_attr::None, post_op_attr::Some);
            match self.readlink(id).await {
                Ok(target) => Ok((attr, target)),
                Err(stat) => Err((stat, attr)),
            }
        }
    }

    /// Look up the id of a path in a directory
    ///
    /// i.e. given a directory dir/ containing a file `a.txt`
    /// this may call `lookup(id_of("dir/"), "a.txt")`
    /// and this should return the id of the file `dir/a.txt`
    ///
    /// This method should be fast as it is used very frequently.
    fn lookup(
        &self,
        dirid: &Self::Handle,
        filename: &filename3<'_>,
    ) -> impl Future<Output = Result<Self::Handle, nfsstat3>> + Send;

    /// This method is used when the client tries to mount a subdirectory.
    /// The default implementation walks the directory structure with [`lookup`](Self::lookup).
    fn lookup_by_path(
        &self,
        path: &str,
    ) -> impl Future<Output = Result<Self::Handle, nfsstat3>> + Send {
        async move {
            let splits = path.split('/');
            let mut fid = self.root_dir();
            for component in splits {
                if component.is_empty() {
                    continue;
                }
                fid = self.lookup(&fid, &component.as_bytes().into()).await?;
            }
            Ok(fid)
        }
    }

    /// Returns the attributes of an id.
    /// This method should be fast as it is used very frequently.
    fn getattr(&self, id: &Self::Handle) -> impl Future<Output = Result<fattr3, nfsstat3>> + Send;

    /// Reads the contents of a file returning (bytes, EOF)
    /// Note that offset/count may go past the end of the file and that
    /// in that case, all bytes till the end of file are returned.
    /// EOF must be flagged if the end of the file is reached by the read.
    ///
    /// The server caps the count at the `rtmax` returned by [`Self::fsinfo`]
    /// for [`Self::root_dir`].
    ///
    /// Returned buffers of 1 KiB or more are sent to the client without copying;
    /// a `Vec<u8>` converts into [`Bytes`] for free.
    fn read(
        &self,
        id: &Self::Handle,
        offset: u64,
        count: u32,
    ) -> impl Future<Output = Result<(Bytes, bool), nfsstat3>> + Send;

    /// Simple version of readdir. Only need to return filename and id
    ///
    /// By default it uses `readdirplus` method to create an iterator
    fn readdir(
        &self,
        dirid: &Self::Handle,
        cookie: u64,
    ) -> impl Future<Output = Result<impl ReadDirIterator, nfsstat3>> + Send {
        async move {
            self.readdirplus(dirid, cookie)
                .await
                .map(ReadDirPlusToReadDir::new)
        }
    }

    /// Returns the contents of a directory with pagination.
    /// Directory listing should be deterministic.
    /// Up to `max_entries` may be returned, and `start_after` is used
    /// to determine where to start returning entries from.
    ///
    /// For instance if the directory has entry with ids `[1,6,2,11,8,9]`
    /// and `start_after=6`, readdir should returning 2,11,8,...
    fn readdirplus(
        &self,
        dirid: &Self::Handle,
        cookie: u64,
    ) -> impl Future<Output = Result<impl ReadDirPlusIterator<Self::Handle>, nfsstat3>> + Send;

    /// Reads a symlink
    fn readlink(
        &self,
        id: &Self::Handle,
    ) -> impl Future<Output = Result<nfspath3<'_>, nfsstat3>> + Send;

    /// Get static file system information.
    ///
    /// During initialization, the server queries the export root and keeps
    /// its `rtmax` as the READ limit for the server's lifetime. If that
    /// query fails, the server does not start.
    fn fsinfo(
        &self,
        root_fileid: &Self::Handle,
    ) -> impl Future<Output = Result<fsinfo3, nfsstat3>> + Send {
        async move { Ok(default_fsinfo(self.getattr(root_fileid).await.ok())) }
    }
}

pub(crate) fn default_fsinfo(attr: Option<fattr3>) -> fsinfo3 {
    fsinfo3 {
        obj_attributes: attr.map_or(post_op_attr::None, post_op_attr::Some),
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
        id: &Self::Handle,
        setattr: sattr3,
    ) -> impl Future<Output = Result<fattr3, nfsstat3>> + Send;

    /// Writes the contents of a file.
    ///
    /// If the offset and count go past the end of the file, the file is extended.
    /// If not supported due to readonly file system this should return
    /// `Err(nfsstat3::NFS3ERR_ROFS)`
    ///
    /// # Returns
    ///
    /// On success, returns `(fattr3, stable_how)` where:
    /// - [`fattr3`] contains the updated file attributes after the write.
    /// - [`stable_how`] reflects the level of stability actually achieved, which must be equal to
    ///   or greater than the requested `stable` level.
    ///
    /// # `stable` parameter
    ///
    /// - [`stable_how::FILE_SYNC`]: all data and metadata must be committed to stable storage
    ///   before returning. Any other behavior is a protocol violation.
    /// - [`stable_how::DATA_SYNC`]: all data and enough metadata to retrieve it must be committed
    ///   before returning. Could be implemented identically to `FILE_SYNC`.
    /// - [`stable_how::UNSTABLE`]: the server may defer committing any data or metadata.
    ///   Uncommitted data can later be flushed via [`commit`][NfsFileSystem::commit]. Usually,
    ///   clients will send a series of UNSTABLE writes followed by a
    ///   [`commit`][NfsFileSystem::commit], so servers can optimize for this case by deferring the
    ///   actual disk writes until the [`commit`][NfsFileSystem::commit] is received.
    ///
    /// # `NFS3ERR_INVAL`:
    ///
    /// Some NFS version 2 protocol server implementations
    /// incorrectly returned `NFSERR_ISDIR` if the file system
    /// object type was not a regular file. The correct return
    /// value for the NFS version 3 protocol is `NFS3ERR_INVAL`.
    ///
    /// `data` is a reference-counted [`Bytes`]. If it makes up at least half of
    /// the buffer the request was received in, it shares that buffer instead of
    /// being copied, so keeping `data` keeps at most twice its size in memory.
    fn write(
        &self,
        id: &Self::Handle,
        offset: u64,
        data: Bytes,
        stable: stable_how,
    ) -> impl Future<Output = Result<(fattr3, stable_how), nfsstat3>> + Send;

    /// Creates a file with the following attributes.
    /// If not supported due to readonly file system
    /// this should return `Err(nfsstat3::NFS3ERR_ROFS)`
    fn create(
        &self,
        dirid: &Self::Handle,
        filename: &filename3<'_>,
        attr: sattr3,
    ) -> impl Future<Output = Result<(Self::Handle, fattr3), nfsstat3>> + Send;

    /// Creates a file if it does not already exist.
    /// If not supported due to readonly file system
    /// this should return `Err(nfsstat3::NFS3ERR_ROFS)`
    ///
    /// # NOTE:
    /// If the server can not support these exclusive create
    /// semantics, possibly because of the requirement to commit
    /// the verifier to stable storage, it should fail the CREATE
    /// request with the error, `NFS3ERR_NOTSUPP`.
    fn create_exclusive(
        &self,
        dirid: &Self::Handle,
        filename: &filename3<'_>,
        createverf: createverf3,
    ) -> impl Future<Output = Result<Self::Handle, nfsstat3>> + Send;

    /// Makes a directory with the following attributes.
    /// If not supported dur to readonly file system
    /// this should return `Err(nfsstat3::NFS3ERR_ROFS)`
    fn mkdir(
        &self,
        dirid: &Self::Handle,
        dirname: &filename3<'_>,
    ) -> impl Future<Output = Result<(Self::Handle, fattr3), nfsstat3>> + Send;

    /// Removes a file.
    /// If not supported due to readonly file system
    /// this should return `Err(nfsstat3::NFS3ERR_ROFS)`
    fn remove(
        &self,
        dirid: &Self::Handle,
        filename: &filename3<'_>,
    ) -> impl Future<Output = Result<(), nfsstat3>> + Send;

    /// Removes a file.
    /// If not supported due to readonly file system
    /// this should return `Err(nfsstat3::NFS3ERR_ROFS)`
    ///
    /// # NOTE:
    ///
    /// If the directory, `to_dirid`, already contains an entry with
    /// the name, `to_filename`, the source object must be compatible
    /// with the target: either both are non-directories or both
    /// are directories and the target must be empty. If
    /// compatible, the existing target is removed before the
    /// rename occurs. If they are not compatible or if the target
    /// is a directory but not empty, the server should return the
    /// error, `NFS3ERR_EXIST`.
    fn rename<'a>(
        &self,
        from_dirid: &Self::Handle,
        from_filename: &filename3<'a>,
        to_dirid: &Self::Handle,
        to_filename: &filename3<'a>,
    ) -> impl Future<Output = Result<(), nfsstat3>> + Send;

    /// Makes a symlink with the following attributes.
    /// If not supported due to readonly file system
    /// this should return `Err(nfsstat3::NFS3ERR_ROFS)`
    fn symlink<'a>(
        &self,
        dirid: &Self::Handle,
        linkname: &filename3<'a>,
        symlink: &nfspath3<'a>,
        attr: &sattr3,
    ) -> impl Future<Output = Result<(Self::Handle, fattr3), nfsstat3>> + Send;

    /// Commits previously written data to stable storage.
    ///
    /// `offset` and `count` indicate the region to commit. If `count` is 0
    /// the entire file should be committed.
    fn commit(
        &self,
        id: &Self::Handle,
        offset: u64,
        count: u32,
    ) -> impl Future<Output = Result<(), nfsstat3>> + Send;
}

/// Computes POSIX `ACCESS` bits from `attr`, `auth`, and filesystem capabilities.
#[must_use]
pub fn posix_access(
    attr: &fattr3,
    auth: Option<&auth_unix>,
    requested: u32,
    capabilities: VFSCapabilities,
) -> u32 {
    let mode = attr.mode;
    let is_dir = matches!(attr.type_, ftype3::NF3DIR);
    let is_reg = matches!(attr.type_, ftype3::NF3REG);
    let (can_read, can_write, can_exec) = match auth {
        Some(auth) if auth.uid == 0 => {
            let exec = if is_dir {
                true
            } else if is_reg {
                (mode & 0o111) != 0
            } else {
                false
            };
            (true, true, exec)
        }
        Some(auth) => {
            let triad = if auth.uid == attr.uid {
                (mode >> 6) & 0o7
            } else if auth.gid == attr.gid || auth.gids.contains(&attr.gid) {
                (mode >> 3) & 0o7
            } else {
                mode & 0o7
            };
            ((triad & 0o4) != 0, (triad & 0o2) != 0, (triad & 0o1) != 0)
        }
        None => {
            let triad = mode & 0o7;
            ((triad & 0o4) != 0, (triad & 0o2) != 0, (triad & 0o1) != 0)
        }
    };

    let mut granted = 0u32;
    if can_read {
        granted |= ACCESS3_READ;
    }
    if is_dir && can_exec {
        granted |= ACCESS3_LOOKUP;
    }
    if is_reg && can_exec {
        granted |= ACCESS3_EXECUTE;
    }
    if matches!(capabilities, VFSCapabilities::ReadWrite) && can_write {
        granted |= ACCESS3_MODIFY | ACCESS3_EXTEND;
        if is_dir {
            granted |= ACCESS3_DELETE;
        }
    }
    requested & granted
}

#[cfg(test)]
mod tests {
    use nfs3_types::nfs3::specdata3;
    use nfs3_types::xdr_codec::BoundedVec;

    use super::*;

    const ALL_ACCESS: u32 = ACCESS3_READ
        | ACCESS3_LOOKUP
        | ACCESS3_MODIFY
        | ACCESS3_EXTEND
        | ACCESS3_DELETE
        | ACCESS3_EXECUTE;

    fn make_attr(type_: ftype3, mode: u32, uid: u32, gid: u32) -> fattr3 {
        fattr3 {
            type_,
            mode,
            nlink: 1,
            uid,
            gid,
            size: 0,
            used: 0,
            rdev: specdata3::default(),
            fsid: 0,
            fileid: 1,
            atime: nfstime3::default(),
            mtime: nfstime3::default(),
            ctime: nfstime3::default(),
        }
    }

    fn make_auth(uid: u32, gid: u32, gids: &[u32]) -> auth_unix {
        auth_unix {
            uid,
            gid,
            gids: BoundedVec::try_from(gids.to_vec()).expect("test gids fit"),
            ..auth_unix::default()
        }
    }

    #[test]
    fn posix_access_checks_owner_group_supplementary_and_other() {
        let attr = make_attr(ftype3::NF3REG, 0o754, 1000, 2000);
        let owner = make_auth(1000, 3000, &[]);
        let group = make_auth(1001, 2000, &[]);
        let supp = make_auth(1001, 3000, &[2000]);
        let other = make_auth(1001, 3000, &[]);

        assert_eq!(
            posix_access(&attr, Some(&owner), ALL_ACCESS, VFSCapabilities::ReadWrite),
            ACCESS3_READ | ACCESS3_EXECUTE | ACCESS3_MODIFY | ACCESS3_EXTEND
        );
        assert_eq!(
            posix_access(&attr, Some(&owner), ALL_ACCESS, VFSCapabilities::ReadOnly),
            ACCESS3_READ | ACCESS3_EXECUTE
        );
        assert_eq!(
            posix_access(&attr, Some(&group), ALL_ACCESS, VFSCapabilities::ReadWrite),
            ACCESS3_READ | ACCESS3_EXECUTE
        );
        assert_eq!(
            posix_access(&attr, Some(&supp), ALL_ACCESS, VFSCapabilities::ReadWrite),
            ACCESS3_READ | ACCESS3_EXECUTE
        );
        assert_eq!(
            posix_access(&attr, Some(&other), ALL_ACCESS, VFSCapabilities::ReadWrite),
            ACCESS3_READ
        );
        assert_eq!(
            posix_access(&attr, None, ALL_ACCESS, VFSCapabilities::ReadWrite),
            ACCESS3_READ
        );
    }

    #[test]
    fn posix_access_root_requires_exec_bit_on_regular_files() {
        let root = make_auth(0, 0, &[]);
        let non_exec_file = make_attr(ftype3::NF3REG, 0o644, 1000, 1000);
        let exec_file = make_attr(ftype3::NF3REG, 0o641, 1000, 1000);
        let zero_dir = make_attr(ftype3::NF3DIR, 0o000, 1000, 1000);

        assert_eq!(
            posix_access(
                &non_exec_file,
                Some(&root),
                ALL_ACCESS,
                VFSCapabilities::ReadOnly
            ),
            ACCESS3_READ
        );
        assert_eq!(
            posix_access(
                &exec_file,
                Some(&root),
                ALL_ACCESS,
                VFSCapabilities::ReadOnly
            ),
            ACCESS3_READ | ACCESS3_EXECUTE
        );
        assert_eq!(
            posix_access(
                &zero_dir,
                Some(&root),
                ALL_ACCESS,
                VFSCapabilities::ReadWrite
            ),
            ACCESS3_READ | ACCESS3_LOOKUP | ACCESS3_MODIFY | ACCESS3_EXTEND | ACCESS3_DELETE
        );
    }
}
