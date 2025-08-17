use std::sync::Arc;

use nfs3_server::nfs3_types::nfs3::{fileid3, filename3, ftype3, nfsstat3};
use nfs3_server::vfs::{
    DirEntry, DirEntryPlus, FileHandleU64, NextResult, ReadDirIterator, ReadDirPlusIterator,
};
use tracing::debug;

use super::fs_map::{FSEntry, FSMap};
use super::string_ext::FromOsString;

pub(super) struct MirrorFsIterator {
    fsmap: Arc<tokio::sync::RwLock<FSMap>>,
    entries: Vec<fileid3>,
    index: usize,
}

impl MirrorFsIterator {
    #[allow(clippy::significant_drop_tightening)] // doesn't really matter in this case
    pub(super) async fn new(
        fsmap: Arc<tokio::sync::RwLock<FSMap>>,
        dirid: fileid3,
        start_after: fileid3,
    ) -> Result<Self, nfsstat3> {
        let fsmap_clone = Arc::clone(&fsmap);
        let mut fsmap = fsmap.write().await;
        fsmap.refresh_entry(dirid).await?;
        fsmap.refresh_dir_list(dirid).await?;

        let entry = fsmap.find_entry(dirid)?;
        if !matches!(entry.fsmeta.type_, ftype3::NF3DIR) {
            return Err(nfsstat3::NFS3ERR_NOTDIR);
        }
        debug!("readdir({:?}, {:?})", entry, start_after);
        // we must have children here
        let children = entry.children.as_ref().ok_or(nfsstat3::NFS3ERR_IO)?;

        let pos = match children.binary_search(&start_after) {
            Ok(pos) => pos + 1,
            Err(pos) => {
                // just ignore missing entry
                pos
            }
        };

        let remain_children = children.iter().skip(pos).copied().collect::<Vec<_>>();
        debug!("children len: {:?}", children.len());
        debug!("remaining_len : {:?}", remain_children.len());

        Ok(Self {
            fsmap: fsmap_clone,
            entries: remain_children,
            index: 0,
        })
    }

    async fn visit_next_entry<R>(
        &mut self,
        f: fn(fileid3, &FSEntry, filename3<'static>) -> R,
    ) -> NextResult<R> {
        loop {
            if self.index >= self.entries.len() {
                return NextResult::Eof;
            }

            let fileid = self.entries[self.index];
            self.index += 1;

            let fsmap = self.fsmap.read().await;
            let fs_entry = match fsmap.find_entry(fileid) {
                Ok(entry) => entry,
                Err(nfsstat3::NFS3ERR_NOENT) => {
                    // skip missing entries
                    debug!("missing entry {fileid}");
                    continue;
                }
                Err(e) => {
                    return NextResult::Err(e);
                }
            };

            let name = fsmap.sym_to_fname(&fs_entry.path);
            debug!("\t --- {fileid} {name:?}");
            let name = filename3::from_os_string(name);
            return NextResult::Ok(f(fileid, fs_entry, name));
        }
    }
}

impl ReadDirPlusIterator<FileHandleU64> for MirrorFsIterator {
    async fn next(&mut self) -> NextResult<DirEntryPlus<FileHandleU64>> {
        self.visit_next_entry(|fileid, fs_entry, name| DirEntryPlus {
            fileid,
            name,
            cookie: fileid,
            name_attributes: Some(fs_entry.fsmeta.clone()),
            name_handle: Some(FileHandleU64::new(fileid)),
        })
        .await
    }
}

impl ReadDirIterator for MirrorFsIterator {
    async fn next(&mut self) -> NextResult<DirEntry> {
        self.visit_next_entry(|fileid, _fs_entry, name| DirEntry {
            fileid,
            name,
            cookie: fileid,
        })
        .await
    }
}
