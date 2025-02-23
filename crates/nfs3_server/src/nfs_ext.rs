use nfs3_types::nfs3::{cookie3, cookieverf3, entryplus3, fileid3, post_op_attr};
use nfs3_types::xdr_codec::{BoundedList, List};

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
            let cvf_version = ((attr.mtime.seconds as u64) << 32) | (attr.mtime.nseconds as u64);
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
    entries: BoundedList<entryplus3<'static>>,
    dircount: usize,
    accumulated_dircount: usize,
}

impl BoundedEntryPlusList {
    pub fn new(dircount: usize, maxcount: usize) -> Self {
        Self {
            entries: BoundedList::new(maxcount),
            dircount,
            accumulated_dircount: 0,
        }
    }

    pub fn try_push(&mut self, entry: entryplus3<'static>) -> Result<(), entryplus3> {
        let added_dircount = size_of::<fileid3>() // fileid
            + size_of::<u32>() + entry.name.len() // name
            + size_of::<cookie3>(); // cookie

        if self.accumulated_dircount + added_dircount > self.dircount {
            return Err(entry);
        }

        let result = self.entries.try_push(entry);
        if result.is_ok() {
            self.dircount += added_dircount;
        }
        result
    }

    pub fn into_inner(self) -> List<entryplus3<'static>> {
        self.entries.into_inner()
    }
}
