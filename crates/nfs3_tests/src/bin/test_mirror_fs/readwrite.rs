use std::fs;
use std::path::PathBuf;

use nfs3_client::nfs3_types::nfs3::*;
use nfs3_client::nfs3_types::xdr_codec::Opaque;

use crate::context::TestContext;
use crate::fs_util::assert_attributes_match;

// ============================================================================
// Write Operations
// ============================================================================

pub async fn write_to_file(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    let file_path = subdir.join("write_test.txt");
    let initial_content = "initial content";
    fs::write(&file_path, initial_content).expect("failed to write test file");

    let lookup_resok = ctx
        .client
        .lookup(&LOOKUP3args {
            what: diropargs3 {
                dir: subdir_fh,
                name: filename3(Opaque::borrowed(b"write_test.txt")),
            },
        })
        .await
        .expect("lookup failed")
        .unwrap();

    let file_fh = lookup_resok.object;

    let new_content = b"new content";

    // Truncate file first to match expected size
    let _setattr_res = ctx
        .client
        .setattr(&SETATTR3args {
            object: file_fh.clone(),
            new_attributes: sattr3 {
                size: set_size3::Some(new_content.len() as u64),
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
            count: new_content.len() as u32,
            stable: stable_how::UNSTABLE,
            data: Opaque::borrowed(new_content),
        })
        .await
        .expect("write call failed");

    let resok = write_res.unwrap();
    assert_eq!(
        resok.count,
        new_content.len() as u32,
        "Write count mismatch"
    );
    // Verify file content changed
    let fs_content = fs::read_to_string(&file_path).expect("failed to read file from filesystem");
    assert_eq!(fs_content, "new content", "File content should be updated");
    // Verify attributes
    let attrs = resok.file_wcc.after.unwrap();
    assert_attributes_match(&attrs, &file_path, ftype3::NF3REG)
        .expect("write file attributes do not match filesystem");
}

pub async fn write_with_offset(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    let file_path = subdir.join("write_offset.txt");
    let initial_content = "0123456789";
    fs::write(&file_path, initial_content).expect("failed to write test file");

    let lookup_resok = ctx
        .client
        .lookup(&LOOKUP3args {
            what: diropargs3 {
                dir: subdir_fh,
                name: filename3(Opaque::borrowed(b"write_offset.txt")),
            },
        })
        .await
        .expect("lookup failed")
        .unwrap();

    let file_fh = lookup_resok.object;

    let write_data = b"ABCDE";
    let write_res = ctx
        .client
        .write(&WRITE3args {
            file: file_fh,
            offset: 5,
            count: write_data.len() as u32,
            stable: stable_how::UNSTABLE,
            data: Opaque::borrowed(write_data),
        })
        .await
        .expect("write call failed");

    let resok = write_res.unwrap();
    assert_eq!(resok.count, write_data.len() as u32, "Write count mismatch");
    // Verify file content changed at offset
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
    let file_path = subdir.join("created_file.txt");

    let create_res = ctx
        .client
        .create(&CREATE3args {
            where_: diropargs3 {
                dir: subdir_fh,
                name: filename3(Opaque::borrowed(b"created_file.txt")),
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
    let file_path = subdir.join("exclusive_file.txt");

    let create_res = ctx
        .client
        .create(&CREATE3args {
            where_: diropargs3 {
                dir: subdir_fh,
                name: filename3(Opaque::borrowed(b"exclusive_file.txt")),
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
    let dir_path = subdir.join("new_directory");

    let mkdir_res = ctx
        .client
        .mkdir(&MKDIR3args {
            where_: diropargs3 {
                dir: subdir_fh,
                name: filename3(Opaque::borrowed(b"new_directory")),
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
    let parent_path = subdir.join("parent_dir");
    let nested_path = parent_path.join("nested_dir");

    // First create parent directory
    let mkdir_res = ctx
        .client
        .mkdir(&MKDIR3args {
            where_: diropargs3 {
                dir: subdir_fh,
                name: filename3(Opaque::borrowed(b"parent_dir")),
            },
            attributes: sattr3::default(),
        })
        .await
        .expect("mkdir call failed")
        .unwrap();

    let parent_fh = mkdir_res.obj.unwrap();

    // Now create nested directory
    let nested_mkdir_res = ctx
        .client
        .mkdir(&MKDIR3args {
            where_: diropargs3 {
                dir: parent_fh,
                name: filename3(Opaque::borrowed(b"nested_dir")),
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
    let file_path = subdir.join("remove_me.txt");
    fs::write(&file_path, "to be removed").expect("failed to write test file");

    let remove_res = ctx
        .client
        .remove(&REMOVE3args {
            object: diropargs3 {
                dir: subdir_fh,
                name: filename3(Opaque::borrowed(b"remove_me.txt")),
            },
        })
        .await
        .expect("remove call failed");

    let _resok = remove_res.unwrap();
    assert!(!file_path.exists(), "File should be removed");
}

pub async fn rmdir_directory(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    let dir_path = subdir.join("remove_dir");
    fs::create_dir(&dir_path).expect("failed to create test directory");

    let rmdir_res = ctx
        .client
        .rmdir(&RMDIR3args {
            object: diropargs3 {
                dir: subdir_fh,
                name: filename3(Opaque::borrowed(b"remove_dir")),
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
    let source_path = subdir.join("rename_source.txt");
    let dest_path = subdir.join("rename_dest.txt");
    fs::write(&source_path, "rename test").expect("failed to write test file");

    let rename_res = ctx
        .client
        .rename(&RENAME3args {
            from: diropargs3 {
                dir: subdir_fh.clone(),
                name: filename3(Opaque::borrowed(b"rename_source.txt")),
            },
            to: diropargs3 {
                dir: subdir_fh,
                name: filename3(Opaque::borrowed(b"rename_dest.txt")),
            },
        })
        .await
        .expect("rename call failed");

    let _resok = rename_res.unwrap();
    assert!(!source_path.exists(), "Source file should be renamed");
    assert!(dest_path.exists(), "Destination file should exist");
    let content = fs::read_to_string(&dest_path).expect("failed to read renamed file");
    assert_eq!(content, "rename test", "Renamed file content should match");
}

pub async fn rename_directory(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    let source_path = subdir.join("rename_dir_source");
    let dest_path = subdir.join("rename_dir_dest");
    fs::create_dir(&source_path).expect("failed to create test directory");

    let rename_res = ctx
        .client
        .rename(&RENAME3args {
            from: diropargs3 {
                dir: subdir_fh.clone(),
                name: filename3(Opaque::borrowed(b"rename_dir_source")),
            },
            to: diropargs3 {
                dir: subdir_fh,
                name: filename3(Opaque::borrowed(b"rename_dir_dest")),
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
    let source_path = subdir.join("link_source.txt");
    let dest_path = subdir.join("link_dest.txt");
    let content = "link test";
    fs::write(&source_path, content).expect("failed to write test file");

    let lookup_resok = ctx
        .client
        .lookup(&LOOKUP3args {
            what: diropargs3 {
                dir: subdir_fh.clone(),
                name: filename3(Opaque::borrowed(b"link_source.txt")),
            },
        })
        .await
        .expect("lookup failed")
        .unwrap();

    let source_fh = lookup_resok.object;

    // Hard links may not be supported - handle both success and NOTSUPP
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
                assert_eq!(fs_content, content, "Linked file content should match");
                // Verify attributes
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
    let target_path = subdir.join("symlink_target.txt");
    let link_path = subdir.join("symlink_link");
    let content = "symlink target";
    fs::write(&target_path, content).expect("failed to write test file");

    let symlink_res = ctx
        .client
        .symlink(&SYMLINK3args {
            where_: diropargs3 {
                dir: subdir_fh,
                name: filename3(Opaque::borrowed(b"symlink_link")),
            },
            symlink: symlinkdata3 {
                symlink_attributes: sattr3::default(),
                symlink_data: nfspath3(Opaque::borrowed(b"symlink_target.txt")),
            },
        })
        .await;

    // Symlinks may not be supported - handle both success and NOTSUPP/IO errors
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
    let file_path = subdir.join("setattr_test.txt");
    fs::write(&file_path, "setattr test").expect("failed to write test file");

    let lookup_resok = ctx
        .client
        .lookup(&LOOKUP3args {
            what: diropargs3 {
                dir: subdir_fh,
                name: filename3(Opaque::borrowed(b"setattr_test.txt")),
            },
        })
        .await
        .expect("lookup failed")
        .unwrap();

    let file_fh = lookup_resok.object;

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
    let file_path = subdir.join("commit_test.txt");
    fs::write(&file_path, "commit content").expect("failed to write test file");

    let lookup_res = ctx
        .client
        .lookup(&LOOKUP3args {
            what: diropargs3 {
                dir: subdir_fh,
                name: filename3(Opaque::borrowed(b"commit_test.txt")),
            },
        })
        .await
        .expect("lookup failed")
        .unwrap();

    let file_fh = lookup_res.object;

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
                // Verify attributes - commit should return attributes on success
                let attrs = resok.file_wcc.after.unwrap();
                assert_attributes_match(&attrs, &file_path, ftype3::NF3REG)
                    .expect("commit file attributes do not match filesystem");
            } else {
                // It's okay if commit fails with other errors
            }
        }
        Err(e) => panic!("Commit operation failed: {}", e),
    }
}
