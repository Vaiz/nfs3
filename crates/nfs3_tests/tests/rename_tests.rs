use nfs3_client::nfs3_types::nfs3::{Nfs3Result, RENAME3args, diropargs3, nfsstat3};
use nfs3_tests::{JustClientExt, TestContext};

#[tokio::test]
async fn test_rename_in_same_folder() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let create = client
        .just_create(&root, "old_name", b"hello world")
        .await
        .unwrap();
    tracing::info!("Created file for rename: {create:?}");

    let rename = client
        .rename(&RENAME3args {
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
    let old_lookup = client.just_lookup(&root, "old_name").await;
    assert!(matches!(old_lookup, Err(nfsstat3::NFS3ERR_NOENT)));
    let _ = client.just_lookup(&root, "new_name").await.unwrap();

    client.shutdown().await
}

#[tokio::test]
async fn test_rename_noent() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let rename = client
        .rename(&RENAME3args {
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
        .just_create(&root, "src_file", b"hello world")
        .await
        .unwrap();

    let _ = client
        .just_create(&root, "dst_file", b"bye bye world")
        .await
        .unwrap();

    let _rename = client
        .rename(&RENAME3args {
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

    let old_lookup = client.just_lookup(&root, "src_file").await;
    assert!(matches!(old_lookup, Err(nfsstat3::NFS3ERR_NOENT)));
    let handle = client.just_lookup(&root, "dst_file").await.unwrap();
    let read = client.just_read(&handle).await.unwrap();
    assert_eq!(read, b"hello world");

    client.shutdown().await
}

#[tokio::test]
async fn test_rename_directory() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let _ = client.just_mkdir(&root, "dir_to_rename").await.unwrap();

    let rename = client
        .rename(&RENAME3args {
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

    let old_lookup = client.just_lookup(&root, "dir_to_rename").await;
    assert!(matches!(old_lookup, Err(nfsstat3::NFS3ERR_NOENT)));

    let _ = client.just_lookup(&root, "renamed_dir").await.unwrap();

    client.shutdown().await
}

#[tokio::test]
async fn test_rename_directory_over_existing_empty_directory() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let _ = client.just_mkdir(&root, "src_dir").await.unwrap();
    let _ = client.just_mkdir(&root, "dst_dir").await.unwrap();

    let rename = client
        .rename(&RENAME3args {
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

    let old_lookup = client.just_lookup(&root, "src_dir").await;
    assert!(matches!(old_lookup, Err(nfsstat3::NFS3ERR_NOENT)));

    let _ = client.just_lookup(&root, "dst_dir").await.unwrap();

    client.shutdown().await
}

#[tokio::test]
async fn test_rename_directory_over_existing_nonempty_directory() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let _src = client.just_mkdir(&root, "src_dir").await.unwrap();
    let dst = client.just_mkdir(&root, "dst_dir").await.unwrap();
    let _file = client.just_create(&dst, "file.txt", b"").await.unwrap();

    let rename = client
        .rename(&RENAME3args {
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
    let dir = client.just_mkdir(&root, "dir_self").await.unwrap();

    let rename = client
        .rename(&RENAME3args {
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
    let lookup = client.just_lookup(&root, "dir_self").await.unwrap();
    assert_eq!(lookup, dir);

    client.shutdown().await
}

#[tokio::test]
async fn test_rename_file_in_subdirectory() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let subdir = client.just_mkdir(&root, "subdir").await.unwrap();
    let _ = client
        .just_create(&subdir, "file_in_subdir", b"subdir content")
        .await
        .unwrap();

    let _rename = client
        .rename(&RENAME3args {
            from: diropargs3 {
                dir: subdir.clone(),
                name: b"file_in_subdir".as_slice().into(),
            },
            to: diropargs3 {
                dir: subdir.clone(),
                name: b"file_renamed".as_slice().into(),
            },
        })
        .await?
        .unwrap();

    let old_lookup = client.just_lookup(&subdir, "file_in_subdir").await;
    assert!(matches!(old_lookup, Err(nfsstat3::NFS3ERR_NOENT)));
    let handle = client.just_lookup(&subdir, "file_renamed").await.unwrap();
    let read = client.just_read(&handle).await.unwrap();
    assert_eq!(read, b"subdir content");

    client.shutdown().await
}

#[tokio::test]
async fn test_rename_file_across_directories() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let src_dir = client.just_mkdir(&root, "src_dir").await.unwrap();
    let dst_dir = client.just_mkdir(&root, "dst_dir").await.unwrap();

    let _ = client
        .just_create(&src_dir, "file_to_move", b"move me")
        .await
        .unwrap();

    let _rename = client
        .rename(&RENAME3args {
            from: diropargs3 {
                dir: src_dir.clone(),
                name: b"file_to_move".as_slice().into(),
            },
            to: diropargs3 {
                dir: dst_dir.clone(),
                name: b"file_moved".as_slice().into(),
            },
        })
        .await?
        .unwrap();

    let old_lookup = client.just_lookup(&src_dir, "file_to_move").await;
    assert!(matches!(old_lookup, Err(nfsstat3::NFS3ERR_NOENT)));
    let handle = client.just_lookup(&dst_dir, "file_moved").await.unwrap();
    let read = client.just_read(&handle).await.unwrap();
    assert_eq!(read, b"move me");

    client.shutdown().await
}

#[tokio::test]
async fn test_rename_directory_across_directories() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let src_dir = client.just_mkdir(&root, "src_dir").await.unwrap();
    let dst_dir = client.just_mkdir(&root, "dst_dir").await.unwrap();

    let _subdir = client.just_mkdir(&src_dir, "subdir").await.unwrap();

    let _rename = client
        .rename(&RENAME3args {
            from: diropargs3 {
                dir: src_dir.clone(),
                name: b"subdir".as_slice().into(),
            },
            to: diropargs3 {
                dir: dst_dir.clone(),
                name: b"subdir_moved".as_slice().into(),
            },
        })
        .await?
        .unwrap();

    let old_lookup = client.just_lookup(&src_dir, "subdir").await;
    assert!(matches!(old_lookup, Err(nfsstat3::NFS3ERR_NOENT)));
    let _ = client.just_lookup(&dst_dir, "subdir_moved").await.unwrap();

    client.shutdown().await
}

#[tokio::test]
async fn test_rename_nonempty_directory() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let src_dir = client.just_mkdir(&root, "src_nonempty_dir").await.unwrap();
    let _ = client
        .just_create(&src_dir, "file_inside.txt", b"some content")
        .await
        .unwrap();

    let rename = client
        .rename(&RENAME3args {
            from: diropargs3 {
                dir: root.clone(),
                name: b"src_nonempty_dir".as_slice().into(),
            },
            to: diropargs3 {
                dir: root.clone(),
                name: b"renamed_nonempty_dir".as_slice().into(),
            },
        })
        .await?
        .unwrap();

    tracing::info!("{rename:?}");

    let old_lookup = client.just_lookup(&root, "src_nonempty_dir").await;
    assert!(matches!(old_lookup, Err(nfsstat3::NFS3ERR_NOENT)));

    let new_dir = client
        .just_lookup(&root, "renamed_nonempty_dir")
        .await
        .unwrap();
    let file_handle = client
        .just_lookup(&new_dir, "file_inside.txt")
        .await
        .unwrap();
    let content = client.just_read(&file_handle).await.unwrap();
    assert_eq!(content, b"some content");

    client.shutdown().await
}

#[tokio::test]
async fn test_rename_directory_into_its_own_subdirectory() -> Result<(), anyhow::Error> {
    let mut client = TestContext::setup();
    let root = client.root_dir().clone();

    let parent_dir = client.just_mkdir(&root, "parent_dir").await.unwrap();
    let _subdir = client.just_mkdir(&parent_dir, "child_dir").await.unwrap();

    let rename = client
        .rename(&RENAME3args {
            from: diropargs3 {
                dir: root.clone(),
                name: b"parent_dir".as_slice().into(),
            },
            to: diropargs3 {
                dir: parent_dir.clone(),
                name: b"child_dir".as_slice().into(),
            },
        })
        .await?;

    tracing::info!("{rename:?}");
    assert!(
        matches!(rename, Nfs3Result::Err((nfsstat3::NFS3ERR_INVAL, _))),
        "Expected NFS3ERR_INVAL error"
    );

    client.shutdown().await
}
