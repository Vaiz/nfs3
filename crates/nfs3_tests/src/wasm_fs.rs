#![allow(unused)] // FIXME: remove this

use std::path::Path;

use intaglio::Symbol;
use nfs3_server::vfs::{ReadDirResult, ReadDirSimpleResult, VFSCapabilities};
use nfs3_types::nfs3::*;
use nfs3_types::xdr_codec::Opaque;

const MEBIBYTE: u32 = 1024 * 1024;
const GIBIBYTE: u64 = 1024 * 1024 * 1024;

#[derive(Debug)]
struct WasmFs<FS> {
    fs: FS,
    id_to_path_table: intaglio::path::SymbolTable,
    server_id: u64,
    root: fileid3,
}

impl<FS> WasmFs<FS> {
    pub fn new_mem_fs() -> WasmFs<wasmer_vfs::mem_fs::FileSystem> {
        let mut fhid_map = intaglio::path::SymbolTable::new();
        let root = fhid_map
            .intern(Path::new("/"))
            .expect("failed to add root path");
        
        let mut fs = WasmFs {
            fs: wasmer_vfs::mem_fs::FileSystem::default(),
            id_to_path_table: intaglio::path::SymbolTable::new(),
            server_id: (0xdead_beef << 32), // keep the same server id for testing
            root: 0,
        };

        fs.root = fs.symbol_to_id(root);
        fs
    }

    fn symbol_to_id(&self, symbol: Symbol) -> fileid3 {
        self.server_id | (symbol.id() as u64)
    }

    fn id_to_path(&self, id: fileid3) -> Option<&Path> {
        let local_id = Symbol::new((id & 0xFFFF_FFFF) as u32);
        self
            .id_to_path_table
            .get(local_id)
    }
}

#[async_trait::async_trait]
impl<FS: wasmer_vfs::FileSystem> nfs3_server::vfs::NFSFileSystem for WasmFs<FS> {
    fn capabilities(&self) -> VFSCapabilities {
        VFSCapabilities::ReadWrite
    }
    fn root_dir(&self) -> fileid3 {
        self.root
    }
    async fn lookup(&self, dirid: fileid3, filename: &filename3) -> Result<fileid3, nfsstat3> {
        todo!()
    }

    async fn getattr(&self, id: fileid3) -> Result<fattr3, nfsstat3> {
        todo!()
    }

    async fn setattr(&self, id: fileid3, setattr: sattr3) -> Result<fattr3, nfsstat3> {
        todo!()
    }

    async fn read(
        &self,
        id: fileid3,
        offset: u64,
        count: u32,
    ) -> Result<(Vec<u8>, bool), nfsstat3> {
        todo!()
    }
    async fn write(&self, id: fileid3, offset: u64, data: &[u8]) -> Result<fattr3, nfsstat3> {
        todo!()
    }

    async fn create(
        &self,
        dirid: fileid3,
        filename: &filename3,
        attr: sattr3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        todo!()
    }

    async fn create_exclusive(
        &self,
        dirid: fileid3,
        filename: &filename3,
    ) -> Result<fileid3, nfsstat3> {
        todo!()
    }

    async fn mkdir(
        &self,
        dirid: fileid3,
        dirname: &filename3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        todo!()
    }

    async fn remove(&self, dirid: fileid3, filename: &filename3) -> Result<(), nfsstat3> {
        todo!()
    }

    async fn rename(
        &self,
        from_dirid: fileid3,
        from_filename: &filename3,
        to_dirid: fileid3,
        to_filename: &filename3,
    ) -> Result<(), nfsstat3> {
        todo!()
    }

    async fn readdir(
        &self,
        dirid: fileid3,
        start_after: fileid3,
        max_entries: usize,
    ) -> Result<ReadDirResult<'static>, nfsstat3> {
        todo!()
    }

    async fn readdir_simple(
        &self,
        dirid: fileid3,
        count: usize,
    ) -> Result<ReadDirSimpleResult, nfsstat3> {
        todo!()
    }

    async fn symlink(
        &self,
        dirid: fileid3,
        linkname: &filename3,
        symlink: &nfspath3,
        attr: &sattr3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        todo!()
    }

    /// Reads a symlink
    async fn readlink(&self, id: fileid3) -> Result<nfspath3, nfsstat3> {
        todo!()
    }

    /// Get static file system Information
    async fn fsinfo(&self, root_fileid: fileid3) -> Result<FSINFO3resok, nfsstat3> {
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

    fn id_to_fh(&self, id: fileid3) -> nfs_fh3 {
        nfs_fh3 {
            data: Opaque::owned(id.to_ne_bytes().to_vec()),
        }
    }
    fn fh_to_id(&self, id: &nfs_fh3) -> Result<fileid3, nfsstat3> {
        let id: [u8; 8] = id
            .data
            .as_ref()
            .try_into()
            .map_err(|_| nfsstat3::NFS3ERR_BADHANDLE)?;
        Ok(fileid3::from_ne_bytes(id))
    }
    async fn path_to_id(&self, path: &str) -> Result<fileid3, nfsstat3> {
        let path = Path::new(path);
        let id = self
            .id_to_path_table
            .check_interned(path)
            .ok_or(nfsstat3::NFS3ERR_NOENT)?;

        Ok(self.symbol_to_id(id))
    }

    fn serverid(&self) -> cookieverf3 {
        cookieverf3(self.server_id.to_ne_bytes())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use wasmer_vfs::{FileSystem, OpenOptionsConfig};

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
                .open(format!("/file_{}", i))?;
            file.write_all(b"Hello, World!")?;
            file.flush()?;
        }
        let elapsed = start.elapsed();
        println!("Elapsed: {:?}", elapsed);

        let start = std::time::Instant::now();
        for i in 0..1000 {
            let path = PathBuf::from(format!("/dir_{i}"));
            vfs.create_dir(&path)?;
        }
        let elapsed = start.elapsed();
        println!("Elapsed: {:?}", elapsed);

        Ok(())
    }
}
