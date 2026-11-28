use nfs3_server::nfs3_types::xdr_codec::BoundedOpaque;
use nfs3_server::vfs::{FileHandle, FileHandleBytes};
use uuid::Uuid;

// Use this as NfsReadFileSystem::Handle. The server prepends its 8-byte
// generation number, so the UUID is the second part of the wire handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct UuidHandle(Uuid);

impl FileHandle for UuidHandle {
    fn len(&self) -> usize {
        self.0.as_bytes().len()
    }

    fn as_bytes(&self) -> FileHandleBytes<'_> {
        BoundedOpaque::from_array(self.0.as_bytes()).into()
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        Uuid::from_slice(bytes).ok().map(Self)
    }
}

fn main() {
    let handle = UuidHandle(Uuid::from_u128(0x550e8400_e29b_41d4_a716_446655440000));
    let bytes = handle.as_bytes();
    assert_eq!(handle.len(), 16);
    assert_eq!(bytes.as_ref(), handle.0.as_bytes());
    assert_eq!(UuidHandle::from_bytes(bytes.as_ref()), Some(handle));
    assert!(UuidHandle::from_bytes(&[0; 15]).is_none());
    assert!(UuidHandle::from_bytes(&[0; 17]).is_none());
    println!("{handle:?}");
}
