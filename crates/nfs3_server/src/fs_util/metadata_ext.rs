use std::fs::{File, FileType, Permissions};
#[cfg(unix)]
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;
use std::path::Path;

use nfs3_types::nfs3::ftype3;

pub struct NfsMetadataExt<'a>(pub &'a std::fs::Metadata);

#[cfg(unix)]
impl NfsMetadataExt<'_> {
    pub fn mode(&self) -> u32 {
        self.0.mode()
    }
    pub fn nlink(&self) -> u32 {
        self.0.nlink().min(u32::max_value().into()) as u32
    }

    pub fn uid(&self) -> u32 {
        self.0.uid()
    }

    pub fn gid(&self) -> u32 {
        self.0.gid()
    }

    pub fn file_type(&self) -> ftype3 {
        if self.0.is_file() {
            ftype3::NF3REG
        } else if self.0.is_dir() {
            ftype3::NF3DIR
        } else if self.0.is_symlink() {
            ftype3::NF3LNK
        } else if self.0.is_block_device() {
            ftype3::NF3BLK
        } else if self.0.is_char_device() {
            ftype3::NF3CHR
        } else if self.0.is_fifo() {
            ftype3::NF3FIFO
        } else if self.0.is_socket() {
            ftype3::NF3SOCK
        } else {
            ftype3::NF3REG // Default case (though ideally unreachable)
        }
    }

    pub fn set_mode_on_path(path: impl AsRef<Path>, mode: u32) -> std::io::Result<()> {
        std::fs::set_permissions(path, Permissions::from_mode(mode))
    }

    pub fn set_mode_on_file(file: &File, mode: u32) -> std::io::Result<()> {
        file.set_permissions(Permissions::from_mode(mode))
    }
}

#[cfg(windows)]
impl NfsMetadataExt<'_> {
    pub fn mode(&self) -> u32 {
        // Assume full `rwxrwxrwx` permissions if not read-only
        if self.0.permissions().readonly() {
            0o444 // Readable by all, not writable
        } else {
            0o666 // Readable and writable by all
        }
    }

    /// number_of_links is nightly only, issue: 63010
    pub fn nlink(&self) -> u32 {
        if self.0.is_dir() {
            2
        } else {
            1
        }
    }

    pub fn uid(&self) -> u32 {
        1000
    }

    pub fn gid(&self) -> u32 {
        1000
    }

    pub fn file_type(&self) -> ftype3 {
        if self.0.is_file() {
            ftype3::NF3REG
        } else if self.0.is_symlink() {
            ftype3::NF3LNK
        } else if self.0.is_dir() {
            ftype3::NF3DIR
        } else {
            ftype3::NF3REG // Default case (though ideally unreachable)
        }
    }

    pub fn set_mode_on_path(_path: impl AsRef<Path>, _mode: u32) -> std::io::Result<()> {
        tracing::debug!("setting permissions is not supported");
        Ok(())
    }

    pub fn set_mode_on_file(_file: &File, _mode: u32) -> std::io::Result<()> {
        tracing::debug!("setting permissions is not supported");
        Ok(())
    }
}
