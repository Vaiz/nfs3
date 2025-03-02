use nfs3_tests::TestContext;
use nfs3_types::nfs3::*;

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

    let getattr = client.getattr(GETATTR3args { object: root.clone() }).await?;
    tracing::info!("{getattr:?}");

    client.shutdown().await
}
/*
#[tokio::test]
async fn test_setattr() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let setattr = client.setattr(SETATTR3args {
        object: root.clone(),
        new_attributes: Default::default(),
        guard: None,
    }).await?;
    tracing::info!("{setattr:?}");

    client.shutdown().await
}
*/
#[tokio::test]
async fn test_access() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let access = client.access(ACCESS3args {
        object: root.clone(),
        access: 0,
    }).await?;
    tracing::info!("{access:?}");

    client.shutdown().await
}

#[tokio::test]
async fn test_readlink() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let readlink = client.readlink(READLINK3args { symlink: root.clone() }).await?;
    tracing::info!("{readlink:?}");

    client.shutdown().await
}

#[tokio::test]
async fn test_read() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let read = client.read(READ3args {
        file: root.clone(),
        offset: 0,
        count: 1024,
    }).await?;
    tracing::info!("{read:?}");

    client.shutdown().await
}
/*
#[tokio::test]
async fn test_write() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let write = client.write(WRITE3args {
        file: root.clone(),
        offset: 0,
        count: 1024,
        stable: 0,
        data: vec![0; 1024].into(),
    }).await?;
    tracing::info!("{write:?}");

    client.shutdown().await
}

#[tokio::test]
async fn test_create() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let create = client.create(CREATE3args {
        where_: diropargs3 {
            dir: root.clone(),
            name: b"new_file".as_slice().into(),
        },
        how: Default::default(),
    }).await?;
    tracing::info!("{create:?}");

    client.shutdown().await
}

#[tokio::test]
async fn test_mkdir() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let mkdir = client.mkdir(MKDIR3args {
        where_: diropargs3 {
            dir: root.clone(),
            name: b"new_dir".as_slice().into(),
        },
        attributes: Default::default(),
    }).await?;
    tracing::info!("{mkdir:?}");

    client.shutdown().await
}

#[tokio::test]
async fn test_symlink() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let symlink = client.symlink(SYMLINK3args {
        where_: diropargs3 {
            dir: root.clone(),
            name: b"new_symlink".as_slice().into(),
        },
        symlink_data: Default::default(),
    }).await?;
    tracing::info!("{symlink:?}");

    client.shutdown().await
}

#[tokio::test]
async fn test_mknod() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let mknod = client.mknod(MKNOD3args {
        where_: diropargs3 {
            dir: root.clone(),
            name: b"new_node".as_slice().into(),
        },
        what: Default::default(),
    }).await?;
    tracing::info!("{mknod:?}");

    client.shutdown().await
}
*/
#[tokio::test]
async fn test_remove() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let remove = client.remove(REMOVE3args {
        object: diropargs3 {
            dir: root.clone(),
            name: b"file_to_remove".as_slice().into(),
        },
    }).await?;
    tracing::info!("{remove:?}");

    client.shutdown().await
}

#[tokio::test]
async fn test_rmdir() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let rmdir = client.rmdir(RMDIR3args {
        object: diropargs3 {
            dir: root.clone(),
            name: b"dir_to_remove".as_slice().into(),
        },
    }).await?;
    tracing::info!("{rmdir:?}");

    client.shutdown().await
}

#[tokio::test]
async fn test_rename() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let rename = client.rename(RENAME3args {
        from: diropargs3 {
            dir: root.clone(),
            name: b"old_name".as_slice().into(),
        },
        to: diropargs3 {
            dir: root.clone(),
            name: b"new_name".as_slice().into(),
        },
    }).await?;
    tracing::info!("{rename:?}");

    client.shutdown().await
}

#[tokio::test]
async fn test_link() -> Result<(), anyhow::Error> {
    use nfs3_client::error::*;

    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let link = client.link(LINK3args {
        file: root.clone(),
        link: diropargs3 {
            dir: root.clone(),
            name: b"new_link".as_slice().into(),
        },
    }).await;
    
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

    let readdir = client.readdir(READDIR3args {
        dir: root.clone(),
        cookie: 0,
        cookieverf: cookieverf3::default(),
        count: 1024,
    }).await?;
    tracing::info!("{readdir:?}");

    client.shutdown().await
}

#[tokio::test]
async fn test_readdirplus() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let readdirplus = client.readdirplus(READDIRPLUS3args {
        dir: root.clone(),
        cookie: 0,
        cookieverf: cookieverf3::default(),
        dircount: 1024,
        maxcount: 1024,
    }).await?;
    tracing::info!("{readdirplus:?}");

    client.shutdown().await
}

#[tokio::test]
async fn test_fsstat() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let fsstat = client.fsstat(FSSTAT3args { fsroot: root.clone() }).await?;
    tracing::info!("{fsstat:?}");

    client.shutdown().await
}

#[tokio::test]
async fn test_fsinfo() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let fsinfo = client.fsinfo(FSINFO3args { fsroot: root.clone() }).await?;
    tracing::info!("{fsinfo:?}");

    client.shutdown().await
}

#[tokio::test]
async fn test_pathconf() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let pathconf = client.pathconf(PATHCONF3args { object: root.clone() }).await?;
    tracing::info!("{pathconf:?}");

    client.shutdown().await
}

#[tokio::test]
async fn test_commit() -> Result<(), anyhow::Error> {
    use nfs3_client::error::*;

    let mut client = TestContext::setup().await;
    let root = client.root_dir().clone();

    let commit = client.commit(COMMIT3args {
        file: root.clone(),
        offset: 0,
        count: 1024,
    }).await;
    
    if let Err(Error::Rpc(RpcError::ProcUnavail)) = commit {
        tracing::info!("Server does not support COMMIT yet");
    } else {
        panic!("Expected ProcUnavail error");        
    }

    client.shutdown().await
}
