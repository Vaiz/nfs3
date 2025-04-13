use nfs3_types::portmap::{self, PMAP_PROG, mapping};
use nfs3_types::rpc::accept_stat_data;
use nfs3_types::xdr_codec::Void;
use tracing::{debug, error, warn};

use crate::context::RPCContext;
use crate::rpcwire::handle;
use crate::rpcwire::messages::{HandleResult, IncomingRpcMessage, OutgoingRpcMessage};

pub async fn handle_portmap(
    context: &RPCContext,
    message: IncomingRpcMessage,
) -> anyhow::Result<HandleResult> {
    let call = message.body();
    if call.vers != portmap::VERSION {
        error!(
            "Invalid Portmap Version number {} != {}",
            call.vers,
            portmap::VERSION
        );
        return OutgoingRpcMessage::accept_error(
            message.xid(),
            accept_stat_data::PROG_MISMATCH {
                low: portmap::VERSION,
                high: portmap::VERSION,
            },
        )
        .try_into();
    }

    let proc = PMAP_PROG::try_from(call.proc);
    match proc {
        Ok(PMAP_PROG::PMAPPROC_NULL) => handle(context, message, pmapproc_null).await,
        Ok(PMAP_PROG::PMAPPROC_GETPORT) => handle(context, message, pmapproc_getport).await,
        _ => {
            warn!("Unimplemented message {}", call.proc);
            message.into_error_reply(accept_stat_data::PROC_UNAVAIL)
        }
    }
}

async fn pmapproc_null(_: &RPCContext, xid: u32, _: Void) -> Void {
    debug!("pmapproc_null({})", xid);
    Void
}

// We fake a portmapper here. And always direct back to the same host port
async fn pmapproc_getport(context: &RPCContext, xid: u32, m: mapping) -> u32 {
    debug!("pmapproc_getport({xid}, {m:?})");
    let port = u32::from(context.local_port);
    debug!("\t{xid} --> {port}");
    port
}
