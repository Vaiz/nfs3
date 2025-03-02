use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use nfs3_server::test_reexports::{
    RPCContext, SocketMessageHandler, TransactionTracker, write_fragment,
};
use nfs3_server::vfs::{DirEntry, NFSFileSystem, ReadDirResult, VFSCapabilities};
use nfs3_types::nfs3::{
    self as nfs, fattr3, fileid3, filename3, ftype3, nfs_fh3, nfspath3, nfsstat3, nfstime3, sattr3,
    specdata3,
};
use tokio::io::AsyncWriteExt;
use tracing::{debug, error};

use crate::io::MockChannel;
use crate::server;

#[derive(Debug, Clone)]
enum FSContents {
    File(Vec<u8>),
    Directory(Vec<fileid3>),
}
#[allow(dead_code)]
#[derive(Debug)]
struct FSEntry {
    id: fileid3,
    attr: fattr3,
    name: filename3<'static>,
    parent: fileid3,
    contents: FSContents,
}

fn make_file(name: filename3<'static>, id: fileid3, parent: fileid3, contents: &[u8]) -> FSEntry {
    let attr = fattr3 {
        type_: ftype3::NF3REG,
        mode: 0o755,
        nlink: 1,
        uid: 507,
        gid: 507,
        size: contents.len() as u64,
        used: contents.len() as u64,
        rdev: specdata3::default(),
        fsid: 0,
        fileid: id,
        atime: nfstime3::default(),
        mtime: nfstime3::default(),
        ctime: nfstime3::default(),
    };
    FSEntry {
        id,
        attr,
        name,
        parent,
        contents: FSContents::File(contents.to_vec()),
    }
}

fn make_dir(
    name: filename3<'static>,
    id: fileid3,
    parent: fileid3,
    contents: Vec<fileid3>,
) -> FSEntry {
    let attr = fattr3 {
        type_: ftype3::NF3DIR,
        mode: 0o777,
        nlink: 1,
        uid: 507,
        gid: 507,
        size: 0,
        used: 0,
        rdev: specdata3::default(),
        fsid: 0,
        fileid: id,
        atime: nfstime3::default(),
        mtime: nfstime3::default(),
        ctime: nfstime3::default(),
    };
    FSEntry {
        id,
        attr,
        name,
        parent,
        contents: FSContents::Directory(contents),
    }
}

#[derive(Debug)]
pub struct TestFs {
    fs: Mutex<Vec<FSEntry>>,
    rootdir: fileid3,
}

impl Default for TestFs {
    fn default() -> Self {
        // build the following directory structure
        // /
        // |-a.txt
        // |-b.txt
        // |-another_dir
        //      |-thisworks.txt
        //
        let entries = vec![
            make_file("".as_bytes().into(), 0, 0, &[]), // fileid 0 is special
            make_dir(
                "/".as_bytes().into(),
                1,             // current id. Must match position in entries
                1,             // parent id
                vec![2, 3, 4], // children
            ),
            make_file(
                "a.txt".as_bytes().into(),
                2, // current id
                1, // parent id
                "hello world\n".as_bytes(),
            ),
            make_file(
                "b.txt".as_bytes().into(),
                3,
                1,
                "Greetings to xet data\n".as_bytes(),
            ),
            make_dir("another_dir".as_bytes().into(), 4, 1, vec![5]),
            make_file(
                "thisworks.txt".as_bytes().into(),
                5,
                4,
                "i hope\n".as_bytes(),
            ),
        ];

        Self {
            fs: Mutex::new(entries),
            rootdir: 1,
        }
    }
}

// For this demo file system we let the handle just be the file
// there is only 1 file. a.txt.
#[async_trait]
impl NFSFileSystem for TestFs {
    fn capabilities(&self) -> VFSCapabilities {
        VFSCapabilities::ReadWrite
    }

    fn root_dir(&self) -> fileid3 {
        self.rootdir
    }

    async fn lookup(&self, dirid: fileid3, filename: &filename3) -> Result<fileid3, nfsstat3> {
        let fs = self.fs.lock().unwrap();
        let entry = fs.get(dirid as usize).ok_or(nfsstat3::NFS3ERR_NOENT)?;
        if let FSContents::File(_) = entry.contents {
            return Err(nfsstat3::NFS3ERR_NOTDIR);
        } else if let FSContents::Directory(dir) = &entry.contents {
            // if looking for dir/. its the current directory
            if filename == ".".as_bytes() {
                return Ok(dirid);
            }
            // if looking for dir/.. its the parent directory
            if filename == "..".as_bytes() {
                return Ok(entry.parent);
            }
            for i in dir {
                if let Some(f) = fs.get(*i as usize) {
                    if &f.name == filename {
                        return Ok(*i);
                    }
                }
            }
        }
        Err(nfsstat3::NFS3ERR_NOENT)
    }

    async fn getattr(&self, id: fileid3) -> Result<fattr3, nfsstat3> {
        let fs = self.fs.lock().unwrap();
        let entry = fs.get(id as usize).ok_or(nfsstat3::NFS3ERR_NOENT)?;
        Ok(entry.attr.clone())
    }

    async fn setattr(&self, id: fileid3, setattr: sattr3) -> Result<fattr3, nfsstat3> {
        let mut fs = self.fs.lock().unwrap();
        let entry = fs.get_mut(id as usize).ok_or(nfsstat3::NFS3ERR_NOENT)?;
        match setattr.atime {
            nfs::set_atime::DONT_CHANGE => {}
            nfs::set_atime::SET_TO_CLIENT_TIME(c) => {
                entry.attr.atime = c;
            }
            nfs::set_atime::SET_TO_SERVER_TIME => {
                let d = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap();
                entry.attr.atime.seconds = d.as_secs() as u32;
                entry.attr.atime.nseconds = d.subsec_nanos();
            }
        };
        match setattr.mtime {
            nfs::set_mtime::DONT_CHANGE => {}
            nfs::set_mtime::SET_TO_CLIENT_TIME(c) => {
                entry.attr.mtime = c;
            }
            nfs::set_mtime::SET_TO_SERVER_TIME => {
                let d = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap();
                entry.attr.mtime.seconds = d.as_secs() as u32;
                entry.attr.mtime.nseconds = d.subsec_nanos();
            }
        };
        match setattr.uid {
            nfs::set_uid3::Some(u) => {
                entry.attr.uid = u;
            }
            nfs::set_uid3::None => {}
        }
        match setattr.gid {
            nfs::set_gid3::Some(u) => {
                entry.attr.gid = u;
            }
            nfs::set_gid3::None => {}
        }
        match setattr.size {
            nfs::set_size3::Some(s) => {
                entry.attr.size = s;
                entry.attr.used = s;
                if let FSContents::File(bytes) = &mut entry.contents {
                    bytes.resize(s as usize, 0);
                }
            }
            nfs::set_size3::None => {}
        }
        Ok(entry.attr.clone())
    }

    async fn read(
        &self,
        id: fileid3,
        offset: u64,
        count: u32,
    ) -> Result<(Vec<u8>, bool), nfsstat3> {
        let fs = self.fs.lock().unwrap();
        let entry = fs.get(id as usize).ok_or(nfsstat3::NFS3ERR_NOENT)?;
        if let FSContents::Directory(_) = entry.contents {
            return Err(nfsstat3::NFS3ERR_ISDIR);
        } else if let FSContents::File(bytes) = &entry.contents {
            let mut start = offset as usize;
            let mut end = offset as usize + count as usize;
            let eof = end >= bytes.len();
            if start >= bytes.len() {
                start = bytes.len();
            }
            if end > bytes.len() {
                end = bytes.len();
            }
            return Ok((bytes[start..end].to_vec(), eof));
        }
        Err(nfsstat3::NFS3ERR_NOENT)
    }
    async fn write(&self, id: fileid3, offset: u64, data: &[u8]) -> Result<fattr3, nfsstat3> {
        {
            let mut fs = self.fs.lock().unwrap();
            let mut fssize = fs[id as usize].attr.size;
            if let FSContents::File(bytes) = &mut fs[id as usize].contents {
                let offset = offset as usize;
                if offset + data.len() > bytes.len() {
                    bytes.resize(offset + data.len(), 0);
                    bytes[offset..].copy_from_slice(data);
                    fssize = bytes.len() as u64;
                }
            }
            fs[id as usize].attr.size = fssize;
            fs[id as usize].attr.used = fssize;
        }
        self.getattr(id).await
    }
    async fn create(
        &self,
        dirid: fileid3,
        filename: &filename3,
        _attr: sattr3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        let newid: fileid3;
        {
            let mut fs = self.fs.lock().unwrap();
            newid = fs.len() as fileid3;
            fs.push(make_file(
                filename.clone_to_owned(),
                newid,
                dirid,
                "".as_bytes(),
            ));
            if let FSContents::Directory(dir) = &mut fs[dirid as usize].contents {
                dir.push(newid);
            }
        }
        Ok((newid, self.getattr(newid).await.unwrap()))
    }

    async fn create_exclusive(
        &self,
        _dirid: fileid3,
        _filename: &filename3,
    ) -> Result<fileid3, nfsstat3> {
        Err(nfsstat3::NFS3ERR_NOTSUPP)
    }

    #[allow(unused)]
    async fn mkdir(
        &self,
        dirid: fileid3,
        dirname: &filename3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        let newid: fileid3;
        {
            let mut fs = self.fs.lock().unwrap();
            newid = fs.len() as fileid3;
            fs.push(make_dir(
                dirname.clone_to_owned(),
                newid,
                dirid,
                Vec::new(),
            ));
            if let FSContents::Directory(dir) = &mut fs[dirid as usize].contents {
                dir.push(newid);
            }
        }
        Ok((newid, self.getattr(newid).await.unwrap()))
    }

    /// Removes a file.
    /// If not supported dur to readonly file system
    /// this should return Err(nfsstat3::NFS3ERR_ROFS)
    #[allow(unused)]
    async fn remove(&self, dirid: fileid3, filename: &filename3) -> Result<(), nfsstat3> {
        return Err(nfsstat3::NFS3ERR_NOTSUPP);
    }

    /// Removes a file.
    /// If not supported dur to readonly file system
    /// this should return Err(nfsstat3::NFS3ERR_ROFS)
    #[allow(unused)]
    async fn rename(
        &self,
        from_dirid: fileid3,
        from_filename: &filename3,
        to_dirid: fileid3,
        to_filename: &filename3,
    ) -> Result<(), nfsstat3> {
        return Err(nfsstat3::NFS3ERR_NOTSUPP);
    }

    async fn readdir(
        &self,
        dirid: fileid3,
        start_after: fileid3,
        max_entries: usize,
    ) -> Result<ReadDirResult<'static>, nfsstat3> {
        let fs = self.fs.lock().unwrap();
        let entry = fs.get(dirid as usize).ok_or(nfsstat3::NFS3ERR_NOENT)?;
        if let FSContents::File(_) = entry.contents {
            return Err(nfsstat3::NFS3ERR_NOTDIR);
        } else if let FSContents::Directory(dir) = &entry.contents {
            let mut ret = ReadDirResult {
                entries: Vec::new(),
                end: false,
            };
            let mut start_index = 0;
            if start_after > 0 {
                if let Some(pos) = dir.iter().position(|&r| r == start_after) {
                    start_index = pos + 1;
                } else {
                    return Err(nfsstat3::NFS3ERR_BAD_COOKIE);
                }
            }
            let remaining_length = dir.len() - start_index;

            for i in dir[start_index..].iter() {
                ret.entries.push(DirEntry {
                    fileid: *i,
                    name: fs[(*i) as usize].name.clone_to_owned(),
                    attr: fs[(*i) as usize].attr.clone(),
                });
                if ret.entries.len() >= max_entries {
                    break;
                }
            }
            if ret.entries.len() == remaining_length {
                ret.end = true;
            }
            return Ok(ret);
        }
        Err(nfsstat3::NFS3ERR_NOENT)
    }

    async fn symlink(
        &self,
        _dirid: fileid3,
        _linkname: &filename3,
        _symlink: &nfspath3,
        _attr: &sattr3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        Err(nfsstat3::NFS3ERR_ROFS)
    }
    async fn readlink(&self, _id: fileid3) -> Result<nfspath3, nfsstat3> {
        return Err(nfsstat3::NFS3ERR_NOTSUPP);
    }
}

pub struct Server {
    context: RPCContext,
    mock_channel: MockChannel,
}

impl Server {
    pub fn new(mock_channel: MockChannel) -> Self {
        let test_fs = Arc::new(server::TestFs::default());

        let context = RPCContext {
            local_port: 2049,
            client_addr: "localhost".to_string(),
            auth: nfs3_types::rpc::auth_unix::default(),
            vfs: test_fs,
            mount_signal: None,
            export_name: Arc::new("/mnt".to_string()),
            transaction_tracker: Arc::new(TransactionTracker::new(Duration::from_secs(60))),
        };

        Self {
            context,
            mock_channel,
        }
    }

    pub fn root_dir(&self) -> nfs_fh3 {
        self.context.vfs.id_to_fh(self.context.vfs.root_dir())
    }

    pub async fn run(self) -> Result<(), anyhow::Error> {
        let (mut message_handler, mut socksend, mut msgrecvchan) =
            SocketMessageHandler::new(&self.context);

        tokio::spawn(async move {
            loop {
                if let Err(e) = message_handler.read().await {
                    debug!("Message handling closed: {e}");
                    break;
                }
            }
        });

        let mut mock_channel = self.mock_channel;
        loop {
            tokio::select! {
                result = mock_channel.pop_buf() => {
                    match result {
                        Ok(buf) => {
                            let _ = socksend.write_all(&buf[..]).await;
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            continue;
                        }
                        Err(e) => {
                            debug!("Message handling closed: {e}");
                            return Ok(());
                        }
                    }

                },
                reply = msgrecvchan.recv() => {
                    match reply {
                        Some(Err(e)) => {
                            debug!("Message handling closed: {e}");
                            return Err(e);
                        }
                        Some(Ok(msg)) => {
                            if let Err(e) = write_fragment(&mut mock_channel, &msg).await {
                                error!("Write error {e}");
                            }
                        }
                        None => {
                            return Err(anyhow::anyhow!("Unexpected socket context termination"));
                        }
                    }
                }
            }
        }
    }
}
