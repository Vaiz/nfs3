use std::env;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jiff::Timestamp;
use nfs3_client::Nfs3ConnectionBuilder;
use nfs3_client::nfs3_types::nfs3::{self, Nfs3Option};
use nfs3_client::nfs3_types::portmap::PMAP_PORT;
use nfs3_client::nfs3_types::rpc::{auth_unix, opaque_auth};
use nfs3_client::nfs3_types::xdr_codec::Opaque;
use nfs3_client::tokio::TokioConnector;

fn format_mtime(mtime: SystemTime) -> Result<String, jiff::Error> {
    let timestamp = Timestamp::try_from(mtime)?;
    // Preserve chrono's AutoSi display using the normalized timestamp, including
    // the existing Duration::new normalization of oversized NFS nanoseconds.
    let format = match timestamp.subsec_nanosecond() {
        0 => "%F %T UTC",
        nanos if nanos % 1_000_000 == 0 => "%F %T%.3f UTC",
        nanos if nanos % 1_000 == 0 => "%F %T%.6f UTC",
        _ => "%F %T%.9f UTC",
    };
    Ok(timestamp.strftime(format).to_string())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().collect::<Vec<_>>();
    if args.len() < 3 {
        eprintln!("Usage: ls <server_ip> <mount_path> [portmapper_port]");
        return Ok(());
    }

    let ip = args[1].clone();
    let mount_path = args[2].clone();
    let portmapper_port = args
        .get(3)
        .and_then(|port| port.parse::<u16>().ok())
        .unwrap_or(PMAP_PORT);

    // This auth values are the same that Windows NFS client uses
    let auth_unix = auth_unix {
        stamp: 0xaaaa_aaaa,
        machinename: Opaque::borrowed(b"unknown"),
        uid: 0xffff_fffe,
        gid: 0xffff_fffe,
        gids: vec![],
    };
    let credential = opaque_auth::auth_unix(&auth_unix);

    let mut connection = Nfs3ConnectionBuilder::new(TokioConnector, ip, mount_path)
        .portmapper_port(portmapper_port)
        .credential(credential)
        .mount()
        .await?;

    let root = connection.root_nfs_fh3();

    println!("Listing root directory using readdir:");
    let mut cookie = nfs3::cookie3::default();
    let mut cookieverf = nfs3::cookieverf3::default();
    loop {
        let readdir = connection
            .readdir(&nfs3::READDIR3args {
                dir: root.clone(),
                cookie,
                cookieverf,
                count: 128 * 1024,
            })
            .await?
            .unwrap();

        let entries = readdir.reply.entries.into_inner();
        for entry in &entries {
            let name = String::from_utf8_lossy(&entry.name.0);
            println!("{name}");
        }

        if readdir.reply.eof {
            break;
        }

        cookie = entries.last().expect("entries list is empty").cookie;
        cookieverf = readdir.cookieverf;
    }

    println!();
    println!("Listing root directory using readdirplus:");
    let mut cookie = nfs3::cookie3::default();
    let mut cookieverf = nfs3::cookieverf3::default();
    loop {
        let readdirplus = connection
            .readdirplus(&nfs3::READDIRPLUS3args {
                dir: root.clone(),
                cookie,
                cookieverf,
                maxcount: 128 * 1024,
                dircount: 128 * 1024,
            })
            .await?
            .unwrap();

        let entries = readdirplus.reply.entries.into_inner();
        for entry in &entries {
            let name = String::from_utf8_lossy(&entry.name.0);
            let attrs = &entry.name_attributes;
            let (is_dir, size, mtime) = if let Nfs3Option::Some(attrs) = attrs {
                let is_dir = matches!(attrs.type_, nfs3::ftype3::NF3DIR);
                let mtime = &attrs.mtime;
                let duration = Duration::new(u64::from(mtime.seconds), mtime.nseconds);
                let systime = UNIX_EPOCH.checked_add(duration).unwrap_or(UNIX_EPOCH);
                (is_dir, attrs.size, systime)
            } else {
                (false, 0, UNIX_EPOCH)
            };
            let mtime = format_mtime(mtime)?;
            if is_dir {
                let dirname = format!("[{name}]");
                println!("{dirname:<60} {:>10} {mtime}", " ");
            } else {
                println!("{name:<60} {size:>10} {mtime}");
            }
        }

        if readdirplus.reply.eof {
            break;
        }

        cookie = entries.last().expect("entries list is empty").cookie;
        cookieverf = readdirplus.cookieverf;
    }

    connection.unmount().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mtime_preserves_auto_si_fractional_precision() {
        for (nanos, expected) in [
            (0, "1970-01-01 00:00:00 UTC"),
            (120_000_000, "1970-01-01 00:00:00.120 UTC"),
            (123_450_000, "1970-01-01 00:00:00.123450 UTC"),
            (123_456_780, "1970-01-01 00:00:00.123456780 UTC"),
            (1, "1970-01-01 00:00:00.000000001 UTC"),
            (1_000, "1970-01-01 00:00:00.000001 UTC"),
            (1_000_000, "1970-01-01 00:00:00.001 UTC"),
        ] {
            assert_eq!(
                format_mtime(UNIX_EPOCH + Duration::new(0, nanos)).expect("NFS timestamp"),
                expected,
            );
        }
    }

    #[test]
    fn mtime_preserves_nfs_range_and_nanosecond_normalization() {
        for (seconds, nanos, expected) in [
            (1_709_164_800, 0, "2024-02-29 00:00:00 UTC"),
            (u32::MAX, 0, "2106-02-07 06:28:15 UTC"),
            (u32::MAX, 999_999_999, "2106-02-07 06:28:15.999999999 UTC"),
            (u32::MAX, 1_000_000_000, "2106-02-07 06:28:16 UTC"),
            (u32::MAX, u32::MAX, "2106-02-07 06:28:19.294967295 UTC"),
        ] {
            assert_eq!(
                format_mtime(UNIX_EPOCH + Duration::new(u64::from(seconds), nanos))
                    .expect("NFS timestamp"),
                expected,
            );
        }
    }
}
