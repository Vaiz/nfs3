use nfs3_types::nfs3::{cookieverf3, entry3, entryplus3, post_op_attr, post_op_fh3};
use nfs3_types::xdr_codec::List;

/// Private directory-page storage. Callers supply the checked size of an entry
/// excluding its list discriminator; overflow leaves the accepted page intact.
pub struct DirectoryList<T> {
    list: List<T>,
    current_size: usize,
    max_size: usize,
    max_entries: usize,
}

impl<T> DirectoryList<T> {
    pub(crate) const fn with_entry_limit(max_size: usize, max_entries: usize) -> Self {
        Self {
            list: List(Vec::new()),
            current_size: 4,
            max_size,
            max_entries,
        }
    }

    #[cfg(test)]
    pub(crate) const fn new(max_size: usize) -> Self {
        // Even a zero-sized item requires a four-byte true discriminator.
        Self::with_entry_limit(max_size, max_size.saturating_sub(4) / 4)
    }

    pub(crate) fn try_push_sized(&mut self, item: T, size: Option<usize>) -> Result<(), T> {
        let Some(next_size) = size
            .and_then(|size| size.checked_add(4))
            .and_then(|size| self.current_size.checked_add(size))
        else {
            return Err(item);
        };
        let entries = &mut self.list.0;
        if next_size > self.max_size || entries.len() >= self.max_entries {
            return Err(item);
        }
        if entries.len() == entries.capacity() {
            let target = entries
                .capacity()
                .checked_mul(2)
                .unwrap_or(self.max_entries)
                .max(1)
                .min(self.max_entries);
            let mut grown = Vec::new();
            if grown.try_reserve_exact(target).is_err() || grown.capacity() > target {
                return Err(item);
            }
            // The caller's reservation includes old/new overlap and all
            // geometric targets. Do not hide retained spare capacity.
            grown.append(entries);
            *entries = grown;
        }
        entries.push(item);
        self.current_size = next_size;
        Ok(())
    }

    pub(crate) fn into_inner(self) -> List<T> {
        self.list
    }
}

// Scoped XDR wire arithmetic, never encoding into a scratch allocation.
// fattr3 has 21 four-byte words (RFC 1813 §2.5).
pub const fn attr_size(attr: &post_op_attr) -> usize {
    match attr {
        post_op_attr::None => 4,
        post_op_attr::Some(_) => 88,
    }
}

fn opaque_size(len: usize) -> Option<usize> {
    u32::try_from(len).ok()?;
    len.checked_add(3)?
        .checked_div(4)?
        .checked_mul(4)?
        .checked_add(4)
}

pub fn entry_size(entry: &entry3<'_>) -> Option<usize> {
    opaque_size(entry.name.len())?.checked_add(16)
}

fn entryplus_dir_size(entry: &entryplus3<'_>) -> Option<usize> {
    // Include this entry's true discriminator in dircount, as before.
    opaque_size(entry.name.len())?.checked_add(20)
}

fn entryplus_size(entry: &entryplus3<'_>) -> Option<usize> {
    let handle = match &entry.name_handle {
        post_op_fh3::None => 4,
        post_op_fh3::Some(handle) => opaque_size(handle.data.len())?.checked_add(4)?,
    };
    opaque_size(entry.name.len())?
        .checked_add(16)?
        .checked_add(attr_size(&entry.name_attributes))?
        .checked_add(handle)
}

pub const fn readdir_empty_size(attr: &post_op_attr) -> Option<usize> {
    // XID (4), cookie verifier (8), final false (4), EOF (4).
    attr_size(attr).checked_add(20)
}

pub trait CookieVerfExt {
    const NONE_COOKIE_VERF: cookieverf3 = cookieverf3(0u64.to_be_bytes());
    const SOME_COOKIE_VERF: cookieverf3 = cookieverf3(0xFFCC_FFCC_FFCC_FFCCu64.to_be_bytes());

    fn from_attr(dir_attr: &post_op_attr) -> Self;
    fn is_none(&self) -> bool;
    fn is_some(&self) -> bool;
}

impl CookieVerfExt for cookieverf3 {
    fn from_attr(dir_attr: &post_op_attr) -> Self {
        if let post_op_attr::Some(attr) = dir_attr {
            let cvf_version =
                (u64::from(attr.mtime.seconds) << 32) | u64::from(attr.mtime.nseconds);
            Self(cvf_version.to_be_bytes())
        } else {
            Self::SOME_COOKIE_VERF
        }
    }

    fn is_none(&self) -> bool {
        self == &Self::NONE_COOKIE_VERF
    }

    fn is_some(&self) -> bool {
        !self.is_none()
    }
}
pub struct BoundedEntryPlusList {
    entries: DirectoryList<entryplus3<'static>>,
    dircount: usize,
    accumulated_dircount: usize,
}

impl BoundedEntryPlusList {
    pub fn with_entry_limit(dircount: usize, maxcount: usize, max_entries: usize) -> Self {
        Self {
            entries: DirectoryList::with_entry_limit(maxcount, max_entries.min(maxcount / 24)),
            dircount,
            accumulated_dircount: 0,
        }
    }

    #[cfg(test)]
    pub const fn new(dircount: usize, maxcount: usize) -> Self {
        Self {
            entries: DirectoryList::new(maxcount),
            dircount,
            accumulated_dircount: 0,
        }
    }

    #[expect(
        clippy::result_large_err,
        reason = "Returns the rejected entry without boxing it"
    )]
    pub fn try_push(&mut self, entry: entryplus3<'static>) -> Result<(), entryplus3<'static>> {
        // dircount - the maximum number of bytes of directory information returned. This number
        // should not include the size of the attributes and file handle portions of the result.
        let Some(added_dircount) = entryplus_dir_size(&entry) else {
            return Err(entry);
        };

        let Some(next_dircount) = self.accumulated_dircount.checked_add(added_dircount) else {
            return Err(entry);
        };
        if next_dircount > self.dircount {
            return Err(entry);
        }

        let size = entryplus_size(&entry);
        let result = self.entries.try_push_sized(entry, size);
        if result.is_ok() {
            self.accumulated_dircount = next_dircount;
        }
        result
    }

    pub fn into_inner(self) -> List<entryplus3<'static>> {
        self.entries.into_inner()
    }
}

#[cfg(test)]
mod tests {
    use nfs3_types::nfs3::{
        READDIR3resok, READDIRPLUS3resok, dirlist3, dirlistplus3, fattr3, filename3, ftype3,
        nfs_fh3, nfstime3, post_op_fh3, specdata3,
    };
    use nfs3_types::xdr_codec::{Encode, EncodeLimits, Encoder, Opaque};

    use super::*;

    fn encoded(value: &impl Encode) -> Vec<u8> {
        let mut encoder = Encoder::new(EncodeLimits {
            max_wire_bytes: 4096,
            max_metadata_bytes: 4096,
            max_chunks: 4,
            max_retained_capacity: 0,
            max_depth: 16,
            max_reservation_bytes: 32768,
        });
        encoder.encode(value).expect("fixture encodes");
        encoder
            .finish()
            .expect("exact plan")
            .chunks()
            .flatten()
            .copied()
            .collect()
    }

    fn attr() -> post_op_attr {
        post_op_attr::Some(fattr3 {
            type_: ftype3::NF3REG,
            mode: 0,
            nlink: 0,
            uid: 0,
            gid: 0,
            size: 0,
            used: 0,
            rdev: specdata3::default(),
            fsid: 0,
            fileid: 0,
            atime: nfstime3::default(),
            mtime: nfstime3::default(),
            ctime: nfstime3::default(),
        })
    }

    #[test]
    fn bounded_list_basic() {
        let mut list = DirectoryList::<u32>::new(28);
        for value in [0x1234, 0x5678, 0x9abc] {
            list.try_push_sized(value, Some(4)).unwrap();
        }
        assert!(list.try_push_sized(0xdef0, Some(4)).is_err());
        assert_eq!(list.into_inner().0, vec![0x1234, 0x5678, 0x9abc]);
    }

    #[test]
    fn bounded_list_exact_boundary() {
        for (wire, expected) in [(27, 2), (28, 3), (29, 3)] {
            let mut list = DirectoryList::<u32>::new(wire);
            for (index, value) in [0x1234, 0x5678, 0x9abc, 0xdef0].into_iter().enumerate() {
                assert_eq!(
                    list.try_push_sized(value, Some(4)).is_ok(),
                    index < expected
                );
            }
            assert_eq!(list.into_inner().0, [0x1234, 0x5678, 0x9abc][..expected]);
        }
    }

    #[test]
    fn bounded_list_edge_cases() {
        let mut list = DirectoryList::<u32>::new(4);
        assert!(list.try_push_sized(0x1234, Some(4)).is_err());
        let mut list = DirectoryList::<u32>::new(12);
        list.try_push_sized(0x1234, Some(4)).unwrap();
        assert!(list.try_push_sized(0x5678, Some(4)).is_err());
        // Failed/overflowed sizes cannot change the accepted page.
        assert!(list.try_push_sized(0x5678, None).is_err());
        assert!(list.try_push_sized(0x5678, Some(usize::MAX)).is_err());
        assert_eq!(list.into_inner().0, vec![0x1234]);
    }

    #[test]
    fn large_collections_directory_page() {
        let mut list = DirectoryList::<u32>::new(804);
        for value in 0..100 {
            assert!(
                list.try_push_sized(value, Some(4)).is_ok(),
                "Failed to push item {value}"
            );
        }
        assert!(list.try_push_sized(100, Some(4)).is_err());
        let list = list.into_inner();
        assert_eq!(list.0.len(), 100);
        assert_eq!(list.0, (0..100).collect::<Vec<u32>>());
    }

    #[test]
    fn finite_entry_growth_and_wire_terminal_space() {
        for (wire, max_entries, expected) in [(112, 9, 9), (40, 9, 3), (112, 0, 0)] {
            for count in 0..12 {
                let mut list = DirectoryList::with_entry_limit(wire, max_entries);
                for value in 0..count {
                    assert_eq!(
                        list.try_push_sized(value as u64, Some(8)).is_ok(),
                        value < expected
                    );
                }
                let list = list.into_inner();
                assert_eq!(list.0.len(), count.min(expected));
                assert!(list.0.capacity() <= max_entries);
                assert!(encoded(&list).len() <= wire);
                assert_eq!(list.0, (0..count.min(expected) as u64).collect::<Vec<_>>());
            }
        }
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "Keep independent bytes beside each size assertion"
    )]
    fn directory_wire_goldens_and_checked_lengths() {
        let entry = entry3 {
            fileid: 1,
            name: b"abc".as_slice().into(),
            cookie: 2,
        };
        // Independent RFC 1813 field order, big-endian scalars and zero padding.
        let golden = [
            0, 0, 0, 0, 0, 0, 0, 1, // fileid
            0, 0, 0, 3, b'a', b'b', b'c', 0, 0, 0, 0, 0, 0, 0, 0, 2, // cookie
        ];
        assert_eq!(encoded(&entry), golden);
        assert_eq!(entry_size(&entry), Some(golden.len()));
        for attributes in [post_op_attr::None, attr()] {
            let header = readdir_empty_size(&attributes).expect("fixed size");
            let empty = READDIR3resok {
                dir_attributes: attributes.clone(),
                cookieverf: cookieverf3::default(),
                reply: dirlist3 {
                    entries: List(vec![]),
                    eof: true,
                },
            };
            let empty_bytes = encoded(&empty);
            assert_eq!(header, 4 + empty_bytes.len()); // historical XID allowance
            assert_eq!(
                &empty_bytes[empty_bytes.len() - 8..],
                &[0, 0, 0, 0, 0, 0, 0, 1]
            );
            let page = READDIR3resok {
                reply: dirlist3 {
                    entries: List(vec![entry.clone()]),
                    eof: false,
                },
                ..empty
            };
            let bytes = encoded(&page);
            assert_eq!(4 + bytes.len(), header + 4 + golden.len());
            assert_eq!(
                &bytes[bytes.len() - golden.len() - 8..bytes.len() - 8],
                &golden
            );
        }
        for name_len in [0, 1, 3, 4, 5] {
            for attributes in [post_op_attr::None, attr()] {
                for handle_len in [None, Some(0), Some(1), Some(4), Some(64)] {
                    let entry = entryplus3 {
                        fileid: 1,
                        name: vec![b'x'; name_len].into(),
                        cookie: 2,
                        name_attributes: attributes.clone(),
                        name_handle: handle_len.map_or(post_op_fh3::None, |len| {
                            post_op_fh3::Some(nfs_fh3 {
                                data: Opaque::owned(vec![7; len]),
                            })
                        }),
                    };
                    let entry_bytes = encoded(&entry);
                    let mut golden = 1_u64.to_be_bytes().to_vec();
                    golden.extend_from_slice(
                        &u32::try_from(name_len)
                            .expect("small fixture")
                            .to_be_bytes(),
                    );
                    golden.extend_from_slice(&vec![b'x'; name_len]);
                    golden.resize(12 + name_len.div_ceil(4) * 4, 0);
                    golden.extend_from_slice(&2_u64.to_be_bytes());
                    let mut attr_golden = vec![
                        0,
                        0,
                        0,
                        u8::from(matches!(attributes, post_op_attr::Some(_))),
                    ];
                    if matches!(attributes, post_op_attr::Some(_)) {
                        attr_golden.extend_from_slice(&[0, 0, 0, 1]); // NF3REG
                        attr_golden.extend_from_slice(&[0; 80]);
                    }
                    golden.extend_from_slice(&attr_golden);
                    golden.extend_from_slice(&[0, 0, 0, u8::from(handle_len.is_some())]);
                    if let Some(len) = handle_len {
                        golden.extend_from_slice(
                            &u32::try_from(len).expect("small fixture").to_be_bytes(),
                        );
                        golden.extend_from_slice(&vec![7; len]);
                        golden.resize(golden.len() + (4 - len % 4) % 4, 0);
                    }
                    assert_eq!(entry_bytes, golden);
                    assert_eq!(entryplus_size(&entry), Some(entry_bytes.len()));
                    let empty = READDIRPLUS3resok {
                        dir_attributes: attributes.clone(),
                        cookieverf: cookieverf3::default(),
                        reply: dirlistplus3 {
                            entries: List(vec![]),
                            eof: true,
                        },
                    };
                    let mut empty_golden = attr_golden.clone();
                    empty_golden.extend_from_slice(&[0; 12]); // verifier, final false
                    empty_golden.extend_from_slice(&[0, 0, 0, 1]); // EOF
                    assert_eq!(encoded(&empty), empty_golden);
                    let empty_len = empty_golden.len();
                    let page = READDIRPLUS3resok {
                        reply: dirlistplus3 {
                            entries: List(vec![entry]),
                            eof: false,
                        },
                        ..empty
                    };
                    let mut page_golden = attr_golden;
                    page_golden.extend_from_slice(&[0; 8]); // verifier
                    page_golden.extend_from_slice(&[0, 0, 0, 1]); // true entry
                    page_golden.extend_from_slice(&golden);
                    page_golden.extend_from_slice(&[0; 8]); // final false, non-EOF
                    assert_eq!(encoded(&page), page_golden);
                    assert_eq!(page_golden.len(), empty_len + 4 + entry_bytes.len());
                }
            }
        }
        assert_eq!(opaque_size(usize::MAX), None);
        #[cfg(target_pointer_width = "64")]
        assert_eq!(opaque_size(u32::MAX as usize + 1), None);
    }

    #[test]
    fn test_dircount() {
        let mut list = BoundedEntryPlusList::new(70, 1000);
        list.try_push(make_entry("test")).unwrap();
        assert_eq!(list.accumulated_dircount, 28);
        list.try_push(make_entry("test2")).unwrap();
        assert_eq!(list.accumulated_dircount, 60);
        assert!(list.try_push(make_entry("test3")).is_err());
        assert_eq!(list.accumulated_dircount, 60);

        let entries = list.into_inner();
        assert!(encoded(&entries).len() < 1000);
        assert_eq!(entries.0.len(), 2);
    }

    #[test]
    fn test_maxcount() {
        let mut list = BoundedEntryPlusList::new(1000, 100);
        list.try_push(make_entry("test")).unwrap();
        assert_eq!(list.accumulated_dircount, 28);
        list.try_push(make_entry("test2")).unwrap();
        assert_eq!(list.accumulated_dircount, 60);
        assert!(list.try_push(make_entry("test3")).is_err());
        assert_eq!(list.accumulated_dircount, 60);

        let entries = list.into_inner();
        assert_eq!(encoded(&entries).len(), 80);
        assert_eq!(entries.0.len(), 2);
    }

    fn make_entry(name: &str) -> entryplus3<'_> {
        entryplus3 {
            fileid: 0,
            name: filename3(Opaque::borrowed(name.as_bytes())),
            cookie: 0,
            name_attributes: post_op_attr::None,
            name_handle: post_op_fh3::None,
        }
    }
}
