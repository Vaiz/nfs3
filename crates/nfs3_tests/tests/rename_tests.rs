use nfs3_tests::TestContext;
use nfs3_types::nfs3::{Nfs3Result, RENAME3args, diropargs3, nfsstat3};

#[tokio::test]
async fn test_rename_in_same_folder() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let create = client
        .just_create(root.clone(), "old_name", b"hello world")
        .await
        .unwrap();
    tracing::info!("Created file for rename: {create:?}");

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
        .await?
        .unwrap();

    tracing::info!("{rename:?}");
    let old_lookup = client.just_lookup(root.clone(), "old_name").await;
    assert!(matches!(old_lookup, Err(nfsstat3::NFS3ERR_NOENT)));
    let _ = client.just_lookup(root.clone(), "new_name").await.unwrap();

    client.shutdown().await
}

#[tokio::test]
async fn test_rename_noent() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let rename = client
        .rename(RENAME3args {
            from: diropargs3 {
                dir: root.clone(),
                name: b"nonexistent_file".as_slice().into(),
            },
            to: diropargs3 {
                dir: root.clone(),
                name: b"new_name".as_slice().into(),
            },
        })
        .await?;

    tracing::info!("{rename:?}");
    if !matches!(rename, Nfs3Result::Err((nfsstat3::NFS3ERR_NOENT, _))) {
        panic!("Expected NFS3ERR_NOENT error");
    }

    client.shutdown().await
}

#[tokio::test]
async fn test_rename_target_file_exists() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let _ = client
        .just_create(root.clone(), "src_file", b"hello world")
        .await
        .unwrap();

    let _ = client
        .just_create(root.clone(), "dst_file", b"bye bye world")
        .await
        .unwrap();

    let _rename = client
        .rename(RENAME3args {
            from: diropargs3 {
                dir: root.clone(),
                name: b"src_file".as_slice().into(),
            },
            to: diropargs3 {
                dir: root.clone(),
                name: b"dst_file".as_slice().into(),
            },
        })
        .await?
        .unwrap();

    let old_lookup = client.just_lookup(root.clone(), "src_file").await;
    assert!(matches!(old_lookup, Err(nfsstat3::NFS3ERR_NOENT)));
    let handle = client.just_lookup(root.clone(), "dst_file").await.unwrap();
    let read = client.just_read(handle).await.unwrap();
    assert_eq!(read, b"hello world");

    client.shutdown().await
}

#[tokio::test]
async fn test_rename_directory() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let _ = client
        .just_mkdir(root.clone(), "dir_to_rename")
        .await
        .unwrap();

    let rename = client
        .rename(RENAME3args {
            from: diropargs3 {
                dir: root.clone(),
                name: b"dir_to_rename".as_slice().into(),
            },
            to: diropargs3 {
                dir: root.clone(),
                name: b"renamed_dir".as_slice().into(),
            },
        })
        .await?
        .unwrap();

    tracing::info!("{rename:?}");

    let old_lookup = client.just_lookup(root.clone(), "dir_to_rename").await;
    assert!(matches!(old_lookup, Err(nfsstat3::NFS3ERR_NOENT)));

    let _ = client
        .just_lookup(root.clone(), "renamed_dir")
        .await
        .unwrap();

    client.shutdown().await
}

#[tokio::test]
async fn test_rename_directory_over_existing_empty_directory() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let _ = client.just_mkdir(root.clone(), "src_dir").await.unwrap();
    let _ = client.just_mkdir(root.clone(), "dst_dir").await.unwrap();

    let rename = client
        .rename(RENAME3args {
            from: diropargs3 {
                dir: root.clone(),
                name: b"src_dir".as_slice().into(),
            },
            to: diropargs3 {
                dir: root.clone(),
                name: b"dst_dir".as_slice().into(),
            },
        })
        .await?
        .unwrap();

    tracing::info!("{rename:?}");

    let old_lookup = client.just_lookup(root.clone(), "src_dir").await;
    assert!(matches!(old_lookup, Err(nfsstat3::NFS3ERR_NOENT)));

    let _ = client.just_lookup(root.clone(), "dst_dir").await.unwrap();

    client.shutdown().await
}

#[tokio::test]
async fn test_rename_directory_over_existing_nonempty_directory() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let _src = client.just_mkdir(root.clone(), "src_dir").await.unwrap();
    let dst = client.just_mkdir(root.clone(), "dst_dir").await.unwrap();
    let _file = client
        .just_create(dst.clone(), "file.txt", b"")
        .await
        .unwrap();

    let rename = client
        .rename(RENAME3args {
            from: diropargs3 {
                dir: root.clone(),
                name: b"src_dir".as_slice().into(),
            },
            to: diropargs3 {
                dir: root.clone(),
                name: b"dst_dir".as_slice().into(),
            },
        })
        .await?;

    tracing::info!("{rename:?}");
    assert!(
        matches!(rename, Nfs3Result::Err((nfsstat3::NFS3ERR_NOTEMPTY, _))),
        "Expected NFS3ERR_NOTEMPTY error"
    );

    client.shutdown().await
}

#[tokio::test]
async fn test_rename_directory_to_self() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();
    let dir = client.just_mkdir(root.clone(), "dir_self").await.unwrap();

    let rename = client
        .rename(RENAME3args {
            from: diropargs3 {
                dir: root.clone(),
                name: b"dir_self".as_slice().into(),
            },
            to: diropargs3 {
                dir: root.clone(),
                name: b"dir_self".as_slice().into(),
            },
        })
        .await?
        .unwrap();

    tracing::info!("{rename:?}");

    // Directory should still exist
    let lookup = client.just_lookup(root.clone(), "dir_self").await.unwrap();
    assert_eq!(lookup, dir);

    client.shutdown().await
}
