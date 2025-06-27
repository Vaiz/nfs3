use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use nfs3_client::Nfs3ConnectionBuilder;
use nfs3_client::io::{AsyncRead, AsyncWrite};
use nfs3_client::tokio::TokioConnector;
use nfs3_client::nfs3_types::nfs3::{self, Nfs3Option, filename3};
use nfs3_client::nfs3_types::xdr_codec::Opaque;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().collect::<Vec<_>>();
    if args.len() < 5 {
        eprintln!(
            "Usage: download_folder <server_ip> <mount_path> <remote_folder> <local_folder> \
             [portmapper_port]"
        );
        return Ok(());
    }

    let ip = args[1].clone();
    let mount_path = args[2].clone();
    let remote_folder = args[3].clone();
    let local_folder = args[4].clone();
    let portmapper_port = args
        .get(5)
        .and_then(|port| port.parse::<u16>().ok())
        .unwrap_or(nfs3_client::nfs3_types::portmap::PMAP_PORT);

    let auth_unix = nfs3_client::nfs3_types::rpc::auth_unix {
        stamp: 0xaaaa_aaaa,
        machinename: Opaque::borrowed(b"unknown"),
        uid: 0xffff_fffe,
        gid: 0xffff_fffe,
        gids: vec![],
    };
    let credential = nfs3_client::nfs3_types::rpc::opaque_auth::auth_unix(&auth_unix);

    let mut connection = Nfs3ConnectionBuilder::new(TokioConnector, ip, mount_path)
        .portmapper_port(portmapper_port)
        .credential(credential)
        .mount()
        .await?;

    let mut remote_folder_fh = connection.root_nfs_fh3();
    let path = Path::new(&remote_folder);
    for component in path.components() {
        if let Component::Normal(name) = component {
            let name = name.to_str().expect("failed to convert name to utf-8");
            let lookup = connection
                .lookup(nfs3::LOOKUP3args {
                    what: nfs3::diropargs3 {
                        dir: remote_folder_fh.clone(),
                        name: filename3(Opaque::borrowed(name.as_bytes())),
                    },
                })
                .await?
                .unwrap();

            remote_folder_fh = lookup.object;
        } else {
            panic!("Invalid remote folder path format: {remote_folder}");
        }
    }

    download_folder(&mut connection, remote_folder_fh, &local_folder).await?;

    connection.unmount().await?;
    Ok(())
}

#[allow(clippy::or_fun_call)]
async fn download_folder(
    connection: &mut nfs3_client::Nfs3Connection<impl AsyncRead + AsyncWrite>,
    folder_fh: nfs3::nfs_fh3,
    local_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(local_path)?;

    let mut queue = vec![(PathBuf::from(local_path), folder_fh)];
    while let Some((local_path, folder_fh)) = queue.pop() {
        let mut cookie = nfs3::cookie3::default();
        let mut cookieverf = nfs3::cookieverf3::default();
        loop {
            let readdirplus = connection
                .readdirplus(nfs3::READDIRPLUS3args {
                    dir: folder_fh.clone(),
                    cookie,
                    cookieverf,
                    maxcount: 128 * 1024,
                    dircount: 128 * 1024,
                })
                .await?
                .unwrap();

            let entries = readdirplus.reply.entries.into_inner();
            cookie = entries
                .last()
                .map_or(nfs3::cookie3::default(), |e| e.cookie);
            cookieverf = readdirplus.cookieverf;

            for entry in entries {
                let name = String::from_utf8_lossy(&entry.name.0).into_owned();
                if name == "." || name == ".." {
                    continue;
                }

                let (fh, attrs) = if let (Nfs3Option::Some(fh), Nfs3Option::Some(attr)) =
                    (entry.name_handle, entry.name_attributes)
                {
                    (fh, attr)
                } else {
                    let lookup = connection
                        .lookup(nfs3::LOOKUP3args {
                            what: nfs3::diropargs3 {
                                dir: folder_fh.clone(),
                                name: entry.name,
                            },
                        })
                        .await?
                        .unwrap();

                    if lookup.obj_attributes.is_none() {
                        eprintln!("Failed to lookup entry: {name}");
                        continue;
                    }
                    (lookup.object, lookup.obj_attributes.unwrap())
                };

                let local_entry_path = Path::new(&local_path).join(name);
                let is_dir = matches!(attrs.type_, nfs3::ftype3::NF3DIR);
                if is_dir {
                    fs::create_dir_all(&local_entry_path)?;
                    queue.push((local_entry_path, fh));
                } else {
                    download_file(connection, fh, &local_entry_path.to_string_lossy()).await?;
                }
            }

            if readdirplus.reply.eof {
                break;
            }
        }
    }

    Ok(())
}

async fn download_file(
    connection: &mut nfs3_client::Nfs3Connection<impl AsyncRead + AsyncWrite>,
    file_fh: nfs3::nfs_fh3,
    local_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Downloading file: {local_path}");
    let mut file = File::create(local_path)?;
    let mut offset = 0;
    loop {
        let read = connection
            .read(nfs3::READ3args {
                file: file_fh.clone(),
                offset,
                count: 128 * 1024,
            })
            .await?
            .unwrap();

        let read_data = read.data.0;
        file.write_all(&read_data)?;
        offset += read_data.len() as u64;

        if read.eof {
            break;
        }
    }

    Ok(())
}
