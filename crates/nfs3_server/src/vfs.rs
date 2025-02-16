use std::cmp::Ordering;
use std::sync::LazyLock;
use std::time::SystemTime;

use async_trait::async_trait;
use nfs3_types::nfs3::{FSINFO3resok as fsinfo3, *};
use nfs3_types::xdr_codec::Opaque;

use crate::units::{GIBIBYTE, MEBIBYTE};

#[derive(Debug)]
pub struct DirEntrySimple<'a> {
    pub fileid: fileid3,
    pub name: filename3<'a>,
}
#[derive(Default, Debug)]
pub struct ReadDirSimpleResult<'a> {
    pub entries: Vec<DirEntrySimple<'a>>,
    pub end: bool,
}

#[derive(Debug)]
pub struct DirEntry<'a> {
    pub fileid: fileid3,
    pub name: filename3<'a>,
    pub attr: fattr3,
}
#[derive(Default, Debug)]
pub struct ReadDirResult<'a> {
    pub entries: Vec<DirEntry<'a>>,
    pub end: bool,
}

impl<'a> ReadDirSimpleResult<'a> {
    fn from_readdir_result(result: ReadDirResult<'a>) -> Self {
        let entries: Vec<DirEntrySimple> = result
            .entries
            .into_iter()
            .map(|e| DirEntrySimple {
                fileid: e.fileid,
                name: e.name,
            })
            .collect();
        Self {
            entries,
            end: result.end,
        }
    }
}

static GENERATION_NUMBER: LazyLock<u64> = LazyLock::new(|| {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
});

fn get_generation_number() -> u64 {
    *GENERATION_NUMBER
}

/// What capabilities are supported
pub enum VFSCapabilities {
    ReadOnly,
    ReadWrite,
}

/// The basic API to implement to provide an NFS file system
///
/// Opaque FH
/// ---------
/// Files are only uniquely identified by a 64-bit file id. (basically an inode number)
/// We automatically produce internally the opaque filehandle which is comprised of
///  - A 64-bit generation number derived from the server startup time
///   (i.e. so the opaque file handle expires when the NFS server restarts)
///  - The 64-bit file id
//
/// readdir pagination
/// ------------------
/// We do not use cookie verifier. We just use the start_after.  The
/// implementation should allow startat to start at any position. That is,
/// the next query to readdir may be the last entry in the previous readdir
/// response.
//
/// There is a wierd annoying thing about readdir that limits the number
/// of bytes in the response (instead of the number of entries). The caller
/// will have to truncate the readdir response / issue more calls to readdir
/// accordingly to fill up the expected number of bytes without exceeding it.
//
/// Other requirements
/// ------------------
///  getattr needs to be fast. NFS uses that a lot
//
///  The 0 fileid is reserved and should not be used
///
#[async_trait]
pub trait NFSFileSystem: Sync {
    /// Returns the set of capabilities supported
    fn capabilities(&self) -> VFSCapabilities;
    /// Returns the ID the of the root directory "/"
    fn root_dir(&self) -> fileid3;
    /// Look up the id of a path in a directory
    ///
    /// i.e. given a directory dir/ containing a file a.txt
    /// this may call lookup(id_of("dir/"), "a.txt")
    /// and this should return the id of the file "dir/a.txt"
    ///
    /// This method should be fast as it is used very frequently.
    async fn lookup(&self, dirid: fileid3, filename: &filename3) -> Result<fileid3, nfsstat3>;

    /// Returns the attributes of an id.
    /// This method should be fast as it is used very frequently.
    async fn getattr(&self, id: fileid3) -> Result<fattr3, nfsstat3>;

    /// Sets the attributes of an id
    /// this should return Err(nfsstat3::NFS3ERR_ROFS) if readonly
    async fn setattr(&self, id: fileid3, setattr: sattr3) -> Result<fattr3, nfsstat3>;

    /// Reads the contents of a file returning (bytes, EOF)
    /// Note that offset/count may go past the end of the file and that
    /// in that case, all bytes till the end of file are returned.
    /// EOF must be flagged if the end of the file is reached by the read.
    async fn read(&self, id: fileid3, offset: u64, count: u32)
        -> Result<(Vec<u8>, bool), nfsstat3>;

    /// Writes the contents of a file returning (bytes, EOF)
    /// Note that offset/count may go past the end of the file and that
    /// in that case, the file is extended.
    /// If not supported due to readonly file system
    /// this should return Err(nfsstat3::NFS3ERR_ROFS)
    async fn write(&self, id: fileid3, offset: u64, data: &[u8]) -> Result<fattr3, nfsstat3>;

    /// Creates a file with the following attributes.
    /// If not supported due to readonly file system
    /// this should return Err(nfsstat3::NFS3ERR_ROFS)
    async fn create(
        &self,
        dirid: fileid3,
        filename: &filename3,
        attr: sattr3,
    ) -> Result<(fileid3, fattr3), nfsstat3>;

    /// Creates a file if it does not already exist
    /// this should return Err(nfsstat3::NFS3ERR_ROFS)
    async fn create_exclusive(
        &self,
        dirid: fileid3,
        filename: &filename3,
    ) -> Result<fileid3, nfsstat3>;

    /// Makes a directory with the following attributes.
    /// If not supported dur to readonly file system
    /// this should return Err(nfsstat3::NFS3ERR_ROFS)
    async fn mkdir(
        &self,
        dirid: fileid3,
        dirname: &filename3,
    ) -> Result<(fileid3, fattr3), nfsstat3>;

    /// Removes a file.
    /// If not supported due to readonly file system
    /// this should return Err(nfsstat3::NFS3ERR_ROFS)
    async fn remove(&self, dirid: fileid3, filename: &filename3) -> Result<(), nfsstat3>;

    /// Removes a file.
    /// If not supported due to readonly file system
    /// this should return Err(nfsstat3::NFS3ERR_ROFS)
    async fn rename(
        &self,
        from_dirid: fileid3,
        from_filename: &filename3,
        to_dirid: fileid3,
        to_filename: &filename3,
    ) -> Result<(), nfsstat3>;

    /// Returns the contents of a directory with pagination.
    /// Directory listing should be deterministic.
    /// Up to max_entries may be returned, and start_after is used
    /// to determine where to start returning entries from.
    ///
    /// For instance if the directory has entry with ids `[1,6,2,11,8,9]`
    /// and start_after=6, readdir should returning 2,11,8,...
    //
    async fn readdir(
        &self,
        dirid: fileid3,
        start_after: fileid3,
        max_entries: usize,
    ) -> Result<ReadDirResult, nfsstat3>;

    /// Simple version of readdir.
    /// Only need to return filename and id
    async fn readdir_simple(
        &self,
        dirid: fileid3,
        count: usize,
    ) -> Result<ReadDirSimpleResult, nfsstat3> {
        Ok(ReadDirSimpleResult::from_readdir_result(
            self.readdir(dirid, 0, count).await?,
        ))
    }

    /// Makes a symlink with the following attributes.
    /// If not supported due to readonly file system
    /// this should return Err(nfsstat3::NFS3ERR_ROFS)
    async fn symlink(
        &self,
        dirid: fileid3,
        linkname: &filename3,
        symlink: &nfspath3,
        attr: &sattr3,
    ) -> Result<(fileid3, fattr3), nfsstat3>;

    /// Reads a symlink
    async fn readlink(&self, id: fileid3) -> Result<nfspath3, nfsstat3>;

    /// Get static file system Information
    async fn fsinfo(&self, root_fileid: fileid3) -> Result<fsinfo3, nfsstat3> {
        let dir_attr: post_op_attr = match self.getattr(root_fileid).await {
            Ok(v) => post_op_attr::Some(v),
            Err(_) => post_op_attr::None,
        };

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
                nseconds: 1000000,
            },
            properties: FSF3_SYMLINK | FSF3_HOMOGENEOUS | FSF3_CANSETTIME,
        };
        Ok(res)
    }

    /// Converts the fileid to an opaque NFS file handle. Optional.
    fn id_to_fh(&self, id: fileid3) -> nfs_fh3 {
        let gennum = get_generation_number();
        let mut ret: Vec<u8> = Vec::new();
        ret.extend_from_slice(&gennum.to_le_bytes());
        ret.extend_from_slice(&id.to_le_bytes());
        nfs_fh3 {
            data: Opaque::owned(ret),
        }
    }
    /// Converts an opaque NFS file handle to a fileid.  Optional.
    fn fh_to_id(&self, id: &nfs_fh3) -> Result<fileid3, nfsstat3> {
        if id.data.len() != 16 {
            return Err(nfsstat3::NFS3ERR_BADHANDLE);
        }
        let gen = u64::from_le_bytes(id.data[0..8].try_into().unwrap());
        let id = u64::from_le_bytes(id.data[8..16].try_into().unwrap());
        let gennum = get_generation_number();
        match gen.cmp(&gennum) {
            Ordering::Less => Err(nfsstat3::NFS3ERR_STALE),
            Ordering::Greater => Err(nfsstat3::NFS3ERR_BADHANDLE),
            Ordering::Equal => Ok(id),
        }
    }
    /// Converts a complete path to a fileid.  Optional.
    /// The default implementation walks the directory structure with lookup()
    async fn path_to_id(&self, path: &str) -> Result<fileid3, nfsstat3> {
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

    fn serverid(&self) -> cookieverf3 {
        let gennum = get_generation_number();
        cookieverf3(gennum.to_le_bytes())
    }
}
