use bytes::Bytes;
use nfs3_client::Nfs3Client;
use nfs3_client::nfs3_types::nfs3::*;
use nfs3_client::nfs3_types::xdr_codec::Opaque;
use nfs3_client::tokio::TokioIo;
use nfs3_server::memfs::{MemFs, MemFsConfig};
use nfs3_server::vfs::adapters::ReadOnlyAdapter;
use nfs3_server::vfs::{FileHandleU64, NfsReadFileSystem, ReadDirPlusIterator};
use nfs3_tests::{JustClientExt, Server, TestContext};

#[tokio::test]
async fn lookup_root() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    client.null().await?;
    let lookup = client
        .lookup(&LOOKUP3args {
            what: diropargs3 {
                dir: root.clone(),
                name: b".".as_slice().into(),
            },
        })
        .await?
        .unwrap();

    tracing::info!("{lookup:?}");
    assert_eq!(lookup.object, root);

    client.shutdown().await
}

#[tokio::test]
async fn test_getattr() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let getattr = client
        .getattr(&GETATTR3args {
            object: root.clone(),
        })
        .await?
        .unwrap();
    tracing::info!("{getattr:?}");

    client.shutdown().await
}

#[tokio::test]
async fn test_getattr_bad_handle() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let invalid_handle = nfs_fh3::default();

    let getattr = client
        .getattr(&GETATTR3args {
            object: invalid_handle,
        })
        .await?;

    tracing::info!("{getattr:?}");
    assert!(
        matches!(getattr, Nfs3Result::Err((nfsstat3::NFS3ERR_BADHANDLE, _))),
        "Expected NFS3ERR_BADHANDLE error"
    );

    client.shutdown().await
}

#[tokio::test]
async fn test_setattr() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let object = client.just_lookup(&root, "a.txt").await.unwrap();

    let setattr = client
        .setattr(&SETATTR3args {
            object,
            new_attributes: sattr3::default(),
            guard: Nfs3Option::None,
        })
        .await?;
    tracing::info!("{setattr:?}");

    client.shutdown().await
}

#[tokio::test]
async fn test_access() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let access = client
        .access(&ACCESS3args {
            object: root.clone(),
            access: 0,
        })
        .await?
        .unwrap();
    tracing::info!("{access:?}");

    client.shutdown().await
}

#[tokio::test]
async fn test_readlink() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let readlink = client
        .readlink(&READLINK3args {
            symlink: root.clone(),
        })
        .await?;

    tracing::info!("{readlink:?}");
    assert!(
        matches!(readlink, Nfs3Result::Err((nfsstat3::NFS3ERR_NOTSUPP, _))),
        "Expected NFS3ERR_NOTSUPP error"
    );

    client.shutdown().await
}

#[tokio::test]
async fn test_read_dir_as_file() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let read = client
        .read(&READ3args {
            file: root.clone(),
            offset: 0,
            count: 1024,
        })
        .await?;

    tracing::info!("{read:?}");
    assert!(
        matches!(read, Nfs3Result::Err((nfsstat3::NFS3ERR_ISDIR, _))),
        "Expected NFS3ERR_ISDIR error"
    );

    client.shutdown().await
}

struct CustomReadLimit {
    fs: MemFs,
    rtmax: u32,
}

impl NfsReadFileSystem for CustomReadLimit {
    type Handle = FileHandleU64;

    fn root_dir(&self) -> Self::Handle {
        self.fs.root_dir()
    }

    async fn lookup(
        &self,
        dirid: &Self::Handle,
        filename: &filename3<'_>,
    ) -> Result<Self::Handle, nfsstat3> {
        self.fs.lookup(dirid, filename).await
    }

    async fn getattr(&self, id: &Self::Handle) -> Result<fattr3, nfsstat3> {
        self.fs.getattr(id).await
    }

    async fn read(
        &self,
        id: &Self::Handle,
        offset: u64,
        count: u32,
    ) -> Result<(Bytes, bool), nfsstat3> {
        self.fs.read(id, offset, count).await
    }

    async fn readdirplus(
        &self,
        dirid: &Self::Handle,
        cookie: u64,
    ) -> Result<impl ReadDirPlusIterator<Self::Handle>, nfsstat3> {
        self.fs.readdirplus(dirid, cookie).await
    }

    async fn readlink(&self, id: &Self::Handle) -> Result<nfspath3<'_>, nfsstat3> {
        self.fs.readlink(id).await
    }

    async fn fsinfo(&self, root_fileid: &Self::Handle) -> Result<FSINFO3resok, nfsstat3> {
        if *root_fileid != self.root_dir() {
            return Err(nfsstat3::NFS3ERR_INVAL);
        }
        let mut info = self.fs.fsinfo(root_fileid).await?;
        info.rtmax = self.rtmax;
        info.rtpref = self.rtmax;
        info.rtmult = 1;
        Ok(info)
    }
}

#[tokio::test]
async fn read_count_respects_root_fsinfo_limits() -> anyhow::Result<()> {
    for rtmax in [128, 1024 * 1024 + 1] {
        check_custom_read_limit(rtmax).await?;
    }
    Ok(())
}

async fn check_custom_read_limit(rtmax: u32) -> anyhow::Result<()> {
    let mut contents = vec![b'a'; rtmax as usize];
    contents.push(b'z');
    let mut config = MemFsConfig::default();
    config.add_file("/large.txt", contents);
    let fs = ReadOnlyAdapter::new(CustomReadLimit {
        fs: MemFs::new(config).expect("initialize fixture filesystem"),
        rtmax,
    });
    let (server_io, client_io) = tokio::io::duplex(1024 * 1024);
    let server = Server::new(server_io, fs)?;
    let root = server.root_dir();
    let server_task = tokio::spawn(server.run());
    let mut client = Nfs3Client::new(TokioIo::new(client_io));

    let info = client
        .fsinfo(&FSINFO3args {
            fsroot: root.clone(),
        })
        .await?
        .unwrap();
    assert_eq!((info.rtmax, info.rtpref, info.rtmult), (rtmax, rtmax, 1));

    let file = client
        .lookup(&LOOKUP3args {
            what: diropargs3 {
                dir: root,
                name: b"large.txt".as_slice().into(),
            },
        })
        .await?
        .unwrap()
        .object;
    let first = client
        .read(&READ3args {
            file: file.clone(),
            offset: 0,
            count: rtmax + 1,
        })
        .await?
        .unwrap();
    assert_eq!(first.count, rtmax);
    assert_eq!(first.data.len(), rtmax as usize);
    assert!(!first.eof);
    assert_eq!(first.file_attributes.unwrap().size, u64::from(rtmax) + 1);

    let last = client
        .read(&READ3args {
            file,
            offset: u64::from(first.count),
            count: rtmax,
        })
        .await?
        .unwrap();
    assert_eq!(last.count, 1);
    assert_eq!(last.data.as_ref(), b"z");
    assert!(last.eof);

    drop(client);
    server_task.await?
}

#[tokio::test]
async fn test_read_file() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let LOOKUP3resok {
        object,
        obj_attributes,
        ..
    } = client
        .lookup(&LOOKUP3args {
            what: diropargs3 {
                dir: root.clone(),
                name: b"a.txt".as_slice().into(),
            },
        })
        .await?
        .unwrap();

    let read = client
        .read(&READ3args {
            file: object,
            offset: 0,
            count: 1024,
        })
        .await?
        .unwrap();

    tracing::info!("{read:?}");
    let expected_len = obj_attributes.unwrap().size.min(1024) as usize;
    assert_eq!(read.data.len(), expected_len);
    assert_eq!(read.data.len(), expected_len);

    client.shutdown().await
}

#[tokio::test]
async fn test_write() -> Result<(), anyhow::Error> {
    const COUNT: usize = 1024;
    let count = u32::try_from(COUNT)?;

    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let create = client
        .create(&CREATE3args {
            where_: diropargs3 {
                dir: root.clone(),
                name: b"new_file.txt".as_slice().into(),
            },
            how: createhow3::UNCHECKED(sattr3::default()),
        })
        .await?
        .unwrap();

    let file_handle = create.obj.unwrap();
    let write = client
        .write(&WRITE3args {
            file: file_handle.clone(),
            offset: 0,
            count,
            stable: stable_how::DATA_SYNC,
            data: Opaque::owned(vec![0u8; COUNT]),
        })
        .await?
        .unwrap();

    tracing::info!("{write:?}");
    assert_eq!(write.count, count);
    // MemFs always commits synchronously regardless of the requested stable level.
    assert_eq!(write.committed, stable_how::FILE_SYNC);

    // Additional check to verify the file was written correctly
    let read = client
        .read(&READ3args {
            file: file_handle.clone(),
            offset: 0,
            count,
        })
        .await?
        .unwrap();
    assert_eq!(read.data.len(), COUNT);
    assert_eq!(read.data.as_ref(), &[0u8; COUNT]);

    client.shutdown().await
}

#[tokio::test]
async fn test_create_unchecked() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let create = client
        .create(&CREATE3args {
            where_: diropargs3 {
                dir: root.clone(),
                name: b"new_file.txt".as_slice().into(),
            },
            how: createhow3::UNCHECKED(sattr3::default()),
        })
        .await?
        .unwrap();
    tracing::info!("{create:?}");

    // Additional check to verify the file was created
    let lookup = client.just_lookup(&root, "new_file.txt").await.unwrap();
    assert_eq!(lookup, create.obj.unwrap());

    client.shutdown().await
}

#[tokio::test]
async fn test_create_guarded() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let create = client
        .create(&CREATE3args {
            where_: diropargs3 {
                dir: root.clone(),
                name: b"new_file.txt".as_slice().into(),
            },
            how: createhow3::GUARDED(sattr3::default()),
        })
        .await?
        .unwrap();

    tracing::info!("{create:?}");

    // Additional check to verify the file was created
    let lookup = client.just_lookup(&root, "new_file.txt").await.unwrap();
    assert_eq!(lookup, create.obj.unwrap());

    client.shutdown().await
}

#[tokio::test]
async fn test_create_exclusive() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let _create = client
        .create(&CREATE3args {
            where_: diropargs3 {
                dir: root.clone(),
                name: b"new_file.txt".as_slice().into(),
            },
            how: createhow3::EXCLUSIVE(createverf3(1234u64.to_ne_bytes())),
        })
        .await?
        .unwrap();

    let create = client
        .create(&CREATE3args {
            where_: diropargs3 {
                dir: root.clone(),
                name: b"new_file.txt".as_slice().into(),
            },
            how: createhow3::EXCLUSIVE(createverf3(5678u64.to_ne_bytes())),
        })
        .await?;

    assert!(
        matches!(create, Nfs3Result::Err((nfsstat3::NFS3ERR_EXIST, _))),
        "Expected NFS3ERR_EXIST error"
    );

    // recreate the file with the same verifier, should succeed
    let _create = client
        .create(&CREATE3args {
            where_: diropargs3 {
                dir: root.clone(),
                name: b"new_file.txt".as_slice().into(),
            },
            how: createhow3::EXCLUSIVE(createverf3(1234u64.to_ne_bytes())),
        })
        .await?
        .unwrap();

    client.shutdown().await
}

#[tokio::test]
async fn test_mkdir() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let mkdir = client
        .mkdir(&MKDIR3args {
            where_: diropargs3 {
                dir: root.clone(),
                name: b"new_dir".as_slice().into(),
            },
            attributes: sattr3::default(),
        })
        .await?
        .unwrap();

    tracing::info!("{mkdir:?}");

    // Additional check to verify the directory was created
    let lookup = client.just_lookup(&root, "new_dir").await.unwrap();
    assert_eq!(lookup, mkdir.obj.unwrap());

    client.shutdown().await
}

#[tokio::test]
async fn test_symlink() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let symlink = client
        .symlink(&SYMLINK3args {
            where_: diropargs3 {
                dir: root.clone(),
                name: b"new_symlink".as_slice().into(),
            },
            symlink: symlinkdata3 {
                symlink_attributes: sattr3::default(),
                symlink_data: b"target".to_vec().into(),
            },
        })
        .await?;

    tracing::info!("{symlink:?}");
    assert!(
        matches!(symlink, Nfs3Result::Err((nfsstat3::NFS3ERR_NOTSUPP, _))),
        "Expected NFS3ERR_NOTSUPP error"
    );

    client.shutdown().await
}

#[tokio::test]
async fn test_mknod() -> Result<(), anyhow::Error> {
    use nfs3_client::RpcError;

    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let mknod = client
        .mknod(&MKNOD3args {
            where_: diropargs3 {
                dir: root.clone(),
                name: b"new_node".as_slice().into(),
            },
            what: mknoddata3::NF3FIFO(sattr3::default()),
        })
        .await;

    tracing::info!("{mknod:?}");
    assert!(
        matches!(mknod, Err(RpcError::ProcUnavail)),
        "Expected ProcUnavail error"
    );

    client.shutdown().await
}

#[tokio::test]
async fn test_remove() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let remove = client
        .remove(&REMOVE3args {
            object: diropargs3 {
                dir: root.clone(),
                name: b"a.txt".as_slice().into(),
            },
        })
        .await?
        .unwrap();

    tracing::info!("{remove:?}");
    client.shutdown().await
}

#[tokio::test]
async fn test_remove_noent() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let remove = client
        .remove(&REMOVE3args {
            object: diropargs3 {
                dir: root.clone(),
                name: b"file_to_remove".as_slice().into(),
            },
        })
        .await?;

    tracing::info!("{remove:?}");
    assert!(
        matches!(remove, Nfs3Result::Err((nfsstat3::NFS3ERR_NOENT, _))),
        "Expected NFS3ERR_NOENT error"
    );

    client.shutdown().await
}

#[tokio::test]
async fn test_rmdir_noent() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let rmdir = client
        .rmdir(&RMDIR3args {
            object: diropargs3 {
                dir: root.clone(),
                name: b"dir_to_remove".as_slice().into(),
            },
        })
        .await?;

    tracing::info!("{rmdir:?}");
    assert!(
        matches!(rmdir, Nfs3Result::Err((nfsstat3::NFS3ERR_NOENT, _))),
        "Expected NFS3ERR_NOENT error"
    );

    client.shutdown().await
}

#[tokio::test]
async fn test_rmdir_notempty() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let rmdir = client
        .rmdir(&RMDIR3args {
            object: diropargs3 {
                dir: root.clone(),
                name: b"another_dir".as_slice().into(),
            },
        })
        .await?;

    tracing::info!("{rmdir:?}");
    assert!(
        matches!(rmdir, Nfs3Result::Err((nfsstat3::NFS3ERR_NOTEMPTY, _))),
        "Expected NFS3ERR_NOTEMPTY error"
    );

    client.shutdown().await
}

#[tokio::test]
async fn test_rmdir() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let _ = client
        .mkdir(&MKDIR3args {
            where_: diropargs3 {
                dir: root.clone(),
                name: b"test_dir".as_slice().into(),
            },
            attributes: sattr3::default(),
        })
        .await?
        .unwrap();

    let rmdir = client
        .rmdir(&RMDIR3args {
            object: diropargs3 {
                dir: root.clone(),
                name: b"test_dir".as_slice().into(),
            },
        })
        .await?
        .unwrap();

    tracing::info!("{rmdir:?}");
    client.shutdown().await
}

#[tokio::test]
async fn test_link() -> Result<(), anyhow::Error> {
    use nfs3_client::RpcError;

    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let link = client
        .link(&LINK3args {
            file: root.clone(),
            link: diropargs3 {
                dir: root.clone(),
                name: b"new_link".as_slice().into(),
            },
        })
        .await;

    assert!(
        matches!(link, Err(RpcError::ProcUnavail)),
        "Expected ProcUnavail error"
    );

    client.shutdown().await
}

#[tokio::test]
async fn test_readdir() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let readdir = client
        .readdir(&READDIR3args {
            dir: root.clone(),
            cookie: 0,
            cookieverf: cookieverf3::default(),
            count: 1024 * 1024,
        })
        .await?
        .unwrap();

    tracing::info!("{readdir:?}");
    client.shutdown().await
}

#[tokio::test]
async fn test_readdir_too_small() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let readdir = client
        .readdir(&READDIR3args {
            dir: root.clone(),
            cookie: 0,
            cookieverf: cookieverf3::default(),
            count: 10,
        })
        .await?;

    tracing::info!("{readdir:?}");
    assert!(
        matches!(readdir, Nfs3Result::Err((nfsstat3::NFS3ERR_TOOSMALL, _))),
        "Expected NFS3ERR_TOOSMALL error"
    );

    client.shutdown().await
}

#[tokio::test]
async fn test_readdirplus() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let readdirplus = client
        .readdirplus(&READDIRPLUS3args {
            dir: root.clone(),
            cookie: 0,
            cookieverf: cookieverf3::default(),
            dircount: 1024 * 1024,
            maxcount: 1024 * 1024,
        })
        .await?
        .unwrap();

    tracing::info!("{readdirplus:?}");
    client.shutdown().await
}

#[tokio::test]
async fn test_readdirplus_dircount_too_small() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let readdirplus = client
        .readdirplus(&READDIRPLUS3args {
            dir: root.clone(),
            cookie: 0,
            cookieverf: cookieverf3::default(),
            dircount: 10,
            maxcount: 1024 * 1024,
        })
        .await?;

    tracing::info!("{readdirplus:?}");
    assert!(
        matches!(
            readdirplus,
            Nfs3Result::Err((nfsstat3::NFS3ERR_TOOSMALL, _))
        ),
        "Expected NFS3ERR_TOOSMALL error"
    );

    client.shutdown().await
}

#[tokio::test]
async fn test_readdirplus_maxcount_too_small() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let readdirplus = client
        .readdirplus(&READDIRPLUS3args {
            dir: root.clone(),
            cookie: 0,
            cookieverf: cookieverf3::default(),
            dircount: 1024 * 1024,
            maxcount: 10,
        })
        .await?;

    tracing::info!("{readdirplus:?}");
    assert!(
        matches!(
            readdirplus,
            Nfs3Result::Err((nfsstat3::NFS3ERR_TOOSMALL, _))
        ),
        "Expected NFS3ERR_TOOSMALL error"
    );

    client.shutdown().await
}

#[tokio::test]
async fn test_fsstat() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let fsstat = client
        .fsstat(&FSSTAT3args {
            fsroot: root.clone(),
        })
        .await?
        .unwrap();

    tracing::info!("{fsstat:?}");
    client.shutdown().await
}

#[tokio::test]
async fn test_fsinfo() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let fsinfo = client
        .fsinfo(&FSINFO3args {
            fsroot: root.clone(),
        })
        .await?
        .unwrap();

    tracing::info!("{fsinfo:?}");
    client.shutdown().await
}

#[tokio::test]
async fn test_pathconf() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let pathconf = client
        .pathconf(&PATHCONF3args {
            object: root.clone(),
        })
        .await?
        .unwrap();

    tracing::info!("{pathconf:?}");
    client.shutdown().await
}

#[tokio::test]
async fn test_commit() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    // MemFs commits immediately; COMMIT on the root dir should succeed.
    let commit = client
        .commit(&COMMIT3args {
            file: root.clone(),
            offset: 0,
            count: 1024,
        })
        .await?
        .unwrap();

    tracing::info!("{commit:?}");

    client.shutdown().await
}

#[tokio::test]
async fn test_unstable_write_then_commit() -> Result<(), anyhow::Error> {
    const DATA: &[u8] = b"unstable write data";
    let count = u32::try_from(DATA.len())?;

    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let create = client
        .create(&CREATE3args {
            where_: diropargs3 {
                dir: root.clone(),
                name: b"unstable.txt".as_slice().into(),
            },
            how: createhow3::UNCHECKED(sattr3::default()),
        })
        .await?
        .unwrap();

    let file_handle = create.obj.unwrap();

    let write = client
        .write(&WRITE3args {
            file: file_handle.clone(),
            offset: 0,
            count,
            stable: stable_how::UNSTABLE,
            data: Opaque::borrowed(DATA),
        })
        .await?
        .unwrap();

    assert_eq!(write.count, count);
    assert_eq!(write.committed, stable_how::FILE_SYNC); // MemFs ignores the requested stable level and always commits synchronously.

    // COMMIT should succeed regardless.
    let commit = client
        .commit(&COMMIT3args {
            file: file_handle.clone(),
            offset: 0,
            count: 1024,
        })
        .await?
        .unwrap();

    tracing::info!("{commit:?}");
    assert_eq!(write.verf, commit.verf);

    let before_wcc = commit.file_wcc.before.unwrap();
    assert_eq!(before_wcc.size, DATA.len() as u64);
    let after_attr = commit.file_wcc.after.unwrap();
    assert_eq!(after_attr.type_, ftype3::NF3REG);
    assert_eq!(after_attr.size, DATA.len() as u64);

    // Data must be readable after commit.
    let read = client
        .read(&READ3args {
            file: file_handle.clone(),
            offset: 0,
            count,
        })
        .await?
        .unwrap();

    assert_eq!(read.data.as_ref(), DATA);

    client.shutdown().await
}
