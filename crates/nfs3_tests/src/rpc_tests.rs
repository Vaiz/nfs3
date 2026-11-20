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

    #[must_use]
    pub fn setup() -> Self {
        let mut config = MemFsConfig::default();

        config.add_file("/a.txt", b"hello world\n");
        config.add_file("/b.txt", b"Greetings to xet data\n");
        config.add_dir("/another_dir");
        config.add_file("/another_dir/thisworks.txt", b"i hope\n");

        Self::setup_with_config(config, tracing::Level::DEBUG)
    }

    /// Starts a test server with the given filesystem configuration.
    ///
    /// # Panics
    ///
    /// Panics if filesystem or server setup fails.
    #[must_use]
    pub fn setup_with_config(fs_config: MemFsConfig, log_level: tracing::Level) -> Self {
        init_logging(log_level);

        let memfs = MemFs::new(fs_config).expect("initialize the test filesystem");
        let (server, client) = tokio::io::duplex(1024 * 1024);
        let server = Server::new(server, memfs).expect("initialize the RPC test server");
        let root_dir = server.root_dir();
        let server_handle = tokio::task::spawn(server.run());

        Self {
            server_handle,
            client_stream: client,
            root_dir,
        }
    }
    #[must_use]
    pub const fn root_dir(&self) -> &nfs_fh3 {
        &self.root_dir
    }

    pub async fn shutdown(self) -> anyhow::Result<()> {
        let Self {
            server_handle,
            client_stream,
            ..
        } = self;

        drop(client_stream);

        server_handle.await?
    }

    pub async fn send_call<T>(&mut self, msg: &rpc_msg<'_, '_>, args: &T) -> anyhow::Result<()>
    where
        T: Pack + Sync,
    {
        let total_len = msg.packed_size() + args.packed_size();
        if !total_len.is_multiple_of(4) {
            anyhow::bail!("Total length is not a multiple of 4: {total_len}");
        }

        let mut buf = Vec::with_capacity(total_len + 4);
        let fragment_header = u32::try_from(total_len)? | Self::EOF_FLAG;
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
        anyhow::ensure!(
            fragment_header.eof(),
            "Fragment header does not have EOF flag"
        );

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

        if matches!(reply.reply_data, accept_stat_data::SUCCESS) {
        } else {
            return Ok((resp_msg, None));
        }

        let (final_value, _) = T::unpack(&mut cursor)?;
        let pos = cursor.position();
        if pos != u64::from(total_len) {
            bail!("Cursor position does not match total length: {pos} != {total_len}");
        }
        Ok((resp_msg, Some(final_value)))
    }
}
