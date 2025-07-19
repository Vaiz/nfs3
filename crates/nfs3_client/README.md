# `nfs3_client`

`nfs3_client` is a Rust crate that provides an asynchronous client implementation for interacting with `NFSv3` servers. It includes functionality for connecting to NFS servers, performing various NFS operations, and handling the underlying RPC communication.

## Features

This crate supports multiple async runtimes via feature flags:

- `tokio` - Support for Tokio runtime
- `smol` - Support for Smol runtime

## Examples

### Using with Tokio

```rust,no_run
use nfs3_client::tokio::TokioConnector;
use nfs3_client::Nfs3ConnectionBuilder;
use nfs3_types::nfs3;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut connection = Nfs3ConnectionBuilder::new(TokioConnector, "127.0.0.1", "/")
        .mount()
        .await?;

    let root = connection.root_nfs_fh3();

    println!("Calling readdir");
    let readdir = connection
        .readdir(&nfs3::READDIR3args {
            dir: root,
            cookie: 0,
            cookieverf: nfs3::cookieverf3::default(),
            count: 128 * 1024 * 1024,
        })
        .await?;

    println!("{readdir:?}");

    connection.unmount().await?;

    Ok(())
}
```

### Using with Smol

```rust,no_run
use nfs3_client::smol::SmolConnector;
use nfs3_client::Nfs3ConnectionBuilder;
use nfs3_client::nfs3_types::nfs3;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    smol::block_on(async {
        let mut connection = Nfs3ConnectionBuilder::new(SmolConnector, "127.0.0.1", "/")
            .mount()
            .await?;

        let root = connection.root_nfs_fh3();

        println!("Calling readdir");
        let readdir = connection
            .readdir(&nfs3::READDIR3args {
                dir: root.clone(),
                cookie: 0,
                cookieverf: nfs3::cookieverf3::default(),
                count: 128 * 1024 * 1024,
            })
            .await?;

        println!("{readdir:?}");

        connection.unmount().await?;

        Ok(())
    })
}
```
