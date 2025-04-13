use nfs3_types::rpc::{msg_body as rpc_body, rejected_reply, reply_body, rpc_msg};

pub const fn rpc_vers_mismatch(xid: u32) -> rpc_msg<'static, 'static> {
    use nfs3_types::rpc::RPC_VERSION_2;
    let reply = reply_body::MSG_DENIED(rejected_reply::rpc_mismatch(RPC_VERSION_2, RPC_VERSION_2));
    rpc_msg {
        xid,
        body: rpc_body::REPLY(reply),
    }
}
