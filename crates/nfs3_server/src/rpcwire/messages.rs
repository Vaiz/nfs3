use std::any;
use std::io::Cursor;

use anyhow::bail;
use nfs3_types::rpc::{accept_stat_data, accepted_reply, call_body, fragment_header, opaque_auth, rpc_msg};
use nfs3_types::xdr_codec::{Pack, PackedSize, Unpack, Void};
use tokio::io::{AsyncRead, AsyncReadExt};
use nfs3_types::rpc::msg_body;
use nfs3_types::rpc::reply_body;
use tracing::error;

use crate::rpc::rpc_vers_mismatch;

pub struct PackedRpcMessage {
    header: fragment_header,
    data: Vec<u8>,
}

impl PackedRpcMessage {
    pub async fn recv<T>(input: &mut T) -> anyhow::Result<Self>
    where 
        T: AsyncRead + Unpin,
     {
        let mut header_buf = [0_u8; 4];
        input.read_exact(&mut header_buf).await?;
        let header: fragment_header = header_buf.into();
        let length = header.fragment_length() as usize;

        let mut data = vec![0_u8; length];
        input.read_exact(&mut data).await?;
        Ok(Self { header, data })
    }
}

pub struct IncomingRpcMessage {
    header: fragment_header,
    xid: u32,
    body: call_body<'static>,
    data: Vec<u8>,
    message_start: usize, // offset of the start of the message in the data buffer
}

impl TryFrom<PackedRpcMessage> for IncomingRpcMessage {
    type Error = anyhow::Error;

    fn try_from(packed: PackedRpcMessage) -> Result<Self, Self::Error> {
        let (rpc, pos) = match rpc_msg::unpack(&mut Cursor::new(&packed.data)) {
            Ok(ok) => ok,
            Err(err) => {
                bail!("Failed to unpack RPC message: {err}");
            }
        };

        let xid = rpc.xid;
        let body = match rpc.body {
            msg_body::CALL(call) => call,
            msg_body::REPLY(_) => {
                bail!("Expected a CALL message, got REPLY. XID: {xid}");
            }
        };

        Ok(Self {
            header: packed.header,
            xid,
            body,
            data: packed.data,
            message_start: pos,
        })
    }
}

impl IncomingRpcMessage {
    pub fn xid(&self) -> u32 {
        self.xid
    }
    pub fn body(&self) -> &call_body<'static> {
        &self.body
    }
    pub fn unpack_message<'a, T: Unpack<Cursor<&'a [u8]>>>(
        &'a self,
    ) -> Result<T, anyhow::Error> {
        let slice = &self.data[self.message_start..];
        let mut cursor = Cursor::new(slice);
        let (msg, pos) = T::unpack(&mut cursor)?;
        if pos != slice.len() {
            bail!("Unpacked message size does not match expected size");
        }
        Ok(msg)
    }
}

pub trait Message: Pack<Cursor<Vec<u8>>> {}

impl<T> Message for T where T: Pack<Cursor<Vec<u8>>> {}

pub struct OutgoingRpcMessage {
    rpc: rpc_msg<'static, 'static>,
    message: Box<dyn Message>,
    message_size: usize,
}

impl OutgoingRpcMessage {
    pub fn new<T>(rpc: rpc_msg<'static, 'static>, message: Box<T>) -> Self
    where
        T: Message + PackedSize + 'static,
    {
        let message_size = message.packed_size() as usize;
        Self {
            rpc,
            message,
            message_size,
        }
    }

    pub fn rpc_mismatch(xid: u32) -> Self {
        let rpc = rpc_vers_mismatch(xid);
        Self {
            rpc,
            message: Box::new(Void),
            message_size: 0,
        }
    }

    pub fn accept_error(xid: u32, err: accept_stat_data) -> Self {
        let rpc = rpc_msg {
            xid,
            body: msg_body::REPLY(reply_body::MSG_ACCEPTED(
                accepted_reply {
                    verf: opaque_auth::default(),
                    reply_data: err,
                },
            )),
        };
        Self {
            rpc,
            message: Box::new(Void),
            message_size: 0,
        }
    }


}
