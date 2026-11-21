use std::num::NonZeroUsize;

use tokio::io::{AsyncRead, AsyncWrite, BufReader};
use tracing::debug;

use crate::context::RPCContext;
use crate::rpcwire::messages::RpcRecordReader;
use crate::rpcwire::{SocketMessageHandler, write_fragment};
use crate::units::KIBIBYTE;
use crate::vfs::NfsFileSystem;

const READ_BUFFER_SIZE: usize = 128 * KIBIBYTE as usize;

/// processes an established socket
pub(crate) async fn process_socket<IO, T>(
    socket: IO,
    context: RPCContext<T>,
    max_rpc_record_size: Option<NonZeroUsize>,
) -> Result<(), anyhow::Error>
where
    IO: AsyncRead + AsyncWrite + Unpin,
    T: NfsFileSystem + 'static,
{
    let (reader, mut writer) = tokio::io::split(socket);
    let reader = BufReader::with_capacity(READ_BUFFER_SIZE, reader);
    let reader = RpcRecordReader::new(reader, max_rpc_record_size);
    let (handler, mut replies) = SocketMessageHandler::new(context);

    // Race the whole loops so replies cannot interrupt a partially read record,
    // and a parser failure can close the socket even during a blocked write.
    tokio::select! {
        biased;
        result = handler.read_and_dispatch(reader) => result,
        result = async {
            while let Some(reply) = replies.recv().await {
                let Ok(reply) = reply else { anyhow::bail!("Error handling RPC message") };
                write_fragment(&mut writer, reply).await?;
            }
            Ok(())
        } => result,
    }
    .inspect_err(|e| debug!("Message handling error: {e}"))
}
