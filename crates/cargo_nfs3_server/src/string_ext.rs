/// Extension methods for converting between `OsString` and `nfs3_types`.
///
/// NOTE: This is something that works without any guarantees of correctly
/// handling OS encoding. It should be used for testing purposes only.
use std::ffi::{OsStr, OsString};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

use nfs3_server::nfs3_types::nfs3::{filename3, nfspath3};
#[cfg(not(unix))]
use nfs3_server::nfs3_types::xdr_codec::Opaque;

pub trait IntoOsString {
    fn as_os_str(&self) -> &OsStr;
    fn to_os_string(&self) -> OsString {
        self.as_os_str().to_os_string()
    }
}

pub trait FromOsString: Sized {
    fn from_os_str(osstr: &OsStr) -> Self;
    #[must_use]
    fn from_os_string(osstr: OsString) -> Self {
        Self::from_os_str(osstr.as_os_str())
    }
}

#[cfg(unix)]
impl IntoOsString for [u8] {
    fn as_os_str(&self) -> &OsStr {
        OsStr::from_bytes(self)
    }
}
#[cfg(unix)]
impl IntoOsString for filename3<'_> {
    fn as_os_str(&self) -> &OsStr {
        OsStr::from_bytes(self.as_ref())
    }
}

#[cfg(unix)]
impl FromOsString for filename3<'static> {
    fn from_os_str(osstr: &OsStr) -> Self {
        Self::from(osstr.as_bytes().to_vec())
    }
}

#[cfg(unix)]
impl IntoOsString for nfspath3<'_> {
    fn as_os_str(&self) -> &OsStr {
        OsStr::from_bytes(self.as_ref())
    }
}
#[cfg(unix)]
impl FromOsString for nfspath3<'static> {
    fn from_os_str(osstr: &OsStr) -> Self {
        Self::from(osstr.as_bytes().to_vec())
    }
}

#[cfg(not(unix))]
impl IntoOsString for [u8] {
    fn as_os_str(&self) -> &OsStr {
        std::str::from_utf8(self)
            .expect("cannot convert bytes to utf8 string")
            .as_ref()
    }
}
#[cfg(not(unix))]
impl IntoOsString for filename3<'_> {
    fn as_os_str(&self) -> &OsStr {
        self.as_ref().as_os_str()
    }
}

#[cfg(not(unix))]
impl FromOsString for filename3<'_> {
    fn from_os_str(osstr: &OsStr) -> Self {
        Self(Opaque::owned(
            osstr
                .to_str()
                .expect("cannot convert OsStr to utf8 string")
                .into(),
        ))
    }
}

#[cfg(not(unix))]
impl FromOsString for nfspath3<'_> {
    fn from_os_str(osstr: &OsStr) -> Self {
        Self(Opaque::owned(
            osstr
                .to_str()
                .expect("cannot convert OsStr to utf8 string")
                .into(),
        ))
    }
}
