#![doc = include_str!("../README.md")]

mod context;
mod mount_handlers;
pub(crate) mod nfs_ext;
mod nfs_handlers;
mod portmap_handlers;
mod rpc;
mod rpcwire;

pub mod fs_util;

pub mod tcp;
mod transaction_tracker;
pub(crate) mod units;
pub mod vfs;

/// Reexport for test purposes
#[doc(hidden)]
#[cfg(feature = "__test_reexports")]
pub mod test_reexports {
    pub use crate::context::RPCContext;
    pub use crate::transaction_tracker::TransactionTracker;

    pub async fn process_socket<IO>(socket: IO, context: RPCContext) -> Result<(), anyhow::Error>
    where
        IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + 'static,
    {
        crate::tcp::process_socket(socket, context).await
    }
}
