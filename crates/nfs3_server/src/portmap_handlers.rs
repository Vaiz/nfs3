use nfs3_types::portmap::{self, PMAP_PROG, mapping};
use nfs3_types::rpc::accept_stat_data;
use nfs3_types::xdr_codec::Void;
use tracing::{debug, error};

use crate::context::RPCContext;
use crate::rpcwire::messages::{IncomingRpcMessage, OutgoingRpcMessage};

#[macro_export]
macro_rules! unpack_message {
    ($message:expr, $type:ty) => {
        match $message.unpack_message::<$type>() {
            Ok(unpacked) => unpacked,
            Err(err) => {
                error!("Failed to unpack message: {err}");
                return Ok(Some(
                    $message.into_error_reply(accept_stat_data::GARBAGE_ARGS),
                ));
            }
        }
    };
}

pub fn handle_portmap(
    context: &RPCContext,
    message: IncomingRpcMessage,
) -> Result<Option<OutgoingRpcMessage>, anyhow::Error> {
    let call = message.body();
    if call.vers != portmap::VERSION {
        error!(
            "Invalid Portmap Version number {} != {}",
            call.vers,
            portmap::VERSION
        );
        return Ok(Some(OutgoingRpcMessage::accept_error(
            message.xid(),
            accept_stat_data::PROG_MISMATCH {
                low: portmap::VERSION,
                high: portmap::VERSION,
            },
        )));
    }

    let prog = PMAP_PROG::try_from(call.proc);
    match prog {
        Ok(PMAP_PROG::PMAPPROC_NULL) => pmapproc_null(message),
        Ok(PMAP_PROG::PMAPPROC_GETPORT) => pmapproc_getport(context, message),
        _ => Ok(Some(OutgoingRpcMessage::accept_error(
            message.xid(),
            accept_stat_data::PROC_UNAVAIL,
        ))),
    }
}

pub fn pmapproc_null(
    message: IncomingRpcMessage,
) -> Result<Option<OutgoingRpcMessage>, anyhow::Error> {
    debug!("pmapproc_null({})", message.xid());
    let _ = unpack_message!(message, Void);
    Ok(Some(message.into_success_reply(Box::new(Void))))
}

// We fake a portmapper here. And always direct back to the same host port
pub fn pmapproc_getport(
    context: &RPCContext,
    message: IncomingRpcMessage,
) -> Result<Option<OutgoingRpcMessage>, anyhow::Error> {
    let mapping = unpack_message!(message, mapping);

    debug!("pmapproc_getport({}, {mapping:?})", message.xid());
    let port = u32::from(context.local_port);
    debug!("\t{} --> {}", message.xid(), port);
    Ok(Some(message.into_success_reply(Box::new(port))))
}
