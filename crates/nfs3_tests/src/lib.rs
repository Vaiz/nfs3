mod io;
mod server;

pub use io::MockChannel;
pub use server::Server;

pub fn create_client_and_server() -> (Server, nfs3_client::Nfs3Client<MockChannel>) {
    let (server, client) = MockChannel::pair();

    (Server::new(server), nfs3_client::Nfs3Client::new(client))
}

#[cfg(test)]
mod tests {
    use nfs3_types::nfs3::{diropargs3, nfs_fh3, LOOKUP3args};

    use super::*;

    #[tokio::test]
    async fn base_test() -> Result<(), anyhow::Error> {
        let (server, mut client) = create_client_and_server();
        let server_handle = tokio::spawn(async move { server.run().await });

        client.null().await?;
        let lookup = client
            .lookup(LOOKUP3args {
                what: diropargs3 {
                    dir: nfs_fh3::default(),
                    name: vec![].into(),
                },
            })
            .await?;

        println!("{lookup:?}");

        drop(client);
        let _ = server_handle.await;

        Ok(())
    }
}
