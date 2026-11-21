use anyhow::bail;
use nfs3_client::nfs3_types::nfs3::nfs_fh3;
use nfs3_client::nfs3_types::rpc::{
    accept_stat_data, fragment_header, msg_body, reply_body, rpc_msg,
};
use nfs3_client::nfs3_types::xdr_codec::{Decode, Decoder, Encode, Encoder, Payload};
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
        let server = Server::new(server, memfs, crate::server_limits())
            .expect("initialize the RPC test server");
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
        T: Encode + Sync,
    {
        let mut encoder = Encoder::new(crate::client_limits().encode);
        encoder.encode(msg)?;
        encoder.encode(args)?;
        let plan = encoder.finish()?;
        let total_len = plan.len();
        if !total_len.is_multiple_of(4) {
            anyhow::bail!("Total length is not a multiple of 4: {total_len}");
        }

        let mut buf = Vec::with_capacity(total_len + 4);
        let fragment_header = u32::try_from(total_len)? | Self::EOF_FLAG;
        buf.extend_from_slice(&fragment_header.to_be_bytes());
        for chunk in plan.chunks() {
            buf.extend_from_slice(chunk);
        }
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
        T: Decode,
    {
        let mut buf = [0u8; 4];
        self.client_stream.read_exact(&mut buf).await?;
        let fragment_header: fragment_header = buf.into();
        anyhow::ensure!(
            fragment_header.eof(),
            "Fragment header does not have EOF flag"
        );

        let total_len = fragment_header.fragment_length();
        anyhow::ensure!(total_len as usize <= crate::client_limits().record.max_record_bytes);
        let mut buf = vec![0u8; total_len as usize];
        self.client_stream.read_exact(&mut buf).await?;

        let policy = crate::client_limits().decode;
        let mut decoder = Decoder::new(Payload::try_from_vec(buf, policy.payload)?, policy);
        let resp_msg: rpc_msg<'static, 'static> = decoder.decode()?;

        let reply = match &resp_msg.body {
            msg_body::REPLY(reply_body::MSG_ACCEPTED(reply)) => reply,
            msg_body::REPLY(reply_body::MSG_DENIED(_)) => {
                decoder.finish()?;
                return Ok((resp_msg, None));
            }
            msg_body::CALL(_) => bail!("Unexpected call"),
        };

        if matches!(reply.reply_data, accept_stat_data::SUCCESS) {
        } else {
            decoder.finish()?;
            return Ok((resp_msg, None));
        }

        let final_value = decoder.decode()?;
        decoder.finish()?;
        Ok((resp_msg, Some(final_value)))
    }
}

/// Explicit test-only metadata materialization for byte fixtures and size oracles.
/// Encodes an RPC wire fixture.
///
/// # Panics
///
/// Panics if the fixture exceeds the explicit codec limits.
#[must_use]
pub fn encode(value: &impl Encode) -> Vec<u8> {
    let mut encoder = Encoder::new(crate::client_limits().encode);
    encoder.encode(value).expect("encode the RPC fixture");
    encoder
        .finish()
        .expect("finish the RPC fixture")
        .chunks()
        .flatten()
        .copied()
        .collect()
}
