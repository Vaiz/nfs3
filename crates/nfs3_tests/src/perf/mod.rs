mod list_fs;
mod read_fs;
mod write_fs;

pub use list_fs::ListFs;
pub use read_fs::ReadFs;
pub use write_fs::WriteFs;

use nfs3_server::nfs3_types;
use nfs3_server::vfs::{DirEntryPlus, FileHandleU64, NextResult, ReadDirPlusIterator};
use nfs3_types::nfs3::{fattr3, ftype3, nfstime3, specdata3};

const ROOT_HANDLE: FileHandleU64 = FileHandleU64::new(1);
const FILE_HANDLE: FileHandleU64 = FileHandleU64::new(2);

fn dir_attr(fileid: u64) -> fattr3 {
    fattr3 {
        type_: ftype3::NF3DIR,
        mode: 0o755,
        nlink: 2,
        uid: 0,
        gid: 0,
        size: 4096,
        used: 4096,
        rdev: specdata3::default(),
        fsid: 1,
        fileid,
        atime: nfstime3::default(),
        mtime: nfstime3::default(),
        ctime: nfstime3::default(),
    }
}

fn file_attr(fileid: u64, size: u64) -> fattr3 {
    fattr3 {
        type_: ftype3::NF3REG,
        mode: 0o644,
        nlink: 1,
        uid: 0,
        gid: 0,
        size,
        used: size,
        rdev: specdata3::default(),
        fsid: 1,
        fileid,
        atime: nfstime3::default(),
        mtime: nfstime3::default(),
        ctime: nfstime3::default(),
    }
}

struct EmptyDirIter;

impl ReadDirPlusIterator<FileHandleU64> for EmptyDirIter {
    async fn next(&mut self) -> NextResult<DirEntryPlus<FileHandleU64>> {
        NextResult::Eof
    }
}
