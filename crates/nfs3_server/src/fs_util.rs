#![cfg_attr(target_os = "windows", allow(unused_imports))]

use std::fs::{File, Metadata, Permissions};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use nfs3_types::nfs3::*;
use tokio::fs::OpenOptions;
use tracing::debug;

/// Compares if file metadata has changed in a significant way
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub fn metadata_differ(lhs: &Metadata, rhs: &Metadata) -> bool {
    lhs.ino() != rhs.ino()
        || lhs.mtime() != rhs.mtime()
        || lhs.len() != rhs.len()
        || lhs.file_type() != rhs.file_type()
}
pub fn fattr3_differ(lhs: &fattr3, rhs: &fattr3) -> bool {
    lhs.fileid != rhs.fileid
        || lhs.mtime != rhs.mtime
        || lhs.size != rhs.size
        || lhs.type_ != rhs.type_
}

/// path.exists() is terrifyingly unsafe as that
/// traverses symlinks. This can cause deadlocks if we have a
/// recursive symlink.
pub fn exists_no_traverse(path: &Path) -> bool {
    path.symlink_metadata().is_ok()
}

fn mode_unmask(mode: u32) -> u32 {
    // it is possible to create a file we cannot write to.
    // we force writable always.
    /*
    let mode = mode | 0x80;
    let mode = Permissions::from_mode(mode);
    mode.mode() & 0x1FF
     */

    (mode | 0x80) & 0x1FF
}

#[cfg(unix)]
fn set_permissions_on_path(path: impl AsRef<Path>, mode: u32) -> std::io::Result<()> {
    std::fs::set_permissions(path, Permissions::from_mode(mode))
}
#[cfg(not(unix))]
fn set_permissions_on_path(_path: impl AsRef<Path>, _mode: u32) -> std::io::Result<()> {
    debug!("setting permissions is not supported");
    Ok(())
}

#[cfg(unix)]
fn get_mode(metadata: &Metadata) -> u32 {
    metadata.mode()
}
#[cfg(not(unix))]
fn get_mode(metadata: &Metadata) -> u32 {
    // Assume full `rwxrwxrwx` permissions if not read-only
    if metadata.permissions().readonly() {
        0o444 // Readable by all, not writable
    } else {
        0o666 // Readable and writable by all
    }
}

#[cfg(unix)]
fn set_permissions_on_file(file: &File, mode: u32) -> std::io::Result<()> {
    file.set_permissions(Permissions::from_mode(mode))
}
#[cfg(not(unix))]
fn set_permissions_on_file(_file: &File, _mode: u32) -> std::io::Result<()> {
    debug!("setting permissions is not supported");
    Ok(())
}

#[cfg(unix)]
fn get_uid(metadata: &Metadata) -> u32 {
    metadata.uid()
}
#[cfg(not(unix))]
fn get_uid(_metadata: &Metadata) -> u32 {
    1000
}

#[cfg(unix)]
fn get_gid(metadata: &Metadata) -> u32 {
    metadata.gid()
}
#[cfg(not(unix))]
fn get_gid(_metadata: &Metadata) -> u32 {
    1000
}

fn to_nfstime3(time: std::io::Result<std::time::SystemTime>) -> nfstime3 {
    match time {
        Ok(time) => time.try_into().unwrap_or_default(),
        Err(_) => nfstime3::default(),
    }
}

/// Converts fs Metadata to NFS fattr3
pub fn metadata_to_fattr3(fid: fileid3, meta: &Metadata) -> fattr3 {
    let size = meta.len();
    let file_mode = mode_unmask(get_mode(meta));
    if meta.is_file() {
        fattr3 {
            type_: ftype3::NF3REG,
            mode: file_mode,
            nlink: 1,
            uid: get_uid(meta),
            gid: get_gid(meta),
            size,
            used: size,
            rdev: specdata3::default(),
            fsid: 0,
            fileid: fid,
            atime: to_nfstime3(meta.accessed()),
            mtime: to_nfstime3(meta.modified()),
            ctime: to_nfstime3(meta.created()),
        }
    } else if meta.is_symlink() {
        fattr3 {
            type_: ftype3::NF3LNK,
            mode: file_mode,
            nlink: 1,
            uid: get_uid(meta),
            gid: get_gid(meta),
            size,
            used: size,
            rdev: specdata3::default(),
            fsid: 0,
            fileid: fid,
            atime: to_nfstime3(meta.accessed()),
            mtime: to_nfstime3(meta.modified()),
            ctime: to_nfstime3(meta.created()),
        }
    } else {
        fattr3 {
            type_: ftype3::NF3DIR,
            mode: file_mode,
            nlink: 2,
            uid: get_uid(meta),
            gid: get_gid(meta),
            size,
            used: size,
            rdev: specdata3::default(),
            fsid: 0,
            fileid: fid,
            atime: to_nfstime3(meta.accessed()),
            mtime: to_nfstime3(meta.modified()),
            ctime: to_nfstime3(meta.created()),
        }
    }
}

/// Set attributes of a path
pub async fn path_setattr(path: &Path, setattr: &sattr3) -> Result<(), nfsstat3> {
    match &setattr.atime {
        set_atime::SET_TO_SERVER_TIME => {
            let _ = filetime::set_file_atime(path, filetime::FileTime::now());
        }
        set_atime::SET_TO_CLIENT_TIME(time) => {
            let time = filetime::FileTime::from_unix_time(time.seconds as i64, time.nseconds);
            let _ = filetime::set_file_atime(path, time);
        }
        _ => {}
    };
    match &setattr.mtime {
        set_mtime::SET_TO_SERVER_TIME => {
            let _ = filetime::set_file_mtime(path, filetime::FileTime::now());
        }
        set_mtime::SET_TO_CLIENT_TIME(time) => {
            let time = filetime::FileTime::from_unix_time(time.seconds as i64, time.nseconds);
            let _ = filetime::set_file_mtime(path, time);
        }
        _ => {}
    };
    if let set_mode3::Some(mode) = setattr.mode {
        debug!(" -- set permissions {:?} {:?}", path, mode);
        let mode = mode_unmask(mode);
        let _ = set_permissions_on_path(path, mode);
    };
    if let set_uid3::Some(_) = setattr.uid {
        debug!("Set uid not implemented");
    }
    if let set_gid3::Some(_) = setattr.gid {
        debug!("Set gid not implemented");
    }
    if let set_size3::Some(size3) = setattr.size {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)
            .await
            .or(Err(nfsstat3::NFS3ERR_IO))?;
        debug!(" -- set size {:?} {:?}", path, size3);
        file.set_len(size3).await.or(Err(nfsstat3::NFS3ERR_IO))?;
    }
    Ok(())
}

/// Set attributes of a file
pub async fn file_setattr(file: &std::fs::File, setattr: &sattr3) -> Result<(), nfsstat3> {
    if let set_mode3::Some(mode) = setattr.mode {
        debug!(" -- set permissions {:?}", mode);
        let mode = mode_unmask(mode);
        let _ = set_permissions_on_file(file, mode);
    }
    if let set_size3::Some(size3) = setattr.size {
        debug!(" -- set size {:?}", size3);
        file.set_len(size3).or(Err(nfsstat3::NFS3ERR_IO))?;
    }
    Ok(())
}
