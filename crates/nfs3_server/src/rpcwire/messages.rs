use std::io::Cursor;

use nfs3_types::rpc::{accept_stat_data, accepted_reply, fragment_header, opaque_auth, rpc_msg};
use nfs3_types::xdr_codec::{Pack, PackedSize, Unpack, Void};
use tokio::io::{AsyncRead, AsyncReadExt};
use nfs3_types::rpc::msg_body;
use nfs3_types::rpc::reply_body;

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
    rpc: rpc_msg<'static, 'static>,
    data: Vec<u8>,
    message_start: usize, // offset of the start of the message in the data buffer
}

impl TryFrom<PackedRpcMessage> for IncomingRpcMessage {
    type Error = nfs3_types::xdr_codec::Error;

    fn try_from(packed: PackedRpcMessage) -> Result<Self, Self::Error> {
        let (rpc, pos) = rpc_msg::unpack(&mut Cursor::new(&packed.data))?;
        Ok(Self {
            header: packed.header,
            rpc,
            data: packed.data,
            message_start: pos,
        })
    }
}

impl IncomingRpcMessage {
    pub fn rpc(&self) -> &rpc_msg<'static, 'static> {
        &self.rpc
    }
    pub fn unpack_message<'a, T: Unpack<Cursor<&'a [u8]>>>(
        &'a self,
    ) -> Result<T, nfs3_types::xdr_codec::Error> {
        let mut cursor = Cursor::new(&self.data[self.message_start..]);
        T::unpack(&mut cursor).map(|(message, _)| message)
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
