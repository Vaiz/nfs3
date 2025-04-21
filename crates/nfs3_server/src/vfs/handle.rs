use std::sync::LazyLock;

use nfs3_types::nfs3::{fileid3, nfs_fh3, nfsstat3};
use nfs3_types::xdr_codec::Opaque;

/// Represents a file handle
/// 
/// This uniquely identifies a file or folder in the implementation of
/// [`NfsReadFileSystem`] and [`NfsFileSystem`]. The value is serialized
/// into a [`nfs_fh3`] handle and sent to the client. The server reserves
/// the first 8 bytes of the handle for its own use, while the remaining
/// 56 bytes can be freely used by the implementation.
pub trait FileHandle: std::fmt::Debug + Clone + Send + Sync {
    /// The length of the handle in bytes
    fn len(&self) -> usize;
    /// Returns the handle as a byte slice
    fn as_bytes(&self) -> &[u8];
    /// Creates a handle from a byte slice
    fn from_bytes(bytes: &[u8]) -> Option<Self>
    where
        Self: Sized;
}


/// A file handle that is 8 bytes long
/// 
/// If your implementation of [`NfsReadFileSystem`] uses a file handle that is
/// 8 bytes long, you can use this type.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FileHandleU64 {
    id: [u8; 8],
}

impl FileHandle for FileHandleU64 {
    fn len(&self) -> usize {
        self.id.len()
    }
    fn as_bytes(&self) -> &[u8] {
        &self.id
    }
    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        bytes.try_into().ok().map(|id| Self { id })
    }
}

impl std::fmt::Debug for FileHandleU64 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("FileHandleU64")
            .field(&u64::from_ne_bytes(self.id))
            .finish()
    }
}

impl std::fmt::Display for FileHandleU64 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", u64::from_ne_bytes(self.id))
    }
}

impl From<u64> for FileHandleU64 {
    fn from(id: u64) -> Self {        
        Self { id: id.to_ne_bytes() }
    }
}

impl Into<u64> for FileHandleU64 {
    fn into(self) -> u64 {
        u64::from_ne_bytes(self.id)
    }
}

impl PartialEq<u64> for FileHandleU64 {
    fn eq(&self, other: &u64) -> bool {
        self.id == other.to_ne_bytes()
    }
}

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
        let generation_number = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("failed to get system time")
            .as_millis() as u64;

        Self {
            generation_number,
            generation_number_le: generation_number.to_le_bytes(),
        }
    }

    pub(crate) fn id_to_fh(&self, id: fileid3) -> nfs_fh3 {
        let mut ret: Vec<u8> = Vec::with_capacity(Self::HANDLE_LENGTH);
        ret.extend_from_slice(&self.generation_number_le);
        ret.extend_from_slice(&id.to_le_bytes());
        nfs_fh3 {
            data: Opaque::owned(ret),
        }
    }
    pub(crate) fn fh_to_id(&self, id: &nfs_fh3) -> Result<fileid3, nfsstat3> {
        self.check_handle(id)?;

        Ok(u64::from_le_bytes(
            id.data[8..16]
                .try_into()
                .map_err(|_| nfsstat3::NFS3ERR_BADHANDLE)?,
        ))
    }
    pub(crate) const fn generation_number_le(&self) -> [u8; 8] {
        self.generation_number_le
    }

    pub(crate) fn fh_to_nfs(&self, id: &impl FileHandle) -> nfs_fh3 {
        let mut ret: Vec<u8> = Vec::with_capacity(8 + id.len());
        ret.extend_from_slice(&self.generation_number_le);
        ret.extend_from_slice(id.as_bytes());
        nfs_fh3 {
            data: Opaque::owned(ret),
        }
    }

    pub(crate) fn fh_from_nfs<FH>(&self, id: &nfs_fh3) -> Result<FH, nfsstat3>
    where
        FH: FileHandle,
    {
        self.check_handle(id)?;

        FH::from_bytes(&id.data[8..]).ok_or(nfsstat3::NFS3ERR_BADHANDLE)
    }

    fn check_handle(&self, id: &nfs_fh3) -> Result<(), nfsstat3> {
        if id.data.len() != Self::HANDLE_LENGTH {
            return Err(nfsstat3::NFS3ERR_BADHANDLE);
        }
        if id.data[0..8] == self.generation_number_le {
            Ok(())
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
}
