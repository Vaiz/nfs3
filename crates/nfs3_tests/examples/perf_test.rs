use nfs3_client::io::{AsyncRead, AsyncWrite};
use nfs3_tests::TestContext;
use nfs3_types::nfs3::{
    LOOKUP3args, READDIR3args, READDIRPLUS3args, cookieverf3, dirlist3, dirlistplus3, diropargs3,
    filename3, nfs_fh3,
};
use nfs3_types::xdr_codec::Opaque;
use tracing::info;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    const DIR: &str = "dir_10000";
    const SIZE: usize = 10000;
    const LOG_LEVEL: tracing::Level = tracing::Level::INFO;

    let config = get_config(DIR, SIZE);
    let mut client = TestContext::setup_with_config(config, LOG_LEVEL);

    let start_time = std::time::Instant::now();
    let requests_count = test_dir(&mut client, "dir_10000").await.unwrap();
    let elapsed_time = start_time.elapsed();

    println!("Elapsed time: {:?}", elapsed_time);
    println!("Requests count: {requests_count}");
    println!(
        "Requests per second: {}",
        requests_count as f64 / elapsed_time.as_secs_f64()
    );

    client.shutdown().await.unwrap();
}

fn get_config(dirname: &str, size: usize) -> nfs3_server::memfs::MemFsConfig {
    let mut config = nfs3_server::memfs::MemFsConfig::default();

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
    format!("{i}_this_is_a_really_long_file_name_that_keeps_going_and_going_and_going_and_going_0123456789.txt")
}

async fn test_dir<IO: AsyncRead + AsyncWrite>(
    client: &mut TestContext<IO>,
    dir: &str,
) -> anyhow::Result<u64> {
    let root_dir = client.root_dir().clone();
    let dir = lookup(client, root_dir.clone(), dir).await?;

    // going lower than 256 bytes will cause NFS3ERR_TOOSMALL
    let mut request_count = 1u64; // lookup
    for count in [256 * 1024, 128 * 1024, 16 * 1024, 4 * 1024, 1024, 384] {
        request_count += readdir(client, dir.clone(), count).await?;
        request_count += readdir_plus(client, dir.clone(), count, count).await?;
        request_count += readdir_plus(client, dir.clone(), 1024 * 1024, count).await?;
        request_count += readdir_plus(client, dir.clone(), count, 1024 * 1024).await?;
    }

    Ok(request_count)
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
// bytes. The size must include all XDR overhead. The
// server is free to return less than count bytes of
// data.
async fn readdir<IO: AsyncRead + AsyncWrite>(
    client: &mut TestContext<IO>,
    dir: nfs_fh3,
    count: u32,
) -> anyhow::Result<u64> {
    info!("readdir count: {count}");

    let mut request_count = 0;
    let mut cookie = 0;
    let mut cookieverf = cookieverf3::default();

    loop {
        let args = READDIR3args {
            dir: dir.clone(),
            cookie,
            cookieverf,
            count,
        };

        let resok = client.readdir(args).await?.unwrap();
        request_count += 1;

        let dirlist3 { entries, eof } = resok.reply;
        let entries = entries.into_inner();
        assert!(eof || !entries.is_empty(), "eof is false but no entries");

        cookieverf = resok.cookieverf;
        cookie = entries.last().map_or(0, |e| e.cookie);

        if eof {
            break;
        }
    }

    Ok(request_count)
}

// dircount
// The maximum number of bytes of directory information
// returned. This number should not include the size of
// the attributes and file handle portions of the result.
//
// maxcount
// The maximum size of the READDIRPLUS3resok structure, in
// bytes. The size must include all XDR overhead. The
// server is free to return fewer than maxcount bytes of
// data.
async fn readdir_plus<IO: AsyncRead + AsyncWrite>(
    client: &mut TestContext<IO>,
    dir: nfs_fh3,
    dircount: u32,
    maxcount: u32,
) -> anyhow::Result<u64> {
    info!("readdir_plus dircount: {dircount} maxcount: {maxcount}");

    let mut request_count = 0;
    let mut cookie = 0;
    let mut cookieverf = cookieverf3::default();

    loop {
        let args = READDIRPLUS3args {
            dir: dir.clone(),
            cookie,
            cookieverf,
            dircount,
            maxcount,
        };

        let resok = client.readdirplus(args).await?.unwrap();
        request_count += 1;

        let dirlistplus3 { entries, eof } = resok.reply;
        let entries = entries.into_inner();
        assert!(eof || !entries.is_empty(), "eof is false but no entries");

        cookieverf = resok.cookieverf;
        cookie = entries.last().map_or(0, |e| e.cookie);

        if eof {
            break;
        }
    }

    Ok(request_count)
}
