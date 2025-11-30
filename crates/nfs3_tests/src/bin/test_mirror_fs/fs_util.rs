use std::path::Path;
use std::time::SystemTime;

use anyhow::bail;
use nfs3_types::nfs3::{fattr3, ftype3};

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
            "Modification time mismatch (diff: {diff} seconds, tolerance: {mtime_tolerance}): NFS reports \
                 {}.{:09} seconds, filesystem shows {:?}",
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

/// Compares NFS attributes with actual filesystem metadata (lenient version).
///
/// This version skips timestamp validation, useful for root directories or
/// other cases where the server may cache metadata and not refresh it.
pub fn assert_attributes_match_lenient(
    nfs_attrs: &fattr3,
    fs_path: &Path,
    expected_type: ftype3,
) -> anyhow::Result<()> {
    let metadata = std::fs::metadata(fs_path)?;

    // Check file type
    if nfs_attrs.type_ != expected_type {
        bail!(
            "File type mismatch: NFS reports {:?}, expected {:?}",
            nfs_attrs.type_,
            expected_type
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

    // Skip timestamp validation - server may cache metadata
    Ok(())
}
