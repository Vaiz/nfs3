use std::fs;
use std::path::PathBuf;

use nfs3_client::nfs3_types::nfs3::*;
use nfs3_client::nfs3_types::xdr_codec::Opaque;
use nfs3_tests::JustClientExt;

use crate::context::TestContext;
use crate::fs_util::assert_attributes_match;

// ============================================================================
// NFS3 Write Operations Coverage
// ============================================================================
// This file tests write operations in readwrite mode:
//
// Write Operations:
//  1. WRITE        - tested in write_to_file(), write_with_offset()
//  2. CREATE       - tested in create_new_file(), create_exclusive()
//  3. MKDIR        - tested in mkdir_new_directory(), mkdir_nested()
//  4. REMOVE       - tested in remove_file()
//  5. RMDIR        - tested in rmdir_directory()
//  6. RENAME       - tested in rename_file(), rename_directory()
//  7. LINK         - tested in create_hard_link()
//  8. SYMLINK      - tested in create_symlink()
//  9. SETATTR      - tested in setattr_file()
// 10. COMMIT       - tested in commit_writes()
// ============================================================================

// ============================================================================
// Write Operations
// ============================================================================

pub async fn write_to_file(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const WRITE_TEST_FILE: &str = "write_test.txt";
    const INITIAL_CONTENT: &str = "initial content";

    let file_path = subdir.join(WRITE_TEST_FILE);
    fs::write(&file_path, INITIAL_CONTENT).expect("failed to write test file");

    let file_fh = ctx.just_lookup(&subdir_fh, WRITE_TEST_FILE).await.unwrap();

    const NEW_CONTENT: &[u8] = b"new content";

    // Truncate file first to match expected size
    let _setattr_res = ctx
        .client
        .setattr(&SETATTR3args {
            object: file_fh.clone(),
            new_attributes: sattr3 {
                size: set_size3::Some(NEW_CONTENT.len() as u64),
                ..Default::default()
            },
            guard: sattrguard3::default(),
        })
        .await
        .expect("setattr call failed")
        .unwrap();

    let write_res = ctx
        .client
        .write(&WRITE3args {
            file: file_fh,
            offset: 0,
            count: NEW_CONTENT.len() as u32,
            stable: stable_how::UNSTABLE,
            data: Opaque::borrowed(NEW_CONTENT),
        })
        .await
        .expect("write call failed");

    let resok = write_res.unwrap();
    assert_eq!(
        resok.count,
        NEW_CONTENT.len() as u32,
        "Write count mismatch"
    );

    let fs_content = fs::read_to_string(&file_path).expect("failed to read file from filesystem");
    assert_eq!(fs_content, "new content", "File content should be updated");

    let attrs = resok.file_wcc.after.unwrap();
    assert_attributes_match(&attrs, &file_path, ftype3::NF3REG)
        .expect("write file attributes do not match filesystem");
}

pub async fn write_with_offset(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const OFFSET_FILE: &str = "write_offset.txt";
    const INITIAL_CONTENT: &str = "0123456789";
    const WRITE_DATA: &[u8] = b"ABCDE";
    const WRITE_OFFSET: u64 = 5;

    let file_path = subdir.join(OFFSET_FILE);
    fs::write(&file_path, INITIAL_CONTENT).expect("failed to write test file");

    let file_fh = ctx.just_lookup(&subdir_fh, OFFSET_FILE).await.unwrap();

    let write_res = ctx
        .client
        .write(&WRITE3args {
            file: file_fh,
            offset: WRITE_OFFSET,
            count: WRITE_DATA.len() as u32,
            stable: stable_how::UNSTABLE,
            data: Opaque::borrowed(WRITE_DATA),
        })
        .await
        .expect("write call failed");

    let resok = write_res.unwrap();
    assert_eq!(resok.count, WRITE_DATA.len() as u32, "Write count mismatch");

    let fs_content = fs::read_to_string(&file_path).expect("failed to read file from filesystem");
    assert_eq!(
        fs_content, "01234ABCDE",
        "File content should be updated at offset"
    );
}

// ============================================================================
// Create Operations
// ============================================================================

pub async fn create_new_file(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const CREATED_FILE: &str = "created_file.txt";

    let file_path = subdir.join(CREATED_FILE);

    let create_res = ctx
        .client
        .create(&CREATE3args {
            where_: diropargs3 {
                dir: subdir_fh,
                name: CREATED_FILE.as_bytes().into(),
            },
            how: createhow3::UNCHECKED(sattr3::default()),
        })
        .await
        .expect("create call failed");

    let resok = create_res.unwrap();
    assert!(file_path.exists(), "File should exist after create");

    let attrs = resok.obj_attributes.unwrap();
    assert_attributes_match(&attrs, &file_path, ftype3::NF3REG)
        .expect("created file attributes do not match filesystem");
}

pub async fn create_exclusive(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const EXCLUSIVE_FILE: &str = "exclusive_file.txt";

    let file_path = subdir.join(EXCLUSIVE_FILE);

    let create_res = ctx
        .client
        .create(&CREATE3args {
            where_: diropargs3 {
                dir: subdir_fh,
                name: EXCLUSIVE_FILE.as_bytes().into(),
            },
            how: createhow3::EXCLUSIVE(createverf3([1, 2, 3, 4, 5, 6, 7, 8])),
        })
        .await
        .expect("create call failed");

    let _resok = create_res.unwrap();
    assert!(
        file_path.exists(),
        "File should exist after exclusive create"
    );
}

// ============================================================================
// Directory Creation
// ============================================================================

pub async fn mkdir_new_directory(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const NEW_DIR: &str = "new_directory";

    let dir_path = subdir.join(NEW_DIR);

    let mkdir_res = ctx
        .client
        .mkdir(&MKDIR3args {
            where_: diropargs3 {
                dir: subdir_fh,
                name: NEW_DIR.as_bytes().into(),
            },
            attributes: sattr3::default(),
        })
        .await
        .expect("mkdir call failed");

    let resok = mkdir_res.unwrap();
    assert!(dir_path.exists(), "Directory should exist after mkdir");
    assert!(dir_path.is_dir(), "Path should be a directory");

    let attrs = resok.obj_attributes.unwrap();
    assert_attributes_match(&attrs, &dir_path, ftype3::NF3DIR)
        .expect("created directory attributes do not match filesystem");
}

pub async fn mkdir_nested(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const PARENT_DIR: &str = "parent_dir";
    const NESTED_DIR: &str = "nested_dir";

    let parent_path = subdir.join(PARENT_DIR);
    let nested_path = parent_path.join(NESTED_DIR);

    let mkdir_res = ctx
        .client
        .mkdir(&MKDIR3args {
            where_: diropargs3 {
                dir: subdir_fh,
                name: PARENT_DIR.as_bytes().into(),
            },
            attributes: sattr3::default(),
        })
        .await
        .expect("mkdir call failed")
        .unwrap();

    let parent_fh = mkdir_res.obj.unwrap();

    let nested_mkdir_res = ctx
        .client
        .mkdir(&MKDIR3args {
            where_: diropargs3 {
                dir: parent_fh,
                name: NESTED_DIR.as_bytes().into(),
            },
            attributes: sattr3::default(),
        })
        .await
        .expect("nested mkdir call failed");

    let nested_resok = nested_mkdir_res.unwrap();
    assert!(parent_path.exists(), "Parent directory should exist");
    assert!(nested_path.exists(), "Nested directory should exist");
    assert!(nested_path.is_dir(), "Path should be a directory");

    let attrs = nested_resok.obj_attributes.unwrap();
    assert_attributes_match(&attrs, &nested_path, ftype3::NF3DIR)
        .expect("nested directory attributes do not match filesystem");
}

// ============================================================================
// Remove Operations
// ============================================================================

pub async fn remove_file(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const REMOVE_FILE: &str = "remove_me.txt";
    const REMOVE_CONTENT: &str = "to be removed";

    let file_path = subdir.join(REMOVE_FILE);
    fs::write(&file_path, REMOVE_CONTENT).expect("failed to write test file");

    let remove_res = ctx
        .client
        .remove(&REMOVE3args {
            object: diropargs3 {
                dir: subdir_fh,
                name: REMOVE_FILE.as_bytes().into(),
            },
        })
        .await
        .expect("remove call failed");

    let _resok = remove_res.unwrap();
    assert!(!file_path.exists(), "File should be removed");
}

pub async fn rmdir_directory(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const REMOVE_DIR: &str = "remove_dir";

    let dir_path = subdir.join(REMOVE_DIR);
    fs::create_dir(&dir_path).expect("failed to create test directory");

    let rmdir_res = ctx
        .client
        .rmdir(&RMDIR3args {
            object: diropargs3 {
                dir: subdir_fh,
                name: REMOVE_DIR.as_bytes().into(),
            },
        })
        .await
        .expect("rmdir call failed");

    let _resok = rmdir_res.unwrap();
    assert!(!dir_path.exists(), "Directory should be removed");
}

// ============================================================================
// Rename Operations
// ============================================================================

pub async fn rename_file(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const SOURCE_FILE: &str = "rename_source.txt";
    const DEST_FILE: &str = "rename_dest.txt";
    const RENAME_CONTENT: &str = "rename test";

    let source_path = subdir.join(SOURCE_FILE);
    let dest_path = subdir.join(DEST_FILE);
    fs::write(&source_path, RENAME_CONTENT).expect("failed to write test file");

    let rename_res = ctx
        .client
        .rename(&RENAME3args {
            from: diropargs3 {
                dir: subdir_fh.clone(),
                name: SOURCE_FILE.as_bytes().into(),
            },
            to: diropargs3 {
                dir: subdir_fh,
                name: DEST_FILE.as_bytes().into(),
            },
        })
        .await
        .expect("rename call failed");

    let _resok = rename_res.unwrap();
    assert!(!source_path.exists(), "Source file should be renamed");
    assert!(dest_path.exists(), "Destination file should exist");

    let content = fs::read_to_string(&dest_path).expect("failed to read renamed file");
    assert_eq!(content, RENAME_CONTENT, "Renamed file content should match");
}

pub async fn rename_directory(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const SOURCE_DIR: &str = "rename_dir_source";
    const DEST_DIR: &str = "rename_dir_dest";

    let source_path = subdir.join(SOURCE_DIR);
    let dest_path = subdir.join(DEST_DIR);
    fs::create_dir(&source_path).expect("failed to create test directory");

    let rename_res = ctx
        .client
        .rename(&RENAME3args {
            from: diropargs3 {
                dir: subdir_fh.clone(),
                name: SOURCE_DIR.as_bytes().into(),
            },
            to: diropargs3 {
                dir: subdir_fh,
                name: DEST_DIR.as_bytes().into(),
            },
        })
        .await
        .expect("rename call failed");

    let _resok = rename_res.unwrap();
    assert!(!source_path.exists(), "Source directory should be renamed");
    assert!(dest_path.exists(), "Destination directory should exist");
    assert!(dest_path.is_dir(), "Destination should be a directory");
}

// ============================================================================
// Link Operations
// ============================================================================

pub async fn create_hard_link(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const SOURCE_FILE: &str = "link_source.txt";
    const DEST_FILE: &str = "link_dest.txt";
    const LINK_CONTENT: &str = "link test";

    let source_path = subdir.join(SOURCE_FILE);
    let dest_path = subdir.join(DEST_FILE);
    fs::write(&source_path, LINK_CONTENT).expect("failed to write test file");

    let source_fh = ctx.just_lookup(&subdir_fh, SOURCE_FILE).await.unwrap();
    let link_res = ctx
        .client
        .link(&LINK3args {
            file: source_fh,
            link: diropargs3 {
                dir: subdir_fh,
                name: filename3(Opaque::borrowed(b"link_dest.txt")),
            },
        })
        .await;

    match link_res {
        Err(e) if e.to_string().contains("Procedure unavailable") => {
            // Hard links not supported by this filesystem - this is OK
        }
        Ok(link_result) => {
            if let LINK3res::Ok(resok) = link_result {
                assert!(dest_path.exists(), "Link should exist after creation");

                let fs_content =
                    fs::read_to_string(&dest_path).expect("failed to read linked file");
                assert_eq!(fs_content, LINK_CONTENT, "Linked file content should match");

                let attrs = resok.file_attributes.unwrap();
                assert_attributes_match(&attrs, &dest_path, ftype3::NF3REG)
                    .expect("link file attributes do not match filesystem");
            } else if let LINK3res::Err((nfsstat3::NFS3ERR_NOTSUPP, _)) = link_result {
                // Not supported - this is OK
            } else {
                panic!("Link failed on readwrite filesystem: {:?}", link_result);
            }
        }
        Err(e) => panic!("Link operation failed: {}", e),
    }
}

pub async fn create_symlink(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const TARGET_FILE: &str = "symlink_target.txt";
    const LINK_NAME: &str = "symlink_link";
    const SYMLINK_CONTENT: &str = "symlink target";

    let target_path = subdir.join(TARGET_FILE);
    let link_path = subdir.join(LINK_NAME);
    fs::write(&target_path, SYMLINK_CONTENT).expect("failed to write test file");

    let symlink_res = ctx
        .client
        .symlink(&SYMLINK3args {
            where_: diropargs3 {
                dir: subdir_fh,
                name: LINK_NAME.as_bytes().into(),
            },
            symlink: symlinkdata3 {
                symlink_attributes: sattr3::default(),
                symlink_data: nfspath3(Opaque::borrowed(TARGET_FILE.as_bytes())),
            },
        })
        .await;
    match symlink_res {
        Err(e) if e.to_string().contains("Procedure unavailable") => {
            // Symlinks not supported by this filesystem - this is OK
        }
        Ok(symlink_result) => {
            if let SYMLINK3res::Ok(resok) = symlink_result {
                assert!(link_path.exists(), "Symlink should exist after creation");

                let attrs = resok.obj_attributes.unwrap();
                assert_eq!(
                    attrs.type_,
                    ftype3::NF3LNK,
                    "Created object should be a symlink"
                );
            } else if let SYMLINK3res::Err((nfsstat3::NFS3ERR_NOTSUPP, _)) = symlink_result {
                // Not supported - this is OK
            } else if let SYMLINK3res::Err((nfsstat3::NFS3ERR_IO, _)) = symlink_result {
                // I/O error (e.g., Windows without symlink support) - this is OK
            } else {
                panic!(
                    "Symlink failed on readwrite filesystem: {:?}",
                    symlink_result
                );
            }
        }
        Err(e) => panic!("Symlink operation failed: {}", e),
    }
}

// ============================================================================
// Setattr Operations
// ============================================================================

pub async fn setattr_file(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const SETATTR_FILE: &str = "setattr_test.txt";
    const SETATTR_CONTENT: &str = "setattr test";

    let file_path = subdir.join(SETATTR_FILE);
    fs::write(&file_path, SETATTR_CONTENT).expect("failed to write test file");

    let file_fh = ctx.just_lookup(&subdir_fh, SETATTR_FILE).await.unwrap();

    let setattr_res = ctx
        .client
        .setattr(&SETATTR3args {
            object: file_fh,
            new_attributes: sattr3 {
                mode: set_mode3::Some(0o644),
                ..Default::default()
            },
            guard: sattrguard3::default(),
        })
        .await
        .expect("setattr call failed");

    let resok = setattr_res.unwrap();

    let attrs = resok.obj_wcc.after.unwrap();
    assert_attributes_match(&attrs, &file_path, ftype3::NF3REG)
        .expect("setattr file attributes do not match filesystem");
}

// ============================================================================
// Commit Operation
// ============================================================================

pub async fn commit_writes(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const COMMIT_FILE: &str = "commit_test.txt";
    const COMMIT_CONTENT: &str = "commit content";

    let file_path = subdir.join(COMMIT_FILE);
    fs::write(&file_path, COMMIT_CONTENT).expect("failed to write test file");

    let file_fh = ctx.just_lookup(&subdir_fh, COMMIT_FILE).await.unwrap();

    let commit_res = ctx
        .client
        .commit(&COMMIT3args {
            file: file_fh,
            offset: 0,
            count: 100,
        })
        .await;

    // Commit may not be supported - handle both procedure unavailable and result errors
    match commit_res {
        Err(e) if e.to_string().contains("Procedure unavailable") => {
            // Commit not supported by this filesystem - this is OK
        }
        Ok(commit_result) => {
            if let COMMIT3res::Ok(resok) = commit_result {
                let attrs = resok.file_wcc.after.unwrap();
                assert_attributes_match(&attrs, &file_path, ftype3::NF3REG)
                    .expect("commit file attributes do not match filesystem");
            }
        }
        Err(e) => panic!("Commit operation failed: {}", e),
    }
}
