use nfs3_client::io::{AsyncRead, AsyncWrite};
use nfs3_tests::{FsConfig, TestContext};
use nfs3_types::nfs3::{
    cookieverf3, dirlist3, dirlistplus3, diropargs3, filename3, nfs_fh3, LOOKUP3args, READDIR3args, READDIRPLUS3args
};
use nfs3_types::xdr_codec::{Opaque, PackedSize};
use tracing::info;


#[ignore = "wip"]
#[tokio::test]
async fn test_10() {
    test_dir(10, "dir_10").await.unwrap();
}

#[ignore = "wip"]
#[tokio::test]
async fn test_100() {
    test_dir(100, "dir_100").await.unwrap();
}


#[ignore = "wip"]
#[tokio::test]
async fn test_1000() {
    test_dir(1000, "dir_1000").await.unwrap();
}


#[ignore = "wip"]
#[tokio::test]
async fn test_10000() {
    test_dir(10000, "dir_10000").await.unwrap();
}

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
    format!("this_is_a_really_long_file_name_number_{i}_that_keeps_going_and_going_and_going_0123456789.txt")
}

async fn test_dir(size: usize, dir: &str) -> anyhow::Result<()> {
    let config = get_config(dir, size);
    let mut client = TestContext::setup_with_config(config).await;

    let root_dir = client.root_dir().clone();
    let dir = lookup(&mut client, root_dir.clone(), dir).await?;

    // going lower than 256 bytes will cause NFS3ERR_TOOSMALL
    for count in [256 * 1024, 128 * 1024, 16 * 1024, 4 * 1024, 1024, 256] {
        readdir(&mut client, dir.clone(), count, size).await?;
        readdir_plus(&mut client, dir.clone(), count, count, size).await?;
        readdir_plus(&mut client, dir.clone(), 1024*1024, count, size).await?;
        readdir_plus(&mut client, dir.clone(), count, 1024*1024, size).await?;
    }

    Ok(())
}

async fn lookup<IO: AsyncRead + AsyncWrite>(
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

// count
// The maximum size of the READDIR3resok structure, in
// bytes.  The size must include all XDR overhead. The
// server is free to return less than count bytes of
// data.
async fn readdir<IO: AsyncRead + AsyncWrite>(
    client: &mut TestContext<IO>,
    dir: nfs_fh3,
    count: u32,
    total_files_count: usize,
) -> anyhow::Result<()> {
    info!("readdir count: {count}");

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
        assert!(
            resok.packed_size() <= count as usize,
            "packed size is larger than count"
        );

        let dirlist3 { entries, eof } = resok.reply;
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
    for i in 0..total_files_count {
        let file_name = get_file_name(i);
        assert!(all_entries.contains(&file_name), "missing entry {i}");
    }

    Ok(())
}

async fn readdir_plus<IO: AsyncRead + AsyncWrite>(
    client: &mut TestContext<IO>,
    dir: nfs_fh3,
    dircount: u32,
    maxcount: u32,
    total_files_count: usize,
) -> anyhow::Result<()> {
    info!("readdir_plus dircount: {dircount} maxcount: {maxcount}");

    let mut cookie = 0;
    let mut cookieverf = cookieverf3::default();
    let mut all_entries = std::collections::HashSet::new();

    loop {
        let args = READDIRPLUS3args {
            dir: dir.clone(),
            cookie,
            cookieverf: cookieverf.clone(),
            dircount,
            maxcount,
        };

        let resok = client.readdirplus(args).await?.unwrap();
        assert!(
            resok.packed_size() <= maxcount as usize,
            "packed size is larger than count"
        );

        let dirlistplus3 { entries, eof } = resok.reply;
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
    for i in 0..total_files_count {
        let file_name = get_file_name(i);
        assert!(all_entries.contains(&file_name), "missing entry {i}");
    }

    Ok(())
}