use std::ops::{Deref, DerefMut};
use std::sync::LazyLock;
use std::sync::atomic::AtomicU16;

use nfs3_types::mount::{dirpath, mountres3_ok};
use nfs3_types::nfs3::nfs_fh3;
use nfs3_types::rpc::opaque_auth;
use nfs3_types::xdr_codec::Opaque;

use crate::error::Error;
use crate::io::{AsyncRead, AsyncWrite};
use crate::{MountClient, Nfs3Client, mount, portmapper};

/// Contains the connection to the NFS server.
#[derive(Debug)]
pub struct Nfs3Connection<IO> {
    pub host: String,
    pub mount_port: u16,
    pub mount_path: dirpath<'static>,
    pub mount_client: MountClient<IO>,
    pub mount_resok: mountres3_ok<'static>,
    pub nfs3_port: u16,
    pub nfs3_client: Nfs3Client<IO>,
}

impl<IO> Nfs3Connection<IO>
where
    IO: AsyncRead + AsyncWrite,
{
    /// Returns the root file handle of the mounted filesystem.
    pub fn root_nfs_fh3(&self) -> nfs_fh3 {
        nfs_fh3 {
            data: self.mount_resok.fhandle.0.clone(),
        }
    }

    /// Unmounts the filesystem and drops the client.
    pub async fn unmount(mut self) -> Result<(), Error> {
        self.mount_client.umnt(self.mount_path).await
    }

    /// Returns the underlying NFSv3 client and drops everything else.
    /// This is useful for when you want to use the NFSv3 client for a long period of time
    /// and don't want to keep the connection to Mount service open.
    pub fn into_nfs3_client(self) -> Nfs3Client<IO> {
        self.nfs3_client
    }
}

impl<IO> Deref for Nfs3Connection<IO> {
    type Target = Nfs3Client<IO>;

    fn deref(&self) -> &Self::Target {
        &self.nfs3_client
    }
}

impl<IO> DerefMut for Nfs3Connection<IO> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.nfs3_client
    }
}

/// Builder for establishing an NFSv3 connection.
///
/// By default, the builder attempts to use a privileged port (300-1023) for outgoing connections.
/// This behavior can be modified by calling `connect_from_privileged_port(false)`.
pub struct Nfs3ConnectionBuilder<C> {
    host: String,
    connector: C,
    connect_from_privileged_port: bool,
    portmapper_port: u16,
    mount_port: Option<u16>,
    nfs3_port: Option<u16>,
    mount_path: dirpath<'static>,
    credential: opaque_auth<'static>,
    verifier: opaque_auth<'static>,
}

impl<C, S> Nfs3ConnectionBuilder<C>
where
    C: crate::net::Connector<Connection = S>,
    S: AsyncRead + AsyncWrite + 'static,
{
    /// Creates a new NFSv3 connection builder.
    /// The `mount_path` is the path to mount on the server.
    pub fn new(connector: C, host: String, mount_path: String) -> Self {
        Self {
            host,
            connector,
            connect_from_privileged_port: true,
            portmapper_port: nfs3_types::portmap::PMAP_PORT,
            mount_port: None,
            nfs3_port: None,
            mount_path: dirpath(Opaque::owned(mount_path.into_bytes())),
            credential: opaque_auth::default(),
            verifier: opaque_auth::default(),
        }
    }

    /// Sets whether to connect from a privileged port (0-1023).
    /// The default is `true`.
    pub fn connect_from_privileged_port(mut self, connect: bool) -> Self {
        self.connect_from_privileged_port = connect;
        self
    }

    /// Sets the portmapper port. The default port is 111.
    pub fn portmapper_port(mut self, port: u16) -> Self {
        self.portmapper_port = port;
        self
    }
    /// Sets the mount port. The default port is resolved from the portmapper.
    pub fn mount_port(mut self, port: u16) -> Self {
        self.mount_port = Some(port);
        self
    }
    /// Sets the NFSv3 port. The default port is resolved from the portmapper.
    pub fn nfs3_port(mut self, port: u16) -> Self {
        self.nfs3_port = Some(port);
        self
    }

    /// Sets the credential for the RPC calls. The default is `opaque_auth::default()`.
    pub fn credential(mut self, credential: opaque_auth<'static>) -> Self {
        self.credential = credential;
        self
    }

    /// Sets the verifier for the RPC calls. The default is `opaque_auth::default()`.
    pub fn verifier(mut self, verifier: opaque_auth<'static>) -> Self {
        self.verifier = verifier;
        self
    }

    /// Mounts the filesystem and returns the connection.
    pub async fn mount(self) -> Result<Nfs3Connection<S>, Error> {
        let (mount_port, nfs3_port) = self.resolve_ports().await?;

        let io = self.connect(mount_port).await?;
        let mut mount_client =
            mount::MountClient::new_with_auth(io, self.credential.clone(), self.verifier.clone());
        let borrowed_mount_path = dirpath(Opaque::borrowed(self.mount_path.0.as_ref()));
        let mount_resok = mount_client.mnt(borrowed_mount_path).await?;

        let io = self.connect(nfs3_port).await?;
        let nfs3_client = Nfs3Client::new_with_auth(io, self.credential, self.verifier);

        Ok(Nfs3Connection {
            host: self.host,
            mount_port,
            mount_path: self.mount_path,
            mount_client,
            mount_resok,
            nfs3_port,
            nfs3_client,
        })
    }

    async fn resolve_ports(&self) -> Result<(u16, u16), Error> {
        if self.mount_port.is_some() && self.nfs3_port.is_some() {
            return Ok((self.mount_port.unwrap(), self.nfs3_port.unwrap()));
        }

        let io = self
            .connector
            .connect(&self.host, self.portmapper_port)
            .await?;

        let mut portmapper = portmapper::PortmapperClient::new(io);
        let mount_port = if let Some(port) = self.mount_port {
            port
        } else {
            portmapper
                .getport(nfs3_types::mount::PROGRAM, nfs3_types::mount::VERSION)
                .await?
        };

        let nfs3_port = if let Some(port) = self.nfs3_port {
            port
        } else {
            portmapper
                .getport(nfs3_types::nfs3::PROGRAM, nfs3_types::nfs3::VERSION)
                .await?
        };

        Ok((mount_port, nfs3_port))
    }

    async fn connect(&self, port: u16) -> std::io::Result<S> {
        if self.connect_from_privileged_port {
            connect_from_privileged_port(&self.connector, &self.host, port).await
        } else {
            self.connector.connect(&self.host, port).await
        }
    }
}

async fn connect_from_privileged_port<C, S>(
    connector: &C,
    host: &str,
    port: u16,
) -> std::io::Result<S>
where
    C: crate::net::Connector<Connection = S>,
    S: AsyncRead + AsyncWrite,
{
    use std::io::{Error as IoError, ErrorKind as IoErrorKind};
    const MIN_PORT: u16 = 300;
    const MAX_PORT: u16 = 1023;
    /// a hack to reduce the chance of port collision
    static PORT_INDEX: LazyLock<AtomicU16> =
        LazyLock::new(|| AtomicU16::new(rand::random::<u16>()));

    for _ in MIN_PORT..=MAX_PORT {
        let index = PORT_INDEX.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let local_port = MIN_PORT + (index % (MAX_PORT - MIN_PORT));

        let result = connector.connect_with_port(host, port, local_port).await;

        match &result {
            Err(e) if e.kind() == IoErrorKind::AddrInUse => {
                // Ignore this error and try the next port
                continue;
            }
            Ok(_) | Err(_) => {
                return result;
            }
        }
    }

    Err(IoError::other(format!(
        "Failed to connect to mount service from privileged port range ({MIN_PORT}-{MAX_PORT})"
    )))
}
