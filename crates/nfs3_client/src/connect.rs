use std::ops::{Deref, DerefMut};

use nfs3_types::mount::{dirpath, mountres3_ok};
use nfs3_types::nfs3::nfs_fh3;
use nfs3_types::xdr_codec::Opaque;

use crate::error::Error;
use crate::io::{AsyncRead, AsyncWrite};
use crate::{mount, portmapper, MountClient, Nfs3Client};

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
    pub fn root_nfs_fh3(&self) -> nfs_fh3 {
        nfs_fh3 {
            data: self.mount_resok.fhandle.0.clone(),
        }
    }

    pub async fn unmount(mut self) -> Result<(), Error> {
        self.mount_client.umnt(self.mount_path).await
    }

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

pub struct Nfs3ConnectionBuilder<C> {
    host: String,
    connector: C,
    portmapper_port: u16,
    mount_port: Option<u16>,
    nfs3_port: Option<u16>,
    mount_path: dirpath<'static>,
}

impl<C, S> Nfs3ConnectionBuilder<C>
where
    C: crate::net::Connector<Connection = S>,
    S: AsyncRead + AsyncWrite + 'static,
{
    pub fn new(connector: C, host: String, mount_path: String) -> Self {
        Self {
            host,
            connector,
            portmapper_port: nfs3_types::portmap::PMAP_PORT,
            mount_port: None,
            nfs3_port: None,
            mount_path: dirpath(Opaque::owned(mount_path.into_bytes())),
        }
    }

    pub fn portmapper_port(mut self, port: u16) -> Self {
        self.portmapper_port = port;
        self
    }
    pub fn mount_port(mut self, port: u16) -> Self {
        self.mount_port = Some(port);
        self
    }
    pub fn nfs3_port(mut self, port: u16) -> Self {
        self.nfs3_port = Some(port);
        self
    }

    pub async fn connect(self) -> Result<Nfs3Connection<S>, Error> {
        let (mount_port, nfs3_port) = self.resolve_ports().await?;

        let io = self.connector.connect(&self.host, mount_port).await?;
        let mut mount_client = mount::MountClient::new(io);
        let borrowed_mount_path = dirpath(Opaque::borrowed(self.mount_path.0.as_ref()));
        let mount_resok = mount_client.mnt(borrowed_mount_path).await?;

        let io = self.connector.connect(&self.host, nfs3_port).await?;
        let nfs3_client = Nfs3Client::new(io);

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
}
