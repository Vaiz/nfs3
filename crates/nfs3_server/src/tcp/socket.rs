use std::num::NonZeroUsize;

use tokio::io::{AsyncRead, AsyncWrite, BufReader};
use tracing::debug;

use crate::context::RPCContext;
use crate::rpcwire::{SocketMessageHandler, write_fragment};
use crate::units::KIBIBYTE;
use crate::vfs::NfsFileSystem;

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
    let mut reader = BufReader::with_capacity(128 * KIBIBYTE as usize, reader);
    let (handler, mut replies) = SocketMessageHandler::new(context, max_rpc_record_size);

    // Race the whole loops so replies cannot interrupt a partially read record,
    // and a parser failure can close the socket even during a blocked write.
    tokio::select! {
        biased;
        result = handler.read_forever(&mut reader) => result,
        result = async {
            while let Some(reply) = replies.recv().await {
                write_fragment(&mut writer, reply?).await?;
            }
            Ok(())
        } => result,
    }
    .inspect_err(|e| debug!("Message handling error: {e}"))
}
