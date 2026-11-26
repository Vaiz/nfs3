use nfs3_types::nfs3::{NFS3_FHSIZE, nfs_fh3, nfsstat3, writeverf3};
use nfs3_types::xdr_codec::BoundedOpaque;

const GENERATION_SIZE: usize = size_of::<u64>();
const MAX_HANDLE_SIZE: usize = NFS3_FHSIZE - GENERATION_SIZE;

/// Represents a file handle
///
/// This uniquely identifies a file or folder in the implementation of
/// [`NfsReadFileSystem`][1] and [`NfsFileSystem`][2]. The value is serialized
/// into a [`nfs_fh3`] handle and sent to the client. The server reserves
/// the first 8 bytes of the handle for its own use, while the remaining
/// 56 bytes can be freely used by the implementation.
///
/// Use [`FileHandleU64`] for numeric IDs or [`FileHandleBytes`]
/// for byte IDs. Custom implementations should validate or construct their
/// bounded representation before returning a handle to the server.
///
/// [1]: crate::vfs::NfsReadFileSystem
/// [2]: crate::vfs::NfsFileSystem
#[expect(
    clippy::len_without_is_empty,
    reason = "Empty handles can be checked with as_bytes"
)]
pub trait FileHandle: std::fmt::Debug + Clone + Send + Sync {
    /// The length of the handle in bytes
    fn len(&self) -> usize;
    /// Returns the serialized handle, limited to 56 bytes.
    ///
    /// Construct it by converting a [`BoundedOpaque`] with [`Into::into`].
    fn as_bytes(&self) -> FileHandleBytes<'_>;
    /// Creates a handle from a byte slice
    fn from_bytes(bytes: &[u8]) -> Option<Self>
    where
        Self: Sized;
}

/// A serialized VFS file handle of at most 56 bytes.
///
/// The owned form implements [`FileHandle`] for backends that use byte IDs.
/// Use [`FileHandle::from_bytes`] to construct it from a slice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileHandleBytes<'a>(BoundedOpaque<'a, MAX_HANDLE_SIZE>);

impl<'a> From<BoundedOpaque<'a, MAX_HANDLE_SIZE>> for FileHandleBytes<'a> {
    fn from(bytes: BoundedOpaque<'a, MAX_HANDLE_SIZE>) -> Self {
        Self(bytes)
    }
}

impl AsRef<[u8]> for FileHandleBytes<'_> {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl FileHandle for FileHandleBytes<'static> {
    fn len(&self) -> usize {
        self.as_ref().len()
    }

    fn as_bytes(&self) -> FileHandleBytes<'_> {
        self.clone()
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        BoundedOpaque::from_slice(bytes)
            .ok()
            .map(|bytes| Self(bytes.into_owned()))
    }
}

/// A basic 8-bytes long file handle
///
/// If your implementation of [`NfsReadFileSystem`][1] uses a file handle that is
/// 8 bytes long, you can use this type instead of creating you own.
///
/// [1]: crate::vfs::NfsReadFileSystem
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileHandleU64 {
    id: [u8; 8],
}

impl FileHandleU64 {
    /// Creates a new file handle from a u64
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self {
            id: id.to_ne_bytes(),
        }
    }

    /// Converts the file handle to a u64
    #[must_use]
    pub const fn as_u64(&self) -> u64 {
        u64::from_ne_bytes(self.id)
    }
}

impl FileHandle for FileHandleU64 {
    fn len(&self) -> usize {
        self.id.len()
    }
    fn as_bytes(&self) -> FileHandleBytes<'_> {
        BoundedOpaque::from_array(&self.id).into()
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
        write!(f, "{}", self.as_u64())
    }
}

impl From<u64> for FileHandleU64 {
    fn from(id: u64) -> Self {
        Self::new(id)
    }
}

impl From<FileHandleU64> for u64 {
    fn from(val: FileHandleU64) -> Self {
        val.as_u64()
    }
}

impl PartialEq<u64> for FileHandleU64 {
    fn eq(&self, other: &u64) -> bool {
        &self.as_u64() == other
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FileHandleConverter {
    generation_number: u64,
    generation_number_le: [u8; GENERATION_SIZE],
}

impl FileHandleConverter {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "It's OK to truncate the generation number to its low 64 bits"
    )]
    pub(crate) fn new() -> Self {
        let generation_number = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("failed to get system time")
            .as_millis() as u64;

        Self::with_generation_number(generation_number)
    }

    /// Creates a `FileHandleConverter` with a specific generation number.
    ///
    /// Use this when multiple NFS server instances need to share the same
    /// generation number so that file handles remain valid across all
    /// instances (e.g. behind a load balancer).
    #[must_use]
    pub const fn with_generation_number(generation_number: u64) -> Self {
        Self {
            generation_number,
            generation_number_le: generation_number.to_le_bytes(),
        }
    }

    /// Encodes a VFS handle into an on-the-wire [`nfs_fh3`] with the generation prefix.
    #[must_use]
    #[expect(
        clippy::missing_panics_doc,
        reason = "FileHandleBytes is bounded to 56 bytes, so generation + handle always fits in 64 bytes"
    )]
    pub fn fh_to_nfs(&self, id: &impl FileHandle) -> nfs_fh3 {
        let bytes = id.as_bytes();
        let bytes = bytes.as_ref();
        let mut data = [0; NFS3_FHSIZE];
        let data = &mut data[..GENERATION_SIZE + bytes.len()];
        let (generation, handle) = data.split_at_mut(GENERATION_SIZE);
        generation.copy_from_slice(&self.generation_number_le);
        handle.copy_from_slice(bytes);
        nfs_fh3 {
            data: BoundedOpaque::from_slice(data)
                .expect("generation prefix and bounded VFS handle fit in the wire handle")
                .into_owned(),
        }
    }

    /// Decodes an on-the-wire [`nfs_fh3`] after checking its generation prefix.
    pub fn fh_from_nfs<FH>(&self, id: &nfs_fh3) -> Result<FH, nfsstat3>
    where
        FH: FileHandle,
    {
        self.check_handle(id)?;

        FH::from_bytes(&id.data[GENERATION_SIZE..]).ok_or(nfsstat3::NFS3ERR_BADHANDLE)
    }

    fn check_handle(&self, id: &nfs_fh3) -> Result<(), nfsstat3> {
        if id.data.len() < GENERATION_SIZE {
            return Err(nfsstat3::NFS3ERR_BADHANDLE);
        }
        if id.data[..GENERATION_SIZE] == self.generation_number_le {
            Ok(())
        } else {
            let id_gen = u64::from_le_bytes(
                id.data[..GENERATION_SIZE]
                    .try_into()
                    .map_err(|_length_error| nfsstat3::NFS3ERR_BADHANDLE)?,
            );
            if id_gen < self.generation_number {
                Err(nfsstat3::NFS3ERR_STALE)
            } else {
                Err(nfsstat3::NFS3ERR_BADHANDLE)
            }
        }
    }

    /// This is a cookie that the client can use to determine
    /// whether the server has rebooted between a call to WRITE
    /// and a subsequent call to COMMIT. This cookie must be
    /// consistent during a single boot session and must be
    /// unique between instances of the NFS version 3 protocol
    /// server where uncommitted data may be lost.
    #[must_use]
    pub const fn verf(&self) -> writeverf3 {
        writeverf3(self.generation_number_le)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_handle_u64() {
        let handle = FileHandleU64::new(42);
        assert_eq!(handle.as_u64(), 42);
        assert_eq!(handle.len(), 8);
        assert_eq!(handle.as_bytes().as_ref(), &42u64.to_ne_bytes());
        assert_eq!(
            FileHandleU64::from_bytes(&[42, 0, 0, 0, 0, 0, 0, 0]),
            Some(handle)
        );
    }

    type ByteHandle = FileHandleBytes<'static>;

    #[test]
    fn test_file_handle_converter_wire_limit() {
        let converter = FileHandleConverter::new();
        let max = ByteHandle::from_bytes(&[1; 56]).unwrap();
        let encoded = converter.fh_to_nfs(&max);
        assert_eq!(encoded.data.len(), 64);
        assert_eq!(converter.fh_from_nfs::<ByteHandle>(&encoded), Ok(max));
        assert!(ByteHandle::from_bytes(&[1; 57]).is_none());
    }

    #[test]
    fn test_file_handle_converter_3bytes() {
        let converter = FileHandleConverter::new();
        let handle = ByteHandle::from_bytes(&[1, 2, 3]).unwrap();
        let nfs_handle = converter.fh_to_nfs(&handle);
        assert_eq!(nfs_handle.data.len(), 11);
        assert_eq!(nfs_handle.data[0..8], converter.generation_number_le);
        assert_eq!(&nfs_handle.data[8..], handle.as_bytes().as_ref());

        let converted_handle = converter.fh_from_nfs::<ByteHandle>(&nfs_handle).unwrap();
        assert_eq!(converted_handle, handle);
    }

    #[test]
    fn test_with_generation_number_round_trip() {
        let converter = FileHandleConverter::with_generation_number(12345);
        let handle = ByteHandle::from_bytes(&[1, 2, 3]).unwrap();
        let nfs_handle = converter.fh_to_nfs(&handle);

        assert_eq!(nfs_handle.data[0..8], 12345u64.to_le_bytes());

        let round_tripped = converter.fh_from_nfs::<ByteHandle>(&nfs_handle).unwrap();
        assert_eq!(round_tripped, handle);
    }

    #[test]
    fn test_with_generation_number_shared_across_instances() {
        // Two converters with the same generation (simulating cluster nodes)
        // must accept each other's handles.
        let node_a = FileHandleConverter::with_generation_number(42);
        let node_b = FileHandleConverter::with_generation_number(42);

        let handle = ByteHandle::from_bytes(&[7, 8, 9]).unwrap();
        let nfs_handle = node_a.fh_to_nfs(&handle);

        let decoded = node_b.fh_from_nfs::<ByteHandle>(&nfs_handle).unwrap();
        assert_eq!(decoded, handle);
    }

    #[test]
    fn test_with_generation_number_stale_vs_badhandle() {
        let converter = FileHandleConverter::with_generation_number(1000);
        let handle = ByteHandle::from_bytes(&[1, 2, 3]).unwrap();
        let nfs_handle = converter.fh_to_nfs(&handle);

        // Newer generation → STALE (handle is from an older server)
        let newer = FileHandleConverter::with_generation_number(2000);
        assert_eq!(
            newer.fh_from_nfs::<ByteHandle>(&nfs_handle),
            Err(nfsstat3::NFS3ERR_STALE)
        );

        // Older generation → BADHANDLE (handle is from a future server, unexpected)
        let older = FileHandleConverter::with_generation_number(500);
        assert_eq!(
            older.fh_from_nfs::<ByteHandle>(&nfs_handle),
            Err(nfsstat3::NFS3ERR_BADHANDLE)
        );
    }

    #[test]
    fn test_with_generation_number_verf() {
        let converter = FileHandleConverter::with_generation_number(12345);
        assert_eq!(converter.verf().0, 12345u64.to_le_bytes());
    }

    #[test]
    fn test_file_handle_converter_19bytes() {
        let converter = FileHandleConverter::new();
        let handle = ByteHandle::from_bytes(&[
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19,
        ])
        .unwrap();
        let nfs_handle = converter.fh_to_nfs(&handle);
        assert_eq!(nfs_handle.data.len(), 27);
        assert_eq!(nfs_handle.data[0..8], converter.generation_number_le);
        assert_eq!(&nfs_handle.data[8..], handle.as_bytes().as_ref());

        let converted_handle = converter.fh_from_nfs::<ByteHandle>(&nfs_handle).unwrap();
        assert_eq!(converted_handle, handle);
    }
}
