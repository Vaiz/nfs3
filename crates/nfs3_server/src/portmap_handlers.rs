use nfs3_types::portmap::mapping;
use tracing::debug;

use crate::context::RPCContext;

// We fake a portmapper here. And always direct back to the same host port
#[expect(
    clippy::unused_async,
    reason = "Uniform asynchronous metadata dispatch interface"
)]
pub async fn pmapproc_getport<T>(context: RPCContext<T>, xid: u32, m: mapping) -> u32
where
    T: crate::vfs::NfsFileSystem,
{
    debug!("pmapproc_getport({xid}, {m:?})");
    let port = u32::from(context.local_port);
    debug!("\t{xid} --> {port}");
    port
}
