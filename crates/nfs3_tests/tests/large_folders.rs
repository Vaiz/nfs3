use nfs3_tests::{FsConfig, TestContext};
use nfs3_types::nfs3::{cookieverf3, dirlist3, diropargs3, filename3, nfs_fh3, LOOKUP3args, READDIR3args};
use nfs3_types::xdr_codec::{Opaque, PackedSize};

fn get_config(dirname: &str, size: usize) -> FsConfig {
    let mut config = FsConfig::default();

    config.add_file("/a.txt", "hello world\n".as_bytes());
    config.add_file("/b.txt", "Greetings\n".as_bytes());

    config.add_dir(&format!("/{dirname}"));
    for i in 0..size {
        let file_name = get_file_name(i);
        config.add_file(&format!("/{dirname}/{file_name}"), i.to_string().as_bytes());
    }

    config
}

fn get_file_name(i: usize) -> String {
    format!("file_{i}.txt")
}

#[ignore = "wip"]
#[tokio::test]
async fn test_10() {
    const SIZE: usize = 10;
    const DIR: &str = "big_dir";
    let config = get_config(DIR, SIZE);
    let mut client = TestContext::setup_with_config(config).await;

    let root_dir = client.root_dir().clone();
    let dir = lookup(&mut client, root_dir.clone(), DIR).await.unwrap();

    for count in [1024*1024, 128*1024, 16*1024, 4*1024, 1024, 100] {
        list_dir(&mut client, dir.clone(), SIZE, count).await.unwrap();
    }
}

async fn lookup<IO: nfs3_client::io::AsyncRead + nfs3_client::io::AsyncWrite>(
    client: &mut TestContext<IO>,
    parent: nfs_fh3,
    name: &str,
) -> anyhow::Result<nfs_fh3> {
    let lookup = client
        .lookup(LOOKUP3args {
            what: diropargs3 {
                dir: parent,
                name: filename3(Opaque::borrowed(name.as_bytes())),
            },
        })
        .await?
        .unwrap();
    Ok(lookup.object)
}

/*
count
    The maximum size of the READDIR3resok structure, in
    bytes.  The size must include all XDR overhead. The
    server is free to return less than count bytes of
    data.
*/
async fn list_dir<IO: nfs3_client::io::AsyncRead + nfs3_client::io::AsyncWrite>(
    client: &mut TestContext<IO>,
    dir: nfs_fh3,
    total_files_count: usize,
    count: u32,
) -> anyhow::Result<()> {

    let mut cookie = 0;
    let mut cookieverf = cookieverf3::default();
    let mut all_entries = std::collections::HashSet::new();

    loop {
        let args = READDIR3args {
            dir: dir.clone(),
            cookie,
            cookieverf: cookieverf.clone(),
            count,
        };

        let resok = client.readdir(args).await?.unwrap();
        assert!(resok.packed_size() <= count as usize, "packed size is larger than count");

        let dirlist3{entries, eof} = resok.reply;
        let entries = entries.into_inner();
        assert!(eof || entries.len() > 0, "eof is false but no entries");

        cookieverf = resok.cookieverf;
        cookie = entries.last().map_or(0, |e| e.cookie);
        
        for entry in entries {
            let filename = String::from_utf8(entry.name.0.to_vec())?;
            assert!(all_entries.insert(filename), "duplicate entry");
        }

        if eof {
            break;
        }
    }

    // Check if we have all entries
    assert_eq!(all_entries.len(), total_files_count);
    println!("{all_entries:?}");
    for i in 0..total_files_count {
        let file_name = get_file_name(i);
        assert!(all_entries.contains(&file_name), "missing entry {i}");
    }
    
    Ok(())
}