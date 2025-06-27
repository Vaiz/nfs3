#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc = include_str!("../README.md")]

pub(crate) mod connect;
pub mod error;
pub mod io;
pub(crate) mod mount;
pub mod net;
pub(crate) mod nfs;
pub(crate) mod portmapper;
pub mod rpc;

#[cfg(feature = "tokio")]
#[cfg_attr(docsrs, doc(cfg(feature = "tokio")))]
pub mod tokio;

pub use connect::*;
pub use mount::*;
pub use nfs::*;
/// Re-export of `nfs3_types` for convenience
pub use nfs3_types;
pub use portmapper::*;
