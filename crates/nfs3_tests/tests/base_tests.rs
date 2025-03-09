use nfs3_tests::TestContext;
use nfs3_types::nfs3::*;
use nfs3_types::xdr_codec::Opaque;

#[tokio::test]
async fn lookup_root() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    client.null().await?;
    let lookup = client
        .lookup(LOOKUP3args {
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
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let getattr = client
        .getattr(GETATTR3args {
            object: root.clone(),
        })
        .await?
        .unwrap();
    tracing::info!("{getattr:?}");

    client.shutdown().await
}

#[tokio::test]
async fn test_setattr() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let LOOKUP3resok {
        object,
        obj_attributes: _,
        dir_attributes: _,
    } = client
        .lookup(LOOKUP3args {
            what: diropargs3 {
                dir: root.clone(),
                name: b"a.txt".as_slice().into(),
            },
        })
        .await?
        .unwrap();

    let setattr = client
        .setattr(SETATTR3args {
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
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let access = client
        .access(ACCESS3args {
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
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let readlink = client
        .readlink(READLINK3args {
            symlink: root.clone(),
        })
        .await?;

    tracing::info!("{readlink:?}");
    if matches!(readlink, Nfs3Result::Err((nfsstat3::NFS3ERR_NOTSUPP, _))) {
        tracing::info!("not supported by current implementation");
    } else {
        panic!("Expected NFS3ERR_NOTSUPP error");
    }

    client.shutdown().await
}

#[tokio::test]
async fn test_read_dir() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let read = client
        .read(READ3args {
            file: root.clone(),
            offset: 0,
            count: 1024,
        })
        .await?;

    tracing::info!("{read:?}");
    if matches!(read, Nfs3Result::Err((nfsstat3::NFS3ERR_ISDIR, _))) {
        tracing::info!("not supported by current implementation");
    } else {
        panic!("Expected NFS3ERR_NOTSUPP error");
    }

    client.shutdown().await
}

#[tokio::test]
async fn test_read_file() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let LOOKUP3resok {
        object,
        obj_attributes,
        ..
    } = client
        .lookup(LOOKUP3args {
            what: diropargs3 {
                dir: root.clone(),
                name: b"a.txt".as_slice().into(),
            },
        })
        .await?
        .unwrap();

    let read = client
        .read(READ3args {
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

    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let create = client
        .create(CREATE3args {
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
        .write(WRITE3args {
            file: file_handle.clone(),
            offset: 0,
            count: COUNT as u32,
            stable: stable_how::DATA_SYNC,
            data: Opaque::owned(vec![0u8; COUNT]),
        })
        .await?
        .unwrap();

    tracing::info!("{write:?}");
    assert_eq!(write.count, COUNT as u32);

    // Additional check to verify the file was written correctly
    let read = client
        .read(READ3args {
            file: file_handle.clone(),
            offset: 0,
            count: COUNT as u32,
        })
        .await?
        .unwrap();
    assert_eq!(read.data.len(), COUNT);
    assert_eq!(read.data.as_ref(), &[0u8; COUNT]);

    client.shutdown().await
}

#[tokio::test]
async fn test_create_unchecked() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let create = client
        .create(CREATE3args {
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
    let lookup = client
        .lookup(LOOKUP3args {
            what: diropargs3 {
                dir: root.clone(),
                name: b"new_file.txt".as_slice().into(),
            },
        })
        .await?
        .unwrap();

    assert_eq!(lookup.object, create.obj.unwrap());

    client.shutdown().await
}

#[tokio::test]
async fn test_create_guarded() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let create = client
        .create(CREATE3args {
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
    let lookup = client
        .lookup(LOOKUP3args {
            what: diropargs3 {
                dir: root.clone(),
                name: b"new_file.txt".as_slice().into(),
            },
        })
        .await?
        .unwrap();
    assert_eq!(lookup.object, create.obj.unwrap());

    client.shutdown().await
}

#[tokio::test]
async fn test_create_exclusive() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let create = client
        .create(CREATE3args {
            where_: diropargs3 {
                dir: root.clone(),
                name: b"new_file.txt".as_slice().into(),
            },
            how: createhow3::EXCLUSIVE(createverf3([0u8; 8])),
        })
        .await?;

    tracing::info!("{create:?}");
    if matches!(&create, Nfs3Result::Err((nfsstat3::NFS3ERR_NOTSUPP, _))) {
        tracing::info!("not supported by current implementation");
    } else {
        panic!("Expected NFS3ERR_NOTSUPP error");
    }

    client.shutdown().await
}

#[tokio::test]
async fn test_mkdir() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let mkdir = client
        .mkdir(MKDIR3args {
            where_: diropargs3 {
                dir: root.clone(),
                name: b"new_dir".as_slice().into(),
            },
            attributes: Default::default(),
        })
        .await?
        .unwrap();

    tracing::info!("{mkdir:?}");

    // Additional check to verify the directory was created
    let lookup = client
        .lookup(LOOKUP3args {
            what: diropargs3 {
                dir: root.clone(),
                name: b"new_dir".as_slice().into(),
            },
        })
        .await?
        .unwrap();
    assert_eq!(lookup.object, mkdir.obj.unwrap());

    client.shutdown().await
}

#[tokio::test]
async fn test_symlink() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let symlink = client
        .symlink(SYMLINK3args {
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
    if matches!(symlink, Nfs3Result::Err((nfsstat3::NFS3ERR_NOTSUPP, _))) {
        tracing::info!("not supported by current implementation yet");
    } else {
        panic!("Expected NFS3ERR_NOTSUPP error");
    }

    client.shutdown().await
}

#[tokio::test]
async fn test_mknod() -> Result<(), anyhow::Error> {
    use nfs3_client::error::*;

    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let mknod = client
        .mknod(MKNOD3args {
            where_: diropargs3 {
                dir: root.clone(),
                name: b"new_node".as_slice().into(),
            },
            what: mknoddata3::NF3FIFO(sattr3::default()),
        })
        .await;

    tracing::info!("{mknod:?}");
    if matches!(mknod, Err(Error::Rpc(RpcError::ProcUnavail))) {
        tracing::info!("not supported by nfs3_server yet");
    } else {
        panic!("Expected NFS3ERR_NOTSUPP error");
    }

    client.shutdown().await
}

#[tokio::test]
async fn test_remove() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let remove = client
        .remove(REMOVE3args {
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
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let remove = client
        .remove(REMOVE3args {
            object: diropargs3 {
                dir: root.clone(),
                name: b"file_to_remove".as_slice().into(),
            },
        })
        .await?;

    tracing::info!("{remove:?}");
    if !matches!(remove, Nfs3Result::Err((nfsstat3::NFS3ERR_NOENT, _))) {
        panic!("Expected NFS3ERR_NOENT error");
    }

    client.shutdown().await
}

#[tokio::test]
async fn test_rmdir_noent() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let rmdir = client
        .rmdir(RMDIR3args {
            object: diropargs3 {
                dir: root.clone(),
                name: b"dir_to_remove".as_slice().into(),
            },
        })
        .await?;

    tracing::info!("{rmdir:?}");
    if !matches!(rmdir, Nfs3Result::Err((nfsstat3::NFS3ERR_NOENT, _))) {
        panic!("Expected NFS3ERR_NOENT error");
    }

    client.shutdown().await
}

#[tokio::test]
async fn test_rmdir_notempty() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let rmdir = client
        .rmdir(RMDIR3args {
            object: diropargs3 {
                dir: root.clone(),
                name: b"another_dir".as_slice().into(),
            },
        })
        .await?;

    tracing::info!("{rmdir:?}");
    if !matches!(rmdir, Nfs3Result::Err((nfsstat3::NFS3ERR_NOTEMPTY, _))) {
        panic!("Expected NFS3ERR_NOTEMPTY error");
    }

    client.shutdown().await
}

#[tokio::test]
async fn test_rmdir() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let _ = client
        .mkdir(MKDIR3args {
            where_: diropargs3 {
                dir: root.clone(),
                name: b"test_dir".as_slice().into(),
            },
            attributes: Default::default(),
        })
        .await?
        .unwrap();

    let rmdir = client
        .rmdir(RMDIR3args {
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
async fn test_rename() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let rename = client
        .rename(RENAME3args {
            from: diropargs3 {
                dir: root.clone(),
                name: b"old_name".as_slice().into(),
            },
            to: diropargs3 {
                dir: root.clone(),
                name: b"new_name".as_slice().into(),
            },
        })
        .await?;

    tracing::info!("{rename:?}");
    if matches!(rename, Nfs3Result::Err((nfsstat3::NFS3ERR_NOTSUPP, _))) {
        tracing::info!("not supported by current implementation yet");
    } else {
        panic!("Expected NFS3ERR_NOTSUPP error");
    }

    client.shutdown().await
}

#[tokio::test]
async fn test_link() -> Result<(), anyhow::Error> {
    use nfs3_client::error::*;

    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let link = client
        .link(LINK3args {
            file: root.clone(),
            link: diropargs3 {
                dir: root.clone(),
                name: b"new_link".as_slice().into(),
            },
        })
        .await;

    if let Err(Error::Rpc(RpcError::ProcUnavail)) = link {
        tracing::info!("Server does not support COMMIT yet");
    } else {
        panic!("Expected ProcUnavail error");
    }

    client.shutdown().await
}

#[tokio::test]
async fn test_readdir() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let readdir = client
        .readdir(READDIR3args {
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
async fn test_readdirplus() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let readdirplus = client
        .readdirplus(READDIRPLUS3args {
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
async fn test_fsstat() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let fsstat = client
        .fsstat(FSSTAT3args {
            fsroot: root.clone(),
        })
        .await?
        .unwrap();

    tracing::info!("{fsstat:?}");
    client.shutdown().await
}

#[tokio::test]
async fn test_fsinfo() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let fsinfo = client
        .fsinfo(FSINFO3args {
            fsroot: root.clone(),
        })
        .await?
        .unwrap();

    tracing::info!("{fsinfo:?}");
    client.shutdown().await
}

#[tokio::test]
async fn test_pathconf() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let pathconf = client
        .pathconf(PATHCONF3args {
            object: root.clone(),
        })
        .await?
        .unwrap();

    tracing::info!("{pathconf:?}");
    client.shutdown().await
}

#[tokio::test]
async fn test_commit() -> Result<(), anyhow::Error> {
    use nfs3_client::error::*;

    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let commit = client
        .commit(COMMIT3args {
            file: root.clone(),
            offset: 0,
            count: 1024,
        })
        .await;

    if let Err(Error::Rpc(RpcError::ProcUnavail)) = commit {
        tracing::info!("Server does not support COMMIT yet");
    } else {
        panic!("Expected ProcUnavail error");
    }

    client.shutdown().await
}
