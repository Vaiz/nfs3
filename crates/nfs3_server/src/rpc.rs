use nfs3_types::rpc::{accept_stat_data as accept_body, msg_body as rpc_body, *};

pub fn proc_unavail_reply_message(xid: u32) -> rpc_msg<'static, 'static> {
    let reply = reply_body::MSG_ACCEPTED(accepted_reply {
        verf: opaque_auth::default(),
        reply_data: accept_body::PROC_UNAVAIL,
    });
    rpc_msg {
        xid,
        body: rpc_body::REPLY(reply),
    }
}
pub fn prog_unavail_reply_message(xid: u32) -> rpc_msg<'static, 'static> {
    let reply = reply_body::MSG_ACCEPTED(accepted_reply {
        verf: opaque_auth::default(),
        reply_data: accept_body::PROG_UNAVAIL,
    });
    rpc_msg {
        xid,
        body: rpc_body::REPLY(reply),
    }
}
pub fn prog_mismatch_reply_message(xid: u32, accepted_ver: u32) -> rpc_msg<'static, 'static> {
    let reply = reply_body::MSG_ACCEPTED(accepted_reply {
        verf: opaque_auth::default(),
        reply_data: accept_body::PROG_MISMATCH {
            low: accepted_ver,
            high: accepted_ver,
        },
    });
    rpc_msg {
        xid,
        body: rpc_body::REPLY(reply),
    }
}
pub fn garbage_args_reply_message(xid: u32) -> rpc_msg<'static, 'static> {
    let reply = reply_body::MSG_ACCEPTED(accepted_reply {
        verf: opaque_auth::default(),
        reply_data: accept_body::GARBAGE_ARGS,
    });
    rpc_msg {
        xid,
        body: rpc_body::REPLY(reply),
    }
}

pub fn system_err_reply_message(xid: u32) -> rpc_msg<'static, 'static> {
    let reply = reply_body::MSG_ACCEPTED(accepted_reply {
        verf: opaque_auth::default(),
        reply_data: accept_body::SYSTEM_ERR,
    });
    rpc_msg {
        xid,
        body: rpc_body::REPLY(reply),
    }
}

pub fn rpc_vers_mismatch(xid: u32) -> rpc_msg<'static, 'static> {
    use nfs3_types::rpc::RPC_VERSION_2;
    let reply = reply_body::MSG_DENIED(rejected_reply::rpc_mismatch(RPC_VERSION_2, RPC_VERSION_2));
    rpc_msg {
        xid,
        body: rpc_body::REPLY(reply),
    }
}

pub fn make_success_reply(xid: u32) -> rpc_msg<'static, 'static> {
    let reply = reply_body::MSG_ACCEPTED(accepted_reply {
        verf: opaque_auth::default(),
        reply_data: accept_body::SUCCESS,
    });
    rpc_msg {
        xid,
        body: rpc_body::REPLY(reply),
    }
}
