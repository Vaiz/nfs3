#![allow(unused)] // FIXME: remove this

use std::fs::File;
use std::path::Path;

use intaglio::Symbol;
use nfs3_server::vfs::{
    DirEntry, DirEntryPlus, FileHandle, FileHandleU64, NfsFileSystem, NfsReadFileSystem,
    ReadDirIterator, ReadDirPlusIterator, VFSCapabilities,
};
use nfs3_types::nfs3::*;
use nfs3_types::xdr_codec::Opaque;
use tracing_subscriber::field::debug;

use crate::server;

const MEBIBYTE: u32 = 1024 * 1024;
const GIBIBYTE: u64 = 1024 * 1024 * 1024;

#[derive(Debug)]
pub struct WasmFs<FS> {
    fs: FS,
    id_to_path_table: intaglio::path::SymbolTable,
    server_id: u64,
    root: FileHandleU64,
}

pub fn new_mem_fs() -> WasmFs<wasmer_vfs::mem_fs::FileSystem> {
    let mut id_to_path_table = intaglio::path::SymbolTable::new();
    let root = id_to_path_table
        .intern(Path::new("/"))
        .expect("failed to add root path");

    let mut fs = WasmFs {
        fs: wasmer_vfs::mem_fs::FileSystem::default(),
        id_to_path_table,
        server_id: (0xdead_beef << 32), // keep the same server id for testing
        root: 0.into(),
    };

    fs.root = fs.symbol_to_id(root);
    fs
}

impl<FS> WasmFs<FS> {
    fn symbol_to_id(&self, symbol: Symbol) -> FileHandleU64 {
        let id = self.server_id | (symbol.id() as u64);
        id.into()
    }

    fn id_to_path(&self, id: &FileHandleU64) -> Result<&Path, nfsstat3> {
        let id = id.as_u64();
        let server_id = id & 0xFFFF_FFFF_0000_0000;
        if server_id != self.server_id {
            return Err(nfsstat3::NFS3ERR_STALE);
        }
        let local_id = Symbol::new((id & 0xFFFF_FFFF) as u32);
        self.id_to_path_table
            .get(local_id)
            .ok_or(nfsstat3::NFS3ERR_BADHANDLE)
    }

    fn filename_to_utf8<'a>(filename: &'a filename3<'a>) -> Result<&'a str, nfsstat3> {
        const INVALID_SYMBOLS: &[char] = &['/', '\0', '\n', '\r', '\t'];

        let filename =
            std::str::from_utf8(filename.as_ref()).map_err(|_| nfsstat3::NFS3ERR_INVAL)?;
        if filename.contains(INVALID_SYMBOLS) {
            Err(nfsstat3::NFS3ERR_INVAL)
        } else {
            Ok(filename)
        }
    }

    fn make_attr(id: &FileHandleU64, metadata: &wasmer_vfs::Metadata) -> Result<fattr3, nfsstat3> {
        use wasmer_vfs::FileType;

        let file_type = match metadata.file_type() {
            FileType { symlink: true, .. } => ftype3::NF3LNK,
            FileType {
                char_device: true, ..
            } => ftype3::NF3CHR,
            FileType {
                block_device: true, ..
            } => ftype3::NF3BLK,
            FileType { socket: true, .. } => ftype3::NF3SOCK,
            FileType { fifo: true, .. } => ftype3::NF3FIFO,
            FileType { dir: true, .. } => ftype3::NF3DIR,
            FileType { file: true, .. } => ftype3::NF3REG,
            _ => return Err(nfsstat3::NFS3ERR_IO),
        };

        fn to_nfstime3(wasm_time: u64) -> nfstime3 {
            const SECOND: u64 = 1_000_000_000;
            nfstime3 {
                seconds: (wasm_time / SECOND) as u32,
                nseconds: (wasm_time % SECOND) as u32,
            }
        }

        let fattr = fattr3 {
            type_: file_type,
            mode: 0o755,
            nlink: 1,
            uid: 0,
            gid: 0,
            size: metadata.len(),
            used: metadata.len(),
            rdev: specdata3::default(),
            fsid: 0,
            fileid: id.as_u64(),
            atime: to_nfstime3(metadata.accessed()),
            mtime: to_nfstime3(metadata.modified()),
            ctime: to_nfstime3(metadata.created()),
        };

        Ok(fattr)
    }
}
impl<FS: wasmer_vfs::FileSystem> NfsReadFileSystem for WasmFs<FS> {
    type Handle = nfs3_server::vfs::FileHandleU64;

    fn root_dir(&self) -> Self::Handle {
        self.root
    }
    async fn lookup(
        &self,
        dirid: &Self::Handle,
        filename: &filename3<'_>,
    ) -> Result<Self::Handle, nfsstat3> {
        let path = self.id_to_path(dirid)?;
        let filename = Self::filename_to_utf8(filename)?;

        let full_path = match filename {
            "." => path.to_path_buf(),
            ".." => path.parent().ok_or(nfsstat3::NFS3ERR_INVAL)?.to_path_buf(),
            _ => path.join(filename),
        };

        tracing::debug!("lookup: {:?} -> {:?}", path, full_path);

        let id = self
            .id_to_path_table
            .check_interned(&full_path)
            .ok_or(nfsstat3::NFS3ERR_NOENT)?;

        Ok(self.symbol_to_id(id))
    }

    async fn getattr(&self, id: &Self::Handle) -> Result<fattr3, nfsstat3> {
        let path = self.id_to_path(id)?;
        let metadata = self.fs.metadata(path).map_err(wasm_error_to_nfsstat3)?;
        Self::make_attr(id, &metadata)
    }

    async fn read(
        &self,
        id: &Self::Handle,
        offset: u64,
        count: u32,
    ) -> Result<(Vec<u8>, bool), nfsstat3> {
        use std::io::SeekFrom;

        use wasmer_vfs::OpenOptionsConfig;

        const OPEN_OPTIONS: OpenOptionsConfig = OpenOptionsConfig {
            read: true,
            write: false,
            create_new: false,
            append: false,
            truncate: false,
            create: false,
        };

        let path = self.id_to_path(id)?;
        let mut file = self
            .fs
            .new_open_options()
            .options(OPEN_OPTIONS)
            .open(path)
            .map_err(wasm_error_to_nfsstat3)?;

        let mut buf = vec![0; count as usize];
        file.seek(SeekFrom::Start(offset))
            .map_err(io_error_to_nfsstat3)?;
        let read = file.read(&mut buf).map_err(io_error_to_nfsstat3)?;
        let eof = file
            .bytes_available_read()
            .map_err(wasm_error_to_nfsstat3)?
            .unwrap_or_default()
            == 0;
        buf.truncate(read as usize);
        Ok((buf, eof))
    }

    async fn readdir(
        &self,
        dirid: &Self::Handle,
        start_after: cookie3,
    ) -> Result<impl ReadDirIterator, nfsstat3> {
        Err::<ReadDirStubIterator, nfsstat3>(nfsstat3::NFS3ERR_NOTSUPP)
    }

    async fn readdirplus(
        &self,
        dirid: &Self::Handle,
        start_after: cookie3,
    ) -> Result<impl ReadDirPlusIterator<Self::Handle>, nfsstat3> {
        Err::<ReadDirStubIterator, nfsstat3>(nfsstat3::NFS3ERR_NOTSUPP)
    }

    /// Reads a symlink
    async fn readlink(&self, id: &Self::Handle) -> Result<nfspath3<'_>, nfsstat3> {
        Err(nfsstat3::NFS3ERR_NOTSUPP)
    }

    /// Get static file system Information
    async fn fsinfo(&self, root_fileid: &Self::Handle) -> Result<FSINFO3resok, nfsstat3> {
        let dir_attr: post_op_attr = match self.getattr(root_fileid).await {
            Ok(v) => post_op_attr::Some(v),
            Err(_) => post_op_attr::None,
        };

        let res = FSINFO3resok {
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

    async fn lookup_by_path(&self, path: &str) -> Result<Self::Handle, nfsstat3> {
        let path = Path::new(path);
        let id = self
            .id_to_path_table
            .check_interned(path)
            .ok_or(nfsstat3::NFS3ERR_NOENT)?;

        Ok(self.symbol_to_id(id))
    }
}

impl<FS: wasmer_vfs::FileSystem> nfs3_server::vfs::NfsFileSystem for WasmFs<FS> {
    async fn setattr(&self, id: &Self::Handle, setattr: sattr3) -> Result<fattr3, nfsstat3> {
        use wasmer_vfs::OpenOptionsConfig;

        let path = self.id_to_path(id)?;
        let mut file = None;

        const OPEN_OPTIONS: OpenOptionsConfig = OpenOptionsConfig {
            read: true,
            write: false,
            create_new: false,
            append: false,
            truncate: false,
            create: false,
        };

        if let Nfs3Option::Some(mode) = setattr.mode {
            // not supported
        }
        if let Nfs3Option::Some(uid) = setattr.uid {
            // not supported
        }
        if let Nfs3Option::Some(gid) = setattr.gid {
            // not supported
        }
        if let Nfs3Option::Some(size) = setattr.size {
            if file.is_none() {
                let mut options = OPEN_OPTIONS;
                options.truncate = true;
                let opened_file = self
                    .fs
                    .new_open_options()
                    .options(options)
                    .open(path)
                    .map_err(wasm_error_to_nfsstat3)?;
                file = Some(opened_file);
            }

            file.as_mut()
                .unwrap()
                .set_len(size)
                .map_err(wasm_error_to_nfsstat3)?;
        }
        // if let Nfs3Option::Some(atime) = setattr.atime
        // if let Nfs3Option::Some(mtime) = setattr.mtime

        let metadata = self.fs.metadata(path).map_err(wasm_error_to_nfsstat3)?;
        Self::make_attr(id, &metadata)
    }

    async fn write(&self, id: &Self::Handle, offset: u64, data: &[u8]) -> Result<fattr3, nfsstat3> {
        Err(nfsstat3::NFS3ERR_NOTSUPP)
    }

    async fn create(
        &self,
        dirid: &Self::Handle,
        filename: &filename3<'_>,
        attr: sattr3,
    ) -> Result<(Self::Handle, fattr3), nfsstat3> {
        Err(nfsstat3::NFS3ERR_NOTSUPP)
    }

    async fn create_exclusive(
        &self,
        dirid: &Self::Handle,
        filename: &filename3<'_>,
        createverf: createverf3,
    ) -> Result<Self::Handle, nfsstat3> {
        Err(nfsstat3::NFS3ERR_NOTSUPP)
    }
    async fn mkdir(
        &self,
        dirid: &Self::Handle,
        dirname: &filename3<'_>,
    ) -> Result<(Self::Handle, fattr3), nfsstat3> {
        Err(nfsstat3::NFS3ERR_NOTSUPP)
    }

    async fn remove(&self, dirid: &Self::Handle, filename: &filename3<'_>) -> Result<(), nfsstat3> {
        Err(nfsstat3::NFS3ERR_NOTSUPP)
    }

    async fn rename<'a>(
        &self,
        from_dirid: &Self::Handle,
        from_filename: &filename3<'a>,
        to_dirid: &Self::Handle,
        to_filename: &filename3<'a>,
    ) -> Result<(), nfsstat3> {
        Err(nfsstat3::NFS3ERR_NOTSUPP)
    }

    async fn symlink<'a>(
        &self,
        dirid: &Self::Handle,
        linkname: &filename3<'a>,
        symlink: &nfspath3<'a>,
        attr: &sattr3,
    ) -> Result<(Self::Handle, fattr3), nfsstat3> {
        Err(nfsstat3::NFS3ERR_NOTSUPP)
    }
}

fn wasm_error_to_nfsstat3(err: wasmer_vfs::FsError) -> nfsstat3 {
    use wasmer_vfs::FsError;
    match err {
        FsError::BaseNotDirectory => nfsstat3::NFS3ERR_NOTDIR,
        FsError::NotAFile => nfsstat3::NFS3ERR_INVAL,
        FsError::InvalidFd => nfsstat3::NFS3ERR_BADHANDLE,
        FsError::AlreadyExists => nfsstat3::NFS3ERR_EXIST,
        FsError::Lock => nfsstat3::NFS3ERR_JUKEBOX,
        FsError::IOError => nfsstat3::NFS3ERR_IO,
        FsError::AddressInUse => nfsstat3::NFS3ERR_ACCES,
        FsError::AddressNotAvailable => nfsstat3::NFS3ERR_ACCES,
        FsError::BrokenPipe => nfsstat3::NFS3ERR_IO,
        FsError::ConnectionAborted => nfsstat3::NFS3ERR_IO,
        FsError::ConnectionRefused => nfsstat3::NFS3ERR_IO,
        FsError::ConnectionReset => nfsstat3::NFS3ERR_IO,
        FsError::Interrupted => nfsstat3::NFS3ERR_IO,
        FsError::InvalidData => nfsstat3::NFS3ERR_INVAL,
        FsError::InvalidInput => nfsstat3::NFS3ERR_INVAL,
        FsError::NotConnected => nfsstat3::NFS3ERR_IO,
        FsError::EntityNotFound => nfsstat3::NFS3ERR_NOENT,
        FsError::NoDevice => nfsstat3::NFS3ERR_NODEV,
        FsError::PermissionDenied => nfsstat3::NFS3ERR_ACCES,
        FsError::TimedOut => nfsstat3::NFS3ERR_IO,
        FsError::UnexpectedEof => nfsstat3::NFS3ERR_IO,
        FsError::WouldBlock => nfsstat3::NFS3ERR_JUKEBOX,
        FsError::WriteZero => nfsstat3::NFS3ERR_IO,
        FsError::DirectoryNotEmpty => nfsstat3::NFS3ERR_NOTEMPTY,
        FsError::UnknownError => nfsstat3::NFS3ERR_SERVERFAULT,
    }
}

fn io_error_to_nfsstat3(err: std::io::Error) -> nfsstat3 {
    use std::io::ErrorKind;
    match err.kind() {
        ErrorKind::NotFound => nfsstat3::NFS3ERR_NOENT,
        ErrorKind::PermissionDenied => nfsstat3::NFS3ERR_ACCES,
        ErrorKind::AlreadyExists => nfsstat3::NFS3ERR_EXIST,
        ErrorKind::InvalidInput => nfsstat3::NFS3ERR_INVAL,
        ErrorKind::UnexpectedEof => nfsstat3::NFS3ERR_IO,
        _ => nfsstat3::NFS3ERR_IO,
    }
}

struct ReadDirStubIterator;

impl ReadDirPlusIterator<FileHandleU64> for ReadDirStubIterator {
    async fn next(&mut self) -> nfs3_server::vfs::NextResult<DirEntryPlus<FileHandleU64>> {
        nfs3_server::vfs::NextResult::Err(nfsstat3::NFS3ERR_NOTSUPP)
    }
}

impl ReadDirIterator for ReadDirStubIterator {
    async fn next(&mut self) -> nfs3_server::vfs::NextResult<DirEntry> {
        nfs3_server::vfs::NextResult::Err(nfsstat3::NFS3ERR_NOTSUPP)
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use nfs3_server::vfs::NfsFileSystem;
    use wasmer_vfs::{FileSystem, OpenOptionsConfig};

    use super::*;

    #[tokio::test]
    async fn test_file_id() {
        let fs = super::new_mem_fs();
        let id = fs.lookup_by_path("/").await.unwrap();
        assert_eq!(id, fs.root_dir());

        let path = fs.id_to_path(&fs.root_dir()).unwrap();
        assert_eq!(path, Path::new("/"));
    }

    #[test]
    fn test_perf() -> anyhow::Result<()> {
        let vfs = wasmer_vfs::mem_fs::FileSystem::default();
        let file_options = OpenOptionsConfig {
            read: true,
            write: true,
            create_new: true,
            append: false,
            truncate: false,
            create: false,
        };

        let start = std::time::Instant::now();
        for i in 0..1000 {
            let mut file = vfs
                .new_open_options()
                .options(file_options.clone())
                .open(format!("/file_{i}"))?;
            file.write_all(b"Hello, World!")?;
            file.flush()?;
        }
        let elapsed = start.elapsed();
        println!("Elapsed: {elapsed:?}");

        let start = std::time::Instant::now();
        for i in 0..1000 {
            let path = PathBuf::from(format!("/dir_{i}"));
            vfs.create_dir(&path)?;
        }
        let elapsed = start.elapsed();
        println!("Elapsed: {elapsed:?}");

        Ok(())
    }
}
