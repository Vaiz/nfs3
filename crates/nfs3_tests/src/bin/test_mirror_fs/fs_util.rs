use std::io::Write;
use std::path::Path;
use std::time::SystemTime;

use anyhow::bail;
use nfs3_tests::JustClientExt;
use nfs3_types::nfs3::{Nfs3Result, fattr3, ftype3};

use crate::context::TestContext;

/// Compares two SystemTime values and returns the absolute difference in seconds.
fn time_diff_secs(t1: SystemTime, t2: SystemTime) -> u64 {
    match t1.cmp(&t2) {
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Less => t2.duration_since(t1).unwrap().as_secs(),
        std::cmp::Ordering::Greater => t1.duration_since(t2).unwrap().as_secs(),
    }
}

/// Compares NFS attributes with actual filesystem metadata.
///
/// Validates file type, size, permissions (Unix), and timestamps (mtime/atime).
/// Allows 1-second tolerance for mtime, 2-second tolerance for atime.
pub fn assert_attributes_match(
    nfs_attrs: &fattr3,
    fs_path: &Path,
    expected_type: ftype3,
) -> anyhow::Result<()> {
    let metadata = std::fs::metadata(fs_path)?;

    // Check file type
    if nfs_attrs.type_ != expected_type {
        bail!(
            "File type mismatch: NFS reports {}, expected {expected_type}",
            nfs_attrs.type_,
        );
    }

    // Verify type matches filesystem
    match expected_type {
        ftype3::NF3DIR => {
            if !metadata.is_dir() {
                bail!("NFS reports directory but filesystem shows file");
            }
        }
        ftype3::NF3REG => {
            if !metadata.is_file() {
                bail!("NFS reports file but filesystem shows directory");
            }
        }
        _ => {}
    }

    // Check size (only for regular files)
    if expected_type == ftype3::NF3REG {
        let fs_size = metadata.len();
        if nfs_attrs.size != fs_size {
            bail!(
                "File size mismatch: NFS reports {}, filesystem shows {}",
                nfs_attrs.size,
                fs_size
            );
        }
    }

    // Check permissions (mode)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let fs_mode = metadata.permissions().mode();
        // Compare only the permission bits (lower 9 bits for rwxrwxrwx)
        let fs_perms = fs_mode & 0o777;
        let nfs_perms = nfs_attrs.mode & 0o777;
        if fs_perms != nfs_perms {
            bail!(
                "Permission mismatch: NFS reports {:o}, filesystem shows {:o}",
                nfs_perms,
                fs_perms
            );
        }
    }

    // Compare modification time (mtime with 5-second tolerance on Windows, 1-second on Unix)
    // Windows file times can have synchronization delays and different precision
    let fs_mtime = metadata.modified()?;
    let nfs_mtime: SystemTime = nfs_attrs.mtime.into();

    #[cfg(windows)]
    let mtime_tolerance = 5;
    #[cfg(not(windows))]
    let mtime_tolerance = 1;

    let diff = time_diff_secs(nfs_mtime, fs_mtime);
    if diff > mtime_tolerance {
        bail!(
            "Modification time mismatch (diff: {diff} seconds, tolerance: {mtime_tolerance}): NFS \
             reports {}.{:09} seconds, filesystem shows {:?}",
            nfs_attrs.mtime.seconds,
            nfs_attrs.mtime.nseconds,
            fs_mtime
        );
    }

    // Compare access time (atime with 10-second tolerance on Windows, 2-second on Unix)
    // Windows atime updates are often disabled or delayed for performance
    let fs_atime = metadata.accessed()?;
    let nfs_atime: SystemTime = nfs_attrs.atime.into();

    #[cfg(windows)]
    let atime_tolerance = 10;
    #[cfg(not(windows))]
    let atime_tolerance = 2;

    let diff = time_diff_secs(nfs_atime, fs_atime);
    if diff > atime_tolerance {
        bail!(
            "Access time mismatch (diff: {} seconds, tolerance: {}): NFS reports {}.{:09} \
             seconds, filesystem shows {:?}",
            diff,
            atime_tolerance,
            nfs_attrs.atime.seconds,
            nfs_attrs.atime.nseconds,
            fs_atime
        );
    }

    Ok(())
}

/// Create a test file at `fs_path` with exactly `size` bytes by writing in blocks.
/// Creates parent directories if needed.
pub fn create_test_file(fs_path: &std::path::Path, size: u64) -> anyhow::Result<()> {
    const BLOCK_SIZE: usize = 64 * 1024;

    if let Some(parent) = fs_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(fs_path)?;

    let mut remaining = size;
    let mut buf = vec![0u8; BLOCK_SIZE];

    while remaining > 0 {
        buf.write_all(format!("test file content at {remaining}, full {size}").as_bytes())?;
        let to_write = std::cmp::min(remaining, BLOCK_SIZE as u64) as usize;
        file.write_all(&buf[..to_write])?;
        remaining -= to_write as u64;
    }

    file.sync_all()?;

    Ok(())
}

pub async fn assert_files_equal(
    test_dir_path: &Path,
    test_dir_handle: &nfs3_types::nfs3::nfs_fh3,
    filename: &str,
    expected_len: u64,
    ctx: &mut TestContext,
) {
    let nfs_file_handle = ctx
        .just_lookup(test_dir_handle, filename)
        .await
        .expect("failed to lookup file on NFS server");

    let nfs_attr = ctx
        .just_getattr(&nfs_file_handle)
        .await
        .expect("failed to get fattr3");

    assert_files_equal_ex(
        test_dir_path,
        &nfs_file_handle,
        &nfs_attr,
        filename,
        expected_len,
        ctx,
    )
    .await;
}

pub async fn assert_files_equal_ex(
    test_dir_path: &Path,
    nfs_file_handle: &nfs3_types::nfs3::nfs_fh3,
    nfs_attr: &fattr3,
    filename: &str,
    expected_len: u64,
    ctx: &mut TestContext,
) {
    use std::fs::File;
    use std::io::Read;

    const BUFFER_SIZE: usize = 1024 * 1024;

    let path = test_dir_path.join(filename);
    let mut local_file = File::open(path).expect("failed to open local file");
    let local_metadata = local_file
        .metadata()
        .expect("failed to get local file metadata");
    assert!(local_metadata.is_file(), "local path is not a regular file");
    assert_eq!(
        local_metadata.len(),
        expected_len,
        "local file size does not match expected length"
    );

    assert_eq!(
        nfs_attr.type_,
        ftype3::NF3REG,
        "NFS file is not a regular file"
    );

    assert_eq!(
        nfs_attr.size,
        local_metadata.len(),
        "file size mismatch between NFS and local file"
    );

    assert_eq!(
        SystemTime::from(nfs_attr.mtime),
        local_metadata.modified().unwrap(),
        "modification time mismatch between NFS and local file"
    );

    // assert_eq!(
    //     SystemTime::from(nfs_attr.atime),
    //     local_metadata.accessed().unwrap(),
    //     "access time mismatch between NFS and local file"
    // );

    assert_eq!(
        SystemTime::from(nfs_attr.ctime),
        local_metadata.created().unwrap(),
        "creation time mismatch between NFS and local file"
    );

    let remaining = nfs_attr.size;
    let mut offset: u64 = 0;
    let mut local_buffer = vec![0u8; BUFFER_SIZE];

    while offset < remaining {
        let nfs_read_result = ctx
            .client
            .read(&nfs3_types::nfs3::READ3args {
                file: nfs_file_handle.clone(),
                offset,
                count: BUFFER_SIZE as u32,
            })
            .await
            .expect("failed to read from NFS file");

        let nfs_data = match nfs_read_result {
            Nfs3Result::Ok(ok) => ok.data,
            Nfs3Result::Err((status, _)) => {
                panic!("NFS read failed with status {status}");
            }
        };

        let nfs_bytes_read = nfs_data.len();
        let local_bytes_read = local_file
            .read(&mut local_buffer)
            .expect("failed to read from local file");

        assert_eq!(
            nfs_bytes_read, local_bytes_read,
            "mismatched byte counts read"
        );

        assert_eq!(
            &nfs_data[..],
            &local_buffer[..local_bytes_read],
            "data mismatch between NFS and local file"
        );

        offset += nfs_bytes_read as u64;
    }
}

pub async fn assert_folders_equal(
    test_dir_path: &Path,
    test_dir_handle: &nfs3_types::nfs3::nfs_fh3,
    foldername: &str,
    ctx: &mut TestContext,
) {
    let nfs_folder_handle = ctx
        .just_lookup(test_dir_handle, foldername)
        .await
        .expect("failed to lookup folder on NFS server");

    let nfs_attr = ctx
        .just_getattr(&nfs_folder_handle)
        .await
        .expect("failed to get fattr3");

    assert_folders_equal_ex(
        test_dir_path,
        &nfs_folder_handle,
        &nfs_attr,
        foldername,
        ctx,
    )
    .await;
}

pub async fn assert_folders_equal_ex(
    test_dir_path: &Path,
    nfs_dir_handle: &nfs3_types::nfs3::nfs_fh3,
    nfs_attr: &fattr3,
    foldername: &str,
    ctx: &mut TestContext,
) {
    let path = test_dir_path.join(foldername);
    let local_metadata = std::fs::metadata(&path).expect("failed to get local folder metadata");
    assert!(
        local_metadata.is_dir(),
        "`{}` is not a directory",
        path.display()
    );

    assert_eq!(nfs_attr.type_, ftype3::NF3DIR);
    assert_eq!(nfs_attr.size, local_metadata.len());

    let nfs_mtime = SystemTime::from(nfs_attr.mtime);
    let local_mtime = local_metadata.modified().unwrap();
    assert_eq!(nfs_mtime, local_mtime);

    let nfs_ctime = SystemTime::from(nfs_attr.ctime);
    let local_ctime = local_metadata.created().unwrap();
    assert_eq!(nfs_ctime, local_ctime);

    let nfs_entries = ctx
        .just_readdir(nfs_dir_handle)
        .await
        .expect("failed to read directory entries from NFS server");

    let mut nfs_entry_names: Vec<String> = nfs_entries
        .iter()
        .map(|entry| {
            String::from_utf8(entry.name.as_ref().into()).expect("invalid UTF-8 in NFS entry name")
        })
        .collect();
    nfs_entry_names.sort();

    let local_entries = std::fs::read_dir(&path).expect("failed to read local directory entries");
    let mut local_entry_names: Vec<String> = local_entries
        .map(|entry| {
            entry
                .expect("failed to read local directory entry")
                .file_name()
                .into_string()
                .expect("invalid UTF-8 in local entry name")
        })
        .collect();
    local_entry_names.sort();

    assert_eq!(nfs_entry_names, local_entry_names);
}
