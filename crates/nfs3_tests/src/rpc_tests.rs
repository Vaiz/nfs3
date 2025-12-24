use anyhow::bail;
use nfs3_client::nfs3_types::nfs3::nfs_fh3;
use nfs3_client::nfs3_types::rpc::{
    accept_stat_data, fragment_header, msg_body, reply_body, rpc_msg,
};
use nfs3_client::nfs3_types::xdr_codec::{Pack, Unpack};
use nfs3_server::memfs::{MemFs, MemFsConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

use crate::{Server, init_logging};

pub struct RpcTestContext {
    server_handle: tokio::task::JoinHandle<anyhow::Result<()>>,
    client_stream: DuplexStream,
    root_dir: nfs_fh3,
}

impl RpcTestContext {
    const EOF_FLAG: u32 = 0x8000_0000;

    pub fn setup() -> Self {
        let mut config = MemFsConfig::default();

        config.add_file("/a.txt", "hello world\n".as_bytes());
        config.add_file("/b.txt", "Greetings to xet data\n".as_bytes());
        config.add_dir("/another_dir");
        config.add_file("/another_dir/thisworks.txt", "i hope\n".as_bytes());

        Self::setup_with_config(config, tracing::Level::DEBUG)
    }

    pub fn setup_with_config(fs_config: MemFsConfig, log_level: tracing::Level) -> Self {
        init_logging(log_level);

        let memfs = MemFs::new(fs_config).unwrap();
        let (server, client) = tokio::io::duplex(1024 * 1024);
        let server = Server::new(server, memfs).unwrap();
        let root_dir = server.root_dir();
        let server_handle = tokio::task::spawn(server.run());

        Self {
            server_handle,
            client_stream: client,
            root_dir,
        }
    }
    pub fn root_dir(&self) -> &nfs_fh3 {
        &self.root_dir
    }

    pub async fn shutdown(self) -> anyhow::Result<()> {
        let Self {
            server_handle,
            client_stream,
            root_dir: _,
        } = self;

        drop(client_stream);

        server_handle.await?
    }

    pub async fn send_call<T>(&mut self, msg: &rpc_msg<'_, '_>, args: &T) -> anyhow::Result<()>
    where
        T: Pack,
    {
        let total_len = msg.packed_size() + args.packed_size();
        if total_len % 4 != 0 {
            anyhow::bail!("Total length is not a multiple of 4: {total_len}");
        }

        let mut buf = Vec::with_capacity(total_len + 4);
        let fragment_header = total_len as u32 | Self::EOF_FLAG;
        fragment_header.pack(&mut buf)?;
        msg.pack(&mut buf)?;
        args.pack(&mut buf)?;
        if buf.len() - 4 != total_len {
            anyhow::bail!(
                "Buffer length does not match total length: {} != {total_len}",
                buf.len() - 4
            );
        }
        self.client_stream.write_all(&buf).await?;
        Ok(())
    }

    pub async fn recv_reply<T>(&mut self) -> anyhow::Result<(rpc_msg<'_, '_>, Option<T>)>
    where
        T: Unpack,
    {
        let mut buf = [0u8; 4];
        self.client_stream.read_exact(&mut buf).await?;
        let fragment_header: fragment_header = buf.into();
        if !fragment_header.eof() {
            panic!("Fragment header does not have EOF flag");
        }

        let total_len = fragment_header.fragment_length();
        let mut buf = vec![0u8; total_len as usize];
        self.client_stream.read_exact(&mut buf).await?;

        let mut cursor = std::io::Cursor::new(buf);
        let (resp_msg, _) = rpc_msg::unpack(&mut cursor)?;

        let reply = match &resp_msg.body {
            msg_body::REPLY(reply_body::MSG_ACCEPTED(reply)) => reply,
            msg_body::REPLY(reply_body::MSG_DENIED(_)) => return Ok((resp_msg, None)),
            msg_body::CALL(_) => bail!("Unexpected call"),
        };

        if let accept_stat_data::SUCCESS = reply.reply_data {
        } else {
            return Ok((resp_msg, None));
        }

        let (final_value, _) = T::unpack(&mut cursor)?;
        if cursor.position() as usize != total_len as usize {
            let pos = cursor.position() as usize;
            bail!("Cursor position does not match total length: {pos} != {total_len}");
        }
        Ok((resp_msg, Some(final_value)))
    }
}
