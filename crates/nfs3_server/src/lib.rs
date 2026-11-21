#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc = include_str!("../README.md")]

mod context;
mod mount_handlers;
pub(crate) mod nfs_ext;
mod nfs_handlers;
mod portmap_handlers;

#[cfg(feature = "fs_util")]
#[cfg_attr(docsrs, doc(cfg(feature = "fs_util")))]
pub mod fs_util;

mod bounded;
pub mod tcp;
pub(crate) mod units;
pub mod vfs;

#[cfg(feature = "memfs")]
#[cfg_attr(docsrs, doc(cfg(feature = "memfs")))]
pub mod memfs;

/// Re-export of `nfs3_types` for convenience
pub use nfs3_types;

/// Reexport for test purposes
#[doc(hidden)]
#[cfg(feature = "__test_reexports")]
pub mod test_reexports {
    pub use crate::bounded::test_support::TestConnection;
}
