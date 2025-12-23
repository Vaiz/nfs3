#![allow(unused_variables)]

use std::fs;
#[cfg(unix)]
use std::os::unix::fs as unix_fs;
use std::path::PathBuf;

use nfs3_client::nfs3_types::nfs3::*;
use nfs3_client::nfs3_types::xdr_codec::Opaque;
use nfs3_tests::JustClientExt;

use crate::context::TestContext;
use crate::fs_util::{
    assert_attributes_match, assert_files_equal, assert_files_equal_ex, assert_folders_equal,
    create_test_file,
};

// ============================================================================
// NFS3 Operations Coverage
// ============================================================================
// This file tests all 22 NFS3 operations in readonly mode:
//
// Read Operations (should succeed):
//   1. NULL         - tested in null()
//   2. GETATTR      - tested in getattr_root(), getattr_file()
//   3. LOOKUP       - tested in lookup_existing_file(), lookup_non_existing_file(), etc.
//   4. ACCESS       - tested in access_file()
//   5. READLINK     - tested in readlink_symlink()
//   6. READ         - tested in read_file_contents(), read_large_file(), etc.
//   7. READDIR      - tested in readdir_multiple_files(), readdir_empty_directory(), etc.
//   8. READDIRPLUS  - tested in readdirplus_basic()
//   9. FSSTAT       - tested in fsstat_root()
//  10. FSINFO       - tested in fsinfo_root()
//  11. PATHCONF     - tested in pathconf_root()
//
// Write Operations (should return NFS3ERR_ROFS):
//  12. SETATTR      - tested in setattr_readonly_error()
//  13. WRITE        - tested in write_readonly_error()
//  14. CREATE       - tested in create_readonly_error()
//  15. MKDIR        - tested in mkdir_readonly_error()
//  16. SYMLINK      - tested in symlink_readonly_error()
//  17. MKNOD        - tested in mknod_readonly_error()
//  18. REMOVE       - tested in remove_readonly_error()
//  19. RMDIR        - tested in rmdir_readonly_error()
//  20. RENAME       - tested in rename_readonly_error()
//  21. LINK         - tested in link_readonly_error()
//  22. COMMIT       - tested in commit_readonly_error()
// ============================================================================

pub async fn null(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    ctx.client.null().await.expect("null call failed");
}

pub async fn getattr_root(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    let root_fh = ctx.root_fh();
    let args = GETATTR3args { object: root_fh };
    let resok = ctx
        .client
        .getattr(&args)
        .await
        .expect("getattr failed")
        .unwrap();

    assert_eq!(resok.obj_attributes.type_, ftype3::NF3DIR);
}

pub async fn getattr_file(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const GETATTR_TEST_FILE: &str = "getattr_test.txt";
    const TEST_CONTENT: &str = "test content";

    let file_path = subdir.join(GETATTR_TEST_FILE);
    fs::write(&file_path, TEST_CONTENT).expect("failed to write test file");

    let file_fh = ctx
        .just_lookup(&subdir_fh, GETATTR_TEST_FILE)
        .await
        .unwrap();

    let getattr_resok = ctx
        .client
        .getattr(&file_fh.clone().into())
        .await
        .expect("getattr failed")
        .unwrap();

    assert_eq!(getattr_resok.obj_attributes.type_, ftype3::NF3REG);
    assert_eq!(getattr_resok.obj_attributes.size, TEST_CONTENT.len() as u64);

    assert_files_equal_ex(
        subdir.as_path(),
        &file_fh,
        &getattr_resok.obj_attributes,
        GETATTR_TEST_FILE,
        TEST_CONTENT.len() as u64,
        ctx,
    )
    .await;
}

pub async fn lookup_existing_file(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const LOOKUP_FILE: &str = "lookup_file.txt";
    const HELLO_NFS: &str = "Hello, NFS!";

    let file_path = subdir.join(LOOKUP_FILE);
    fs::write(&file_path, HELLO_NFS).expect("failed to write test file");

    let args = LOOKUP3args {
        what: diropargs3 {
            dir: subdir_fh.clone(),
            name: LOOKUP_FILE.as_bytes().into(),
        },
    };

    let resok = ctx
        .client
        .lookup(&args)
        .await
        .expect("lookup call failed")
        .unwrap();

    let lookup_attr = resok.obj_attributes.unwrap();
    let get_attr = ctx.just_getattr(&resok.object).await.unwrap();
    assert_eq!(lookup_attr.type_, ftype3::NF3REG);
    assert_eq!(lookup_attr.size, HELLO_NFS.len() as u64);
    assert_eq!(lookup_attr.type_, get_attr.type_);
    assert_eq!(lookup_attr.mode, get_attr.mode);
    assert_eq!(lookup_attr.nlink, get_attr.nlink);
    assert_eq!(lookup_attr.uid, get_attr.uid);
    assert_eq!(lookup_attr.gid, get_attr.gid);
    assert_eq!(lookup_attr.size, get_attr.size);
    assert_eq!(lookup_attr.used, get_attr.used);
    assert_eq!(lookup_attr.rdev, get_attr.rdev);
    assert_eq!(lookup_attr.fsid, get_attr.fsid);
    assert_eq!(lookup_attr.fileid, get_attr.fileid);
    assert_eq!(lookup_attr.atime, get_attr.atime);
    assert_eq!(lookup_attr.mtime, get_attr.mtime);
    assert_eq!(lookup_attr.ctime, get_attr.ctime);

    assert_files_equal(
        subdir.as_path(),
        &subdir_fh,
        LOOKUP_FILE,
        HELLO_NFS.len() as u64,
        ctx,
    )
    .await;
}

pub async fn lookup_non_existing_file(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const NAME: &str = "non_existing_file.txt";

    let non_existing_path = subdir.join(NAME);
    assert!(
        !non_existing_path.exists(),
        "file should not exist in filesystem"
    );

    let args = LOOKUP3args {
        what: diropargs3 {
            dir: subdir_fh,
            name: NAME.as_bytes().into(),
        },
    };

    let res = ctx.client.lookup(&args).await.expect("lookup call failed");
    assert!(matches!(res, LOOKUP3res::Err((nfsstat3::NFS3ERR_NOENT, _))));
}

pub async fn lookup_in_subdirectory(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const SUBDIR_NAME: &str = "subdir";
    const NESTED_FILE: &str = "nested_file.txt";
    const NESTED_CONTENT: &str = "nested content";

    let nested_subdir = subdir.join(SUBDIR_NAME);
    fs::create_dir(&nested_subdir).expect("failed to create subdirectory");
    let nested_file_path = nested_subdir.join(NESTED_FILE);
    fs::write(&nested_file_path, NESTED_CONTENT).expect("failed to write nested file");

    let nested_subdir_resok = ctx
        .client
        .lookup(&LOOKUP3args {
            what: diropargs3 {
                dir: subdir_fh.clone(),
                name: SUBDIR_NAME.as_bytes().into(),
            },
        })
        .await
        .expect("lookup subdir failed")
        .unwrap();

    assert_folders_equal(subdir.as_path(), &subdir_fh, SUBDIR_NAME, ctx).await;

    let nested_subdir_fh = nested_subdir_resok.object;
    let file_resok = ctx
        .client
        .lookup(&LOOKUP3args {
            what: diropargs3 {
                dir: nested_subdir_fh.clone(),
                name: NESTED_FILE.as_bytes().into(),
            },
        })
        .await
        .expect("lookup nested file failed")
        .unwrap();

    assert_files_equal(
        nested_subdir.as_path(),
        &nested_subdir_fh,
        NESTED_FILE,
        NESTED_CONTENT.len() as u64,
        ctx,
    )
    .await;
}

pub async fn access_file(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const ACCESS_TEST_FILE: &str = "access_test.txt";
    const ACCESS_TEST_CONTENT: &str = "access test";

    let file_path = subdir.join(ACCESS_TEST_FILE);
    fs::write(&file_path, ACCESS_TEST_CONTENT).expect("failed to write test file");

    let lookup_resok = ctx
        .client
        .lookup(&LOOKUP3args {
            what: diropargs3 {
                dir: subdir_fh,
                name: ACCESS_TEST_FILE.as_bytes().into(),
            },
        })
        .await
        .expect("lookup failed")
        .unwrap();

    let file_fh = lookup_resok.object;

    let access_resok = ctx
        .client
        .access(&ACCESS3args {
            object: file_fh.clone(),
            access: ACCESS3_READ,
        })
        .await
        .expect("access call failed")
        .unwrap();

    assert!(
        access_resok.access & ACCESS3_READ != 0,
        "Read access not granted for readable file"
    );

    let access_resok_write = ctx
        .client
        .access(&ACCESS3args {
            object: file_fh,
            access: ACCESS3_MODIFY,
        })
        .await
        .expect("access call failed")
        .unwrap();

    assert!(
        access_resok_write.access & ACCESS3_MODIFY == 0,
        "Modify access should not be granted on readonly filesystem"
    );
}

pub async fn read_file_contents(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const READ_TEST_FILE: &str = "read_test.txt";
    const HELLO_WORLD: &[u8] = b"Hello, world!";

    let file_path = subdir.join(READ_TEST_FILE);
    fs::write(&file_path, HELLO_WORLD).expect("failed to write test file");

    let lookup_resok = ctx
        .client
        .lookup(&LOOKUP3args {
            what: diropargs3 {
                dir: subdir_fh.clone(),
                name: READ_TEST_FILE.as_bytes().into(),
            },
        })
        .await
        .expect("lookup failed")
        .unwrap();

    let file_fh = lookup_resok.object;

    let read_resok = ctx
        .client
        .read(&READ3args {
            file: file_fh,
            offset: 0,
            count: 1024,
        })
        .await
        .expect("read call failed")
        .unwrap();

    let read_content = read_resok.data.0;
    assert_eq!(read_content.as_ref(), HELLO_WORLD, "Content mismatch");
    assert!(read_resok.eof, "Expected EOF for complete read");

    assert_files_equal(
        subdir.as_path(),
        &subdir_fh,
        READ_TEST_FILE,
        HELLO_WORLD.len() as u64,
        ctx,
    )
    .await;
}

pub async fn read_large_file(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const LARGE_FILE: &str = "large_file.txt";
    const LARGE_FILE_SIZE: u64 = 100 * 1024 * 1024; // 100MB

    let file_path = subdir.join(LARGE_FILE);
    create_test_file(&file_path, LARGE_FILE_SIZE).expect("failed to create large file");
    assert_files_equal(
        subdir.as_path(),
        &subdir_fh,
        LARGE_FILE,
        LARGE_FILE_SIZE,
        ctx,
    )
    .await;
}

pub async fn read_with_offset(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const OFFSET_TEST_FILE: &str = "offset_test.txt";
    const OFFSET_CONTENT: &[u8] = b"0123456789abcdefghij";

    let file_path = subdir.join(OFFSET_TEST_FILE);
    fs::write(&file_path, OFFSET_CONTENT).expect("failed to write test file");

    let lookup_resok = ctx
        .client
        .lookup(&LOOKUP3args {
            what: diropargs3 {
                dir: subdir_fh,
                name: OFFSET_TEST_FILE.as_bytes().into(),
            },
        })
        .await
        .expect("lookup failed")
        .unwrap();

    let file_fh = lookup_resok.object;

    let read_resok = ctx
        .client
        .read(&READ3args {
            file: file_fh,
            offset: 10,
            count: 5,
        })
        .await
        .expect("read call failed")
        .unwrap();

    let data = read_resok.data.0;
    assert_eq!(data.as_ref(), b"abcde", "Content mismatch with offset");
}

pub async fn read_binary_file(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const BINARY_TEST_FILE: &str = "binary_test.bin";
    const BINARY_DATA_SIZE: usize = 256;

    let binary_data: Vec<u8> = (0..BINARY_DATA_SIZE).map(|i| i as u8).collect();
    let file_path = subdir.join(BINARY_TEST_FILE);
    fs::write(&file_path, &binary_data).expect("failed to write binary file");
    assert_files_equal(
        subdir.as_path(),
        &subdir_fh,
        BINARY_TEST_FILE,
        BINARY_DATA_SIZE as u64,
        ctx,
    )
    .await;
}

pub async fn readdir_multiple_files(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    for i in 1..=5 {
        let file_path = subdir.join(format!("file{}.txt", i));
        fs::write(file_path, format!("content {}", i)).expect("failed to write test file");
    }

    let readdir_resok = ctx
        .client
        .readdir(&READDIR3args {
            dir: subdir_fh,
            cookie: 0,
            cookieverf: cookieverf3::default(),
            count: 256 * 1024,
        })
        .await
        .expect("readdir call failed")
        .unwrap();

    let entries = &readdir_resok.reply.entries.0;
    let mut found_files = 0;
    for entry in entries {
        let name = String::from_utf8_lossy(entry.name.0.as_ref());
        if name.starts_with("file") && name.ends_with(".txt") {
            found_files += 1;
        }
    }
    assert_eq!(found_files, 5, "Should find all 5 created files");
}

pub async fn readdir_empty_directory(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const EMPTY_DIR: &str = "empty_dir";

    let empty_dir = subdir.join(EMPTY_DIR);
    fs::create_dir(&empty_dir).expect("failed to create empty directory");

    let lookup_resok = ctx
        .client
        .lookup(&LOOKUP3args {
            what: diropargs3 {
                dir: subdir_fh,
                name: EMPTY_DIR.as_bytes().into(),
            },
        })
        .await
        .expect("lookup failed")
        .unwrap();

    let dir_fh = lookup_resok.object;

    let readdir_resok = ctx
        .client
        .readdir(&READDIR3args {
            dir: dir_fh,
            cookie: 0,
            cookieverf: cookieverf3::default(),
            count: 4096,
        })
        .await
        .expect("readdir call failed")
        .unwrap();

    // Empty directory should only have . and .. entries (or none on some systems)
    let entries = &readdir_resok.reply.entries.0;
    for entry in entries {
        let name = String::from_utf8_lossy(entry.name.0.as_ref());
        assert!(
            name == "." || name == "..",
            "Empty directory should only contain . and .. entries, found: {name}",
        );
    }
}

pub async fn readdir_many_files(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    for i in 1..=50 {
        let file_path = subdir.join(format!("many_{:03}.txt", i));
        fs::write(file_path, format!("content {}", i)).expect("failed to write test file");
    }

    let readdir_resok = ctx
        .client
        .readdir(&READDIR3args {
            dir: subdir_fh,
            cookie: 0,
            cookieverf: cookieverf3::default(),
            count: 4096,
        })
        .await
        .expect("readdir call failed")
        .unwrap();

    let entries = &readdir_resok.reply.entries.0;
    let mut found_files = 0;
    for entry in entries {
        let name = String::from_utf8_lossy(entry.name.0.as_ref());
        if name.starts_with("many_") {
            found_files += 1;
        }
    }
    assert_eq!(found_files, 50, "Should find all 50 created files");
}

pub async fn readdirplus_basic(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    // Create files in the provided subdirectory
    for i in 1..=3 {
        let file_path = subdir.join(format!("plus_file{}.txt", i));
        fs::write(file_path, format!("content {}", i)).expect("failed to write test file");
    }

    // Now do READDIRPLUS on the subdirectory
    let readdirplus_resok = ctx
        .client
        .readdirplus(&READDIRPLUS3args {
            dir: subdir_fh,
            cookie: 0,
            cookieverf: cookieverf3::default(),
            dircount: 4096,
            maxcount: 8192,
        })
        .await
        .expect("readdirplus call failed")
        .unwrap();

    let entries = &readdirplus_resok.reply.entries.0;
    let mut found_files = 0;
    for entry in entries {
        let name = String::from_utf8_lossy(entry.name.0.as_ref());
        if name.starts_with("plus_file") && name.ends_with(".txt") {
            found_files += 1;
            let attrs = entry.name_attributes.clone().unwrap();
            let file_path = subdir.join(name.to_string());
            assert_files_equal_ex(
                subdir.as_path(),
                &entry.name_handle.clone().unwrap(),
                &attrs,
                &name,
                attrs.size,
                ctx,
            )
            .await;
        }
    }
    assert_eq!(found_files, 3, "Should find all 3 created files");
}

pub async fn fsstat_root(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    let root_fh = ctx.root_fh();
    let fsstat_resok = ctx
        .client
        .fsstat(&root_fh.into())
        .await
        .expect("fsstat call failed")
        .unwrap();

    assert!(
        fsstat_resok.tbytes > 0,
        "Total bytes should be greater than 0"
    );
}

pub async fn fsinfo_root(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    let root_fh = ctx.root_fh();
    let fsinfo_resok = ctx
        .client
        .fsinfo(&root_fh.into())
        .await
        .expect("fsinfo call failed")
        .unwrap();

    let attrs = fsinfo_resok.obj_attributes.unwrap();
    assert_attributes_match(&attrs, ctx.local_path(), ftype3::NF3DIR)
        .expect("fsinfo attributes do not match filesystem");

    assert!(fsinfo_resok.rtmax > 0, "rtmax should be greater than 0");
    assert!(fsinfo_resok.wtmax > 0, "wtmax should be greater than 0");
}

pub async fn pathconf_root(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    let root_fh = ctx.root_fh();
    let pathconf_resok = ctx
        .client
        .pathconf(&root_fh.into())
        .await
        .expect("pathconf call failed")
        .unwrap();

    // Verify attributes match
    let attrs = pathconf_resok.obj_attributes.unwrap();
    assert_attributes_match(&attrs, ctx.local_path(), ftype3::NF3DIR)
        .expect("pathconf attributes do not match filesystem");
    // Verify reasonable values
    assert!(
        pathconf_resok.name_max > 0,
        "name_max should be greater than 0"
    );
}

// ============================================================================
// Complex Scenarios
// ============================================================================

pub async fn deep_directory_navigation(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const LEVEL1_DIR: &str = "level1";
    const LEVEL2_DIR: &str = "level2";
    const LEVEL3_DIR: &str = "level3";
    const DEEP_FILE: &str = "deep.txt";
    const DEEP_CONTENT: &str = "deep content";

    // Create nested directory structure: level1/level2/level3
    let level1 = subdir.join(LEVEL1_DIR);
    let level2 = level1.join(LEVEL2_DIR);
    let level3 = level2.join(LEVEL3_DIR);
    fs::create_dir_all(&level3).expect("failed to create nested directories");
    let deep_file = level3.join(DEEP_FILE);
    fs::write(&deep_file, DEEP_CONTENT).expect("failed to write deep file");

    // Navigate to level1
    let level1_resok = ctx
        .client
        .lookup(&LOOKUP3args {
            what: diropargs3 {
                dir: subdir_fh,
                name: LEVEL1_DIR.as_bytes().into(),
            },
        })
        .await
        .expect("lookup level1 failed")
        .unwrap();

    let attrs = level1_resok.obj_attributes.unwrap();
    assert_attributes_match(&attrs, &level1, ftype3::NF3DIR)
        .expect("level1 attributes do not match filesystem");
    let level1_fh = level1_resok.object;

    // Navigate to level2
    let level2_resok = ctx
        .client
        .lookup(&LOOKUP3args {
            what: diropargs3 {
                dir: level1_fh,
                name: LEVEL2_DIR.as_bytes().into(),
            },
        })
        .await
        .expect("lookup level2 failed")
        .unwrap();

    let attrs = level2_resok.obj_attributes.unwrap();
    assert_attributes_match(&attrs, &level2, ftype3::NF3DIR)
        .expect("level2 attributes do not match filesystem");
    let level2_fh = level2_resok.object;

    // Navigate to level3
    let level3_resok = ctx
        .client
        .lookup(&LOOKUP3args {
            what: diropargs3 {
                dir: level2_fh,
                name: LEVEL3_DIR.as_bytes().into(),
            },
        })
        .await
        .expect("lookup level3 failed")
        .unwrap();

    let attrs = level3_resok.obj_attributes.unwrap();
    assert_attributes_match(&attrs, &level3, ftype3::NF3DIR)
        .expect("level3 attributes do not match filesystem");
    let level3_fh = level3_resok.object;

    // Lookup the deep file
    let file_resok = ctx
        .client
        .lookup(&LOOKUP3args {
            what: diropargs3 {
                dir: level3_fh,
                name: DEEP_FILE.as_bytes().into(),
            },
        })
        .await
        .expect("lookup deep file failed")
        .unwrap();

    let attrs = file_resok.obj_attributes.unwrap();
    assert_attributes_match(&attrs, &deep_file, ftype3::NF3REG)
        .expect("deep file attributes do not match filesystem");
}

pub async fn special_characters_filename(
    ctx: &mut TestContext,
    subdir: PathBuf,
    subdir_fh: nfs_fh3,
) {
    const SPECIAL_FILE: &str = "special-file_123.txt";
    const SPECIAL_CONTENT: &str = "special content";

    let file_path = subdir.join(SPECIAL_FILE);
    fs::write(&file_path, SPECIAL_CONTENT).expect("failed to write file with special characters");

    let lookup_resok = ctx
        .client
        .lookup(&LOOKUP3args {
            what: diropargs3 {
                dir: subdir_fh,
                name: SPECIAL_FILE.as_bytes().into(),
            },
        })
        .await
        .expect("lookup failed")
        .unwrap();

    // Verify the file attributes match the filesystem
    let attrs = lookup_resok.obj_attributes.unwrap();
    assert_attributes_match(&attrs, &file_path, ftype3::NF3REG)
        .expect("special characters file attributes do not match filesystem");
}

pub async fn concurrent_reads(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const CONCURRENT_FILE: &str = "concurrent.txt";
    const CONCURRENT_CONTENT: &str = "concurrent test content";

    let file_path = subdir.join(CONCURRENT_FILE);
    fs::write(&file_path, CONCURRENT_CONTENT).expect("failed to write test file");

    let lookup_resok = ctx
        .client
        .lookup(&LOOKUP3args {
            what: diropargs3 {
                dir: subdir_fh,
                name: CONCURRENT_FILE.as_bytes().into(),
            },
        })
        .await
        .expect("lookup failed")
        .unwrap();

    let file_fh = lookup_resok.object;

    // Perform multiple sequential reads to verify consistency
    for i in 0..5 {
        let read_resok = ctx
            .client
            .read(&READ3args {
                file: file_fh.clone(),
                offset: 0,
                count: 1024,
            })
            .await
            .expect("read call failed")
            .unwrap();

        let data = read_resok.data.0;
        let read_content = String::from_utf8(data.to_vec()).expect("invalid UTF-8 in read data");
        assert_eq!(
            read_content,
            CONCURRENT_CONTENT,
            "Read #{} content mismatch",
            i + 1
        );
    }
}

// ============================================================================
// Symlink Operations
// ============================================================================

#[cfg(unix)]
pub async fn readlink_symlink(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const READLINK_TARGET_FILE: &str = "readlink_target.txt";
    const READLINK_SYMLINK: &str = "readlink_test";
    const SYMLINK_TARGET_CONTENT: &str = "symlink target content";

    let target_file = subdir.join(READLINK_TARGET_FILE);
    let symlink_path = subdir.join(READLINK_SYMLINK);
    fs::write(&target_file, SYMLINK_TARGET_CONTENT).expect("failed to write target file");

    unix_fs::symlink(READLINK_TARGET_FILE, &symlink_path).expect("failed to create symlink");

    let lookup_resok = ctx
        .client
        .lookup(&LOOKUP3args {
            what: diropargs3 {
                dir: subdir_fh,
                name: READLINK_SYMLINK.as_bytes().into(),
            },
        })
        .await
        .expect("lookup failed")
        .unwrap();

    let symlink_fh = lookup_resok.object;

    let readlink_resok = ctx
        .client
        .readlink(&READLINK3args {
            symlink: symlink_fh,
        })
        .await
        .expect("readlink call failed")
        .unwrap();

    let target = readlink_resok.data.0;
    let target_str = String::from_utf8(target.to_vec()).expect("invalid UTF-8 in symlink target");
    assert_eq!(
        target_str, READLINK_TARGET_FILE,
        "symlink target does not match"
    );
}

#[cfg(not(unix))]
pub async fn readlink_symlink(_ctx: &mut TestContext, _subdir: PathBuf, _subdir_fh: nfs_fh3) {
    // Symlinks are not supported on Windows in the same way
    // Skip this test on non-Unix platforms
}

// ============================================================================
// Write Operations - Should Return NFS3ERR_ROFS
// ============================================================================

pub async fn setattr_readonly_error(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const SETATTR_TEST_FILE: &str = "setattr_test.txt";
    const TEST_CONTENT: &str = "test content";

    let file_path = subdir.join(SETATTR_TEST_FILE);
    fs::write(&file_path, TEST_CONTENT).expect("failed to write test file");

    let lookup_resok = ctx
        .client
        .lookup(&LOOKUP3args {
            what: diropargs3 {
                dir: subdir_fh,
                name: SETATTR_TEST_FILE.as_bytes().into(),
            },
        })
        .await
        .expect("lookup failed")
        .unwrap();

    let file_fh = lookup_resok.object;

    let setattr_result = ctx
        .client
        .setattr(&SETATTR3args {
            object: file_fh,
            new_attributes: sattr3 {
                mode: Nfs3Option::Some(0o644),
                uid: Nfs3Option::None,
                gid: Nfs3Option::None,
                size: Nfs3Option::None,
                atime: set_atime::DONT_CHANGE,
                mtime: set_mtime::DONT_CHANGE,
            },
            guard: Nfs3Option::None,
        })
        .await
        .expect("setattr call failed");

    match setattr_result {
        SETATTR3res::Err((nfsstat3::NFS3ERR_ROFS, _)) => {
            // Expected error - readonly filesystem
        }
        _ => panic!(
            "Expected NFS3ERR_ROFS error for setattr on readonly filesystem, got: {:?}",
            setattr_result
        ),
    }
}

pub async fn write_readonly_error(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const WRITE_TEST_FILE: &str = "write_test.txt";
    const ORIGINAL_CONTENT: &str = "original content";

    let file_path = subdir.join(WRITE_TEST_FILE);
    fs::write(&file_path, ORIGINAL_CONTENT).expect("failed to write test file");

    let lookup_resok = ctx
        .client
        .lookup(&LOOKUP3args {
            what: diropargs3 {
                dir: subdir_fh,
                name: WRITE_TEST_FILE.as_bytes().into(),
            },
        })
        .await
        .expect("lookup failed")
        .unwrap();

    let file_fh = lookup_resok.object;

    let write_result = ctx
        .client
        .write(&WRITE3args {
            file: file_fh,
            offset: 0,
            count: 4,
            stable: stable_how::UNSTABLE,
            data: Opaque::borrowed(b"test"),
        })
        .await
        .expect("write call failed");

    match write_result {
        WRITE3res::Err((nfsstat3::NFS3ERR_ROFS, _)) => {
            // Expected error - readonly filesystem
        }
        _ => panic!(
            "Expected NFS3ERR_ROFS error for write on readonly filesystem, got: {:?}",
            write_result
        ),
    }
}

pub async fn create_readonly_error(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const CREATE_TEST_FILE: &str = "create_test.txt";

    let create_result = ctx
        .client
        .create(&CREATE3args {
            where_: diropargs3 {
                dir: subdir_fh,
                name: CREATE_TEST_FILE.as_bytes().into(),
            },
            how: createhow3::UNCHECKED(sattr3::default()),
        })
        .await
        .expect("create call failed");

    match create_result {
        CREATE3res::Err((nfsstat3::NFS3ERR_ROFS, _)) => {
            // Expected error - readonly filesystem
        }
        _ => panic!(
            "Expected NFS3ERR_ROFS error for create on readonly filesystem, got: {:?}",
            create_result
        ),
    }
}

pub async fn mkdir_readonly_error(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const MKDIR_TEST_DIR: &str = "mkdir_test";

    let mkdir_result = ctx
        .client
        .mkdir(&MKDIR3args {
            where_: diropargs3 {
                dir: subdir_fh,
                name: MKDIR_TEST_DIR.as_bytes().into(),
            },
            attributes: sattr3::default(),
        })
        .await
        .expect("mkdir call failed");

    match mkdir_result {
        MKDIR3res::Err((nfsstat3::NFS3ERR_ROFS, _)) => {
            // Expected error - readonly filesystem
        }
        _ => panic!(
            "Expected NFS3ERR_ROFS error for mkdir on readonly filesystem, got: {:?}",
            mkdir_result
        ),
    }
}

pub async fn symlink_readonly_error(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const SYMLINK_TEST: &str = "symlink_test";
    const SYMLINK_TARGET: &str = "target.txt";

    let symlink_result = ctx
        .client
        .symlink(&SYMLINK3args {
            where_: diropargs3 {
                dir: subdir_fh,
                name: SYMLINK_TEST.as_bytes().into(),
            },
            symlink: symlinkdata3 {
                symlink_attributes: sattr3::default(),
                symlink_data: nfspath3(Opaque::borrowed(SYMLINK_TARGET.as_bytes())),
            },
        })
        .await
        .expect("symlink call failed");

    match symlink_result {
        SYMLINK3res::Err((nfsstat3::NFS3ERR_ROFS, _)) => {
            // Expected error - readonly filesystem
        }
        SYMLINK3res::Err((nfsstat3::NFS3ERR_NOTSUPP, _)) => {
            // Symlinks may not be supported on all filesystems - this is acceptable
        }
        _ => panic!(
            "Expected NFS3ERR_ROFS or NFS3ERR_NOTSUPP error for symlink on readonly filesystem, \
             got: {:?}",
            symlink_result
        ),
    }
}

pub async fn mknod_readonly_error(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const MKNOD_TEST: &str = "mknod_test";

    let mknod_result = ctx
        .client
        .mknod(&MKNOD3args {
            where_: diropargs3 {
                dir: subdir_fh,
                name: MKNOD_TEST.as_bytes().into(),
            },
            what: mknoddata3::NF3CHR(devicedata3 {
                dev_attributes: sattr3::default(),
                spec: specdata3 {
                    specdata1: 0,
                    specdata2: 0,
                },
            }),
        })
        .await;

    match mknod_result {
        Ok(MKNOD3res::Err((nfsstat3::NFS3ERR_ROFS, _))) => {
            // Expected error - readonly filesystem
        }
        Ok(MKNOD3res::Err((nfsstat3::NFS3ERR_NOTSUPP, _))) => {
            // Special files may not be supported - this is acceptable
        }
        Err(e) if e.to_string().contains("Procedure unavailable") => {
            // MKNOD procedure not implemented by server - this is acceptable
        }
        Ok(result) => panic!(
            "Expected NFS3ERR_ROFS or NFS3ERR_NOTSUPP error for mknod on readonly filesystem, \
             got: {:?}",
            result
        ),
        Err(e) => panic!("Unexpected RPC error for mknod: {}", e),
    }
}

pub async fn remove_readonly_error(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const REMOVE_TEST_FILE: &str = "remove_test.txt";
    const TEST_CONTENT: &str = "test content";

    let file_path = subdir.join(REMOVE_TEST_FILE);
    fs::write(&file_path, TEST_CONTENT).expect("failed to write test file");

    let remove_result = ctx
        .client
        .remove(&REMOVE3args {
            object: diropargs3 {
                dir: subdir_fh,
                name: REMOVE_TEST_FILE.as_bytes().into(),
            },
        })
        .await
        .expect("remove call failed");

    match remove_result {
        REMOVE3res::Err((nfsstat3::NFS3ERR_ROFS, _)) => {
            // Expected error - readonly filesystem
            assert!(
                file_path.exists(),
                "File should still exist after failed remove"
            );
        }
        _ => panic!(
            "Expected NFS3ERR_ROFS error for remove on readonly filesystem, got: {:?}",
            remove_result
        ),
    }
}

pub async fn rmdir_readonly_error(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const RMDIR_TEST_DIR: &str = "rmdir_test";

    let dir_path = subdir.join(RMDIR_TEST_DIR);
    fs::create_dir(&dir_path).expect("failed to create test directory");

    let rmdir_result = ctx
        .client
        .rmdir(&RMDIR3args {
            object: diropargs3 {
                dir: subdir_fh,
                name: RMDIR_TEST_DIR.as_bytes().into(),
            },
        })
        .await
        .expect("rmdir call failed");

    match rmdir_result {
        RMDIR3res::Err((nfsstat3::NFS3ERR_ROFS, _)) => {
            // Expected error - readonly filesystem
            assert!(
                dir_path.exists(),
                "Directory should still exist after failed rmdir"
            );
        }
        _ => panic!(
            "Expected NFS3ERR_ROFS error for rmdir on readonly filesystem, got: {:?}",
            rmdir_result
        ),
    }
}

pub async fn rename_readonly_error(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const RENAME_SOURCE_FILE: &str = "rename_source.txt";
    const RENAME_DEST_FILE: &str = "rename_dest.txt";
    const TEST_CONTENT: &str = "test content";

    let source_path = subdir.join(RENAME_SOURCE_FILE);
    fs::write(&source_path, TEST_CONTENT).expect("failed to write test file");

    let rename_result = ctx
        .client
        .rename(&RENAME3args {
            from: diropargs3 {
                dir: subdir_fh.clone(),
                name: RENAME_SOURCE_FILE.as_bytes().into(),
            },
            to: diropargs3 {
                dir: subdir_fh,
                name: RENAME_DEST_FILE.as_bytes().into(),
            },
        })
        .await
        .expect("rename call failed");

    match rename_result {
        RENAME3res::Err((nfsstat3::NFS3ERR_ROFS, _)) => {
            // Expected error - readonly filesystem
            assert!(
                source_path.exists(),
                "Source file should still exist after failed rename"
            );
        }
        _ => panic!(
            "Expected NFS3ERR_ROFS error for rename on readonly filesystem, got: {:?}",
            rename_result
        ),
    }
}

pub async fn link_readonly_error(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const LINK_SOURCE_FILE: &str = "link_source.txt";
    const LINK_DEST_FILE: &str = "link_dest.txt";
    const TEST_CONTENT: &str = "test content";

    let file_path = subdir.join(LINK_SOURCE_FILE);
    fs::write(&file_path, TEST_CONTENT).expect("failed to write test file");

    let lookup_resok = ctx
        .client
        .lookup(&LOOKUP3args {
            what: diropargs3 {
                dir: subdir_fh.clone(),
                name: LINK_SOURCE_FILE.as_bytes().into(),
            },
        })
        .await
        .expect("lookup failed")
        .unwrap();

    let file_fh = lookup_resok.object;

    let link_result = ctx
        .client
        .link(&LINK3args {
            file: file_fh,
            link: diropargs3 {
                dir: subdir_fh.clone(),
                name: LINK_DEST_FILE.as_bytes().into(),
            },
        })
        .await;

    match link_result {
        Ok(LINK3res::Err((nfsstat3::NFS3ERR_ROFS, _))) => {
            // Expected error - readonly filesystem
        }
        Err(e) if e.to_string().contains("Procedure unavailable") => {
            // LINK procedure not implemented by server - this is acceptable
        }
        Ok(result) => panic!(
            "Expected NFS3ERR_ROFS error for link on readonly filesystem, got: {:?}",
            result
        ),
        Err(e) => panic!("Unexpected RPC error for link: {}", e),
    }
}

pub async fn commit_readonly_error(ctx: &mut TestContext, subdir: PathBuf, subdir_fh: nfs_fh3) {
    const COMMIT_TEST_FILE: &str = "commit_test.txt";
    const TEST_CONTENT: &str = "test content";

    let file_path = subdir.join(COMMIT_TEST_FILE);
    fs::write(&file_path, TEST_CONTENT).expect("failed to write test file");

    let lookup_resok = ctx
        .client
        .lookup(&LOOKUP3args {
            what: diropargs3 {
                dir: subdir_fh,
                name: COMMIT_TEST_FILE.as_bytes().into(),
            },
        })
        .await
        .expect("lookup failed")
        .unwrap();

    let file_fh = lookup_resok.object;

    let commit_result = ctx
        .client
        .commit(&COMMIT3args {
            file: file_fh,
            offset: 0,
            count: 0,
        })
        .await;

    match commit_result {
        Ok(COMMIT3res::Err((nfsstat3::NFS3ERR_ROFS, _))) => {
            // Expected error - readonly filesystem
        }
        Ok(COMMIT3res::Ok(_)) => {
            // Some implementations may allow COMMIT on readonly filesystems
            // since it's essentially a no-op. This is acceptable.
        }
        Err(e) if e.to_string().contains("Procedure unavailable") => {
            // COMMIT procedure not implemented by server - this is acceptable
        }
        Ok(result) => panic!(
            "Expected NFS3ERR_ROFS error or success for commit on readonly filesystem, got: {:?}",
            result
        ),
        Err(e) => panic!("Unexpected RPC error for commit: {}", e),
    }
}
