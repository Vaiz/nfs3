use nfs3_types::mount::{
    dirpath, export_node, exports, fhandle3, mountres3, mountres3_ok, mountstat3,
};
use nfs3_types::rpc::auth_flavor;
use nfs3_types::xdr_codec::{List, Opaque, Void};
use tracing::debug;

use crate::context::RPCContext;
use crate::vfs::NfsFileSystem;

pub async fn mountproc3_mnt<T>(
    context: RPCContext<T>,
    xid: u32,
    path: dirpath<'_>,
) -> mountres3<'static>
where
    T: NfsFileSystem,
{
    let path = std::str::from_utf8(&path.0);
    let utf8path = match path {
        Ok(path) => path,
        Err(e) => {
            tracing::error!("{xid} --> invalid mount path: {e}");
            return mountres3::Err(mountstat3::MNT3ERR_INVAL);
        }
    };

    debug!("mountproc3_mnt({xid},{utf8path})");
    let path = if let Some(path) = utf8path.strip_prefix(context.export_name.as_str()) {
        path.trim_start_matches('/').trim_end_matches('/').trim()
    } else {
        // invalid export
        debug!("{xid} --> no matching export");
        return mountres3::Err(mountstat3::MNT3ERR_NOENT);
    };

    match context.vfs.lookup_by_path(path).await {
        Ok(fileid) => {
            let root = context.file_handle_converter.fh_to_nfs(&fileid);
            let response = mountres3_ok {
                fhandle: fhandle3(root.data),
                auth_flavors: vec![auth_flavor::AUTH_NULL as u32, auth_flavor::AUTH_UNIX as u32],
            };
            debug!("{xid} --> {response:?}");
            mountres3::Ok(response)
        }
        Err(e) => {
            debug!("{xid} --> MNT3ERR_NOENT({e:?})");
            mountres3::Err(mountstat3::MNT3ERR_NOENT)
        }
    }
}

/// exports `MOUNTPROC3_EXPORT(void)` = 5;
///
/// typedef struct groupnode *groups;
///
/// struct groupnode {
/// name     `gr_name`;
/// groups   `gr_next`;
/// };
///
/// typedef struct exportnode *exports;
///
/// struct exportnode {
/// dirpath  `ex_dir`;
// groups   ex_groups;
// exports  ex_next;
/// };
///
/// DESCRIPTION
///
/// Procedure EXPORT returns a list of all the exported file
/// systems and which clients are allowed to mount each one.
/// The names in the group list are implementation-specific
/// and cannot be directly interpreted by clients. These names
/// can represent hosts or groups of hosts.
///
/// IMPLEMENTATION
///
/// This procedure generally returns the contents of a list of
/// shared or exported file systems. These are the file
/// systems which are made available to NFS version 3 protocol
/// clients.
#[expect(
    clippy::unused_async,
    reason = "Uniform asynchronous metadata dispatch interface"
)]
pub async fn mountproc3_export<T>(
    context: RPCContext<T>,
    _: u32,
    _: Void,
) -> exports<'static, 'static>
where
    T: crate::vfs::NfsFileSystem,
{
    let export_name = context.export_name.as_bytes().to_vec();
    List(vec![export_node {
        ex_dir: dirpath(Opaque::owned(export_name)),
        ex_groups: List::default(),
    }])
}

#[expect(
    clippy::unused_async,
    reason = "Uniform asynchronous metadata dispatch interface"
)]
pub async fn mountproc3_umnt<T>(_: RPCContext<T>, xid: u32, path: dirpath<'_>) -> Void
where
    T: crate::vfs::NfsFileSystem,
{
    let utf8path = match std::str::from_utf8(&path.0) {
        Ok(path) => path,
        Err(e) => {
            tracing::warn!("{xid} --> invalid mount path: {e}");
            return Void;
        }
    };

    debug!("mountproc3_umnt({xid},{utf8path})");
    Void
}

#[expect(
    clippy::unused_async,
    reason = "Uniform asynchronous metadata dispatch interface"
)]
pub async fn mountproc3_umnt_all<T>(_: RPCContext<T>, xid: u32, _: Void) -> Void
where
    T: crate::vfs::NfsFileSystem,
{
    debug!("mountproc3_umnt_all({xid})");
    Void
}
