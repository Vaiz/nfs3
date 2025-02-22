use nfs3_types::mount::{dirpath, mountres3_ok};
use nfs3_types::xdr_codec::Opaque;

use crate::error::Error;
use crate::io::{AsyncRead, AsyncWrite};
use crate::{mount, portmapper, Nfs3Client};

/// Connect to an NFSv3 server
///
/// `connect` resolves the port for MOUNT3 and NFSv3 service using the portmapper, then mounts the
/// filesystem at `mount_path`, and returns a client for the NFSv3 service and the mount result.
///
/// NOTE: Currently it doesn't implement unmounting the filesystem when the client is dropped.
pub async fn connect<C, S>(
    connector: C,
    host: &str,
    mount_path: &str,
) -> Result<(Nfs3Client<S>, mountres3_ok<'static>), Error>
where
    C: crate::net::Connector<Connection = S>,
    S: AsyncRead + AsyncWrite + 'static,
{
    let rpc = connector
        .connect(host, nfs3_types::portmap::PMAP_PORT)
        .await?;
    let mut portmapper = portmapper::PortmapperClient::new(rpc);

    let mount_port = portmapper
        .getport(nfs3_types::mount::PROGRAM, nfs3_types::mount::VERSION)
        .await?;
    let nfs_port = portmapper
        .getport(nfs3_types::nfs3::PROGRAM, nfs3_types::nfs3::VERSION)
        .await?;

    let mount_rpc = connector.connect(host, mount_port as u16).await?;
    let mut mount = mount::MountClient::new(mount_rpc);
    let mount_path = Opaque::borrowed(mount_path.as_bytes());
    let mount_res = mount.mnt(dirpath(mount_path)).await?;

    let rpc = connector.connect(host, nfs_port as u16).await?;
    let nfs3_client = Nfs3Client::new(rpc);

    Ok((nfs3_client, mount_res))
}
