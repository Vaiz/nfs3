use std::io::{Read, Write};

use nfs3_types::portmap;
use nfs3_types::portmap::PMAP_PROG;
use nfs3_types::rpc::*;
use tracing::{debug, error};
use xdr_codec::{Pack, Unpack};

use crate::context::RPCContext;
use crate::rpc::*;

pub fn handle_portmap(
    xid: u32,
    call: call_body<'_>,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    if call.vers != portmap::VERSION {
        error!(
            "Invalid Portmap Version number {} != {}",
            call.vers,
            portmap::VERSION
        );
        prog_mismatch_reply_message(xid, portmap::VERSION).pack(output)?;
        return Ok(());
    }
    let prog = PMAP_PROG::try_from(call.proc);

    match prog {
        Ok(PMAP_PROG::PMAPPROC_NULL) => pmapproc_null(xid, input, output)?,
        Ok(PMAP_PROG::PMAPPROC_GETPORT) => pmapproc_getport(xid, input, output, context)?,
        _ => {
            proc_unavail_reply_message(xid).pack(output)?;
        }
    }
    Ok(())
}

pub fn pmapproc_null(
    xid: u32,
    _: &mut impl Read,
    output: &mut impl Write,
) -> Result<(), anyhow::Error> {
    debug!("pmapproc_null({:?}) ", xid);
    // build an RPC reply
    let msg = make_success_reply(xid);
    debug!("\t{:?} --> {:?}", xid, msg);
    msg.pack(output)?;
    Ok(())
}

/*
 * We fake a portmapper here. And always direct back to the same host port
 */
pub fn pmapproc_getport(
    xid: u32,
    read: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let mapping = portmap::mapping::unpack(read)?.0;
    debug!("pmapproc_getport({:?}, {:?}) ", xid, mapping);
    make_success_reply(xid).pack(output)?;
    let port = context.local_port as u32;
    debug!("\t{:?} --> {:?}", xid, port);
    port.pack(output)?;
    Ok(())
}
