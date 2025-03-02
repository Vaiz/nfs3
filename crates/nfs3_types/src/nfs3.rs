#![allow(
    non_camel_case_types,
    clippy::large_enum_variant,
    clippy::upper_case_acronyms
)]

//! This module contains the definitions of the NFSv3 protocol as defined in RFC 1813.

use nfs3_macros::XdrCodec;

use crate::xdr_codec::{List, Opaque, Pack, PackedSize, Read, Result, Unpack, Write};

pub const PROGRAM: u32 = 100003;
pub const VERSION: u32 = 3;

pub const ACCESS3_READ: u32 = 1;
pub const ACCESS3_LOOKUP: u32 = 2;
pub const ACCESS3_MODIFY: u32 = 4;
pub const ACCESS3_EXTEND: u32 = 8;
pub const ACCESS3_DELETE: u32 = 16;
pub const ACCESS3_EXECUTE: u32 = 32;

pub const FSF3_LINK: u32 = 1;
pub const FSF3_SYMLINK: u32 = 2;
pub const FSF3_HOMOGENEOUS: u32 = 8;
pub const FSF3_CANSETTIME: u32 = 16;

pub const NFS3_COOKIEVERFSIZE: usize = 8;
pub const NFS3_CREATEVERFSIZE: usize = 8;
pub const NFS3_FHSIZE: usize = 64;
pub const NFS3_WRITEVERFSIZE: usize = 8;

#[derive(Debug)]
pub enum Nfs3Result<T, E> {
    Ok(T),
    Err((nfsstat3, E)),
}

impl<T, E: std::fmt::Debug> Nfs3Result<T, E> {
    pub fn unwrap(self) -> T {
        match self {
            Nfs3Result::Ok(val) => val,
            Nfs3Result::Err((code, res)) => panic!("NFS3 error: {code:?}, result: {res:?}"),
        }
    }
}

impl<Out, T, E> Pack<Out> for Nfs3Result<T, E>
where
    Out: Write,
    T: Pack<Out>,
    E: Pack<Out>,
{
    fn pack(&self, out: &mut Out) -> Result<usize> {
        let len = match self {
            Nfs3Result::Ok(v) => nfsstat3::NFS3_OK.pack(out)? + v.pack(out)?,
            Nfs3Result::Err((code, err)) => code.pack(out)? + err.pack(out)?,
        };
        Ok(len)
    }
}

impl<In, T, E> Unpack<In> for Nfs3Result<T, E>
where
    In: Read,
    T: Unpack<In>,
    E: Unpack<In>,
{
    fn unpack(input: &mut In) -> Result<(Self, usize)> {
        let mut sz = 0;
        let (code, dsz): (nfsstat3, usize) = Unpack::unpack(input)?;
        sz += dsz;
        match code {
            nfsstat3::NFS3_OK => {
                let (val, fsz) = Unpack::unpack(input)?;
                sz += fsz;
                Ok((Self::Ok(val), sz))
            }
            _ => {
                let (val, csz) = Unpack::unpack(input)?;
                sz += csz;
                Ok((Self::Err((code, val)), sz))
            }
        }
    }
}

impl<T, E> PackedSize for Nfs3Result<T, E>
where
    T: PackedSize,
    E: PackedSize,
{
    const PACKED_SIZE: Option<usize> = None;

    fn count_packed_size(&self) -> usize {
        4 + match self {
            Nfs3Result::Ok(v) => v.packed_size(),
            Nfs3Result::Err((code, err)) => code.packed_size() + err.packed_size(),
        }
    }
}

pub type ACCESS3res = Nfs3Result<ACCESS3resok, ACCESS3resfail>;
pub type COMMIT3res = Nfs3Result<COMMIT3resok, COMMIT3resfail>;
pub type CREATE3res = Nfs3Result<CREATE3resok, CREATE3resfail>;
pub type FSINFO3res = Nfs3Result<FSINFO3resok, FSINFO3resfail>;
pub type FSSTAT3res = Nfs3Result<FSSTAT3resok, FSSTAT3resfail>;
pub type GETATTR3res = Nfs3Result<GETATTR3resok, ()>;
pub type LINK3res = Nfs3Result<LINK3resok, LINK3resfail>;
pub type LOOKUP3res = Nfs3Result<LOOKUP3resok, LOOKUP3resfail>;
pub type MKDIR3res = Nfs3Result<MKDIR3resok, MKDIR3resfail>;
pub type MKNOD3res = Nfs3Result<MKNOD3resok, MKNOD3resfail>;
pub type PATHCONF3res = Nfs3Result<PATHCONF3resok, PATHCONF3resfail>;
pub type READ3res<'a> = Nfs3Result<READ3resok<'a>, READ3resfail>;
pub type READDIR3res<'a> = Nfs3Result<READDIR3resok<'a>, READDIR3resfail>;
pub type READDIRPLUS3res<'a> = Nfs3Result<READDIRPLUS3resok<'a>, READDIRPLUS3resfail>;
pub type READLINK3res<'a> = Nfs3Result<READLINK3resok<'a>, READLINK3resfail>;
pub type REMOVE3res = Nfs3Result<REMOVE3resok, REMOVE3resfail>;
pub type RENAME3res = Nfs3Result<RENAME3resok, RENAME3resfail>;
pub type RMDIR3res = Nfs3Result<RMDIR3resok, RMDIR3resfail>;
pub type SETATTR3res = Nfs3Result<SETATTR3resok, SETATTR3resfail>;
pub type SYMLINK3res = Nfs3Result<SYMLINK3resok, SYMLINK3resfail>;
pub type WRITE3res = Nfs3Result<WRITE3resok, WRITE3resfail>;

#[derive(Debug, Clone, Default)]
pub enum Nfs3Option<T> {
    Some(T),
    #[default]
    None,
}

impl<Out, T> Pack<Out> for Nfs3Option<T>
where
    Out: Write,
    T: Pack<Out>,
{
    fn pack(&self, out: &mut Out) -> Result<usize> {
        let len = match self {
            Nfs3Option::Some(v) => 1.pack(out)? + v.pack(out)?,
            Nfs3Option::None => 0.pack(out)?,
        };
        Ok(len)
    }
}

impl<In, T> Unpack<In> for Nfs3Option<T>
where
    In: Read,
    T: Unpack<In>,
{
    fn unpack(input: &mut In) -> Result<(Self, usize)> {
        let mut sz = 0;
        let (tag, tsz): (u32, usize) = Unpack::unpack(input)?;
        sz += tsz;
        match tag {
            1 => {
                let (val, vsz) = Unpack::unpack(input)?;
                sz += vsz;
                Ok((Self::Some(val), sz))
            }
            _ => Ok((Self::None, sz)),
        }
    }
}

impl<T: PackedSize> PackedSize for Nfs3Option<T> {
    const PACKED_SIZE: Option<usize> = None;

    fn count_packed_size(&self) -> usize {
        4 + match self {
            Nfs3Option::Some(v) => v.packed_size(),
            Nfs3Option::None => 0,
        }
    }
}

pub type pre_op_attr = Nfs3Option<wcc_attr>;
pub type post_op_attr = Nfs3Option<fattr3>;
pub type post_op_fh3 = Nfs3Option<nfs_fh3>;
pub type sattrguard3 = Nfs3Option<nfstime3>;
pub type set_gid3 = Nfs3Option<gid3>;
pub type set_mode3 = Nfs3Option<mode3>;
pub type set_size3 = Nfs3Option<size3>;
pub type set_uid3 = Nfs3Option<uid3>;

#[derive(Debug, XdrCodec)]
pub struct ACCESS3args {
    pub object: nfs_fh3,
    pub access: u32,
}

#[derive(Debug, XdrCodec)]
pub struct ACCESS3resfail {
    pub obj_attributes: post_op_attr,
}

#[derive(Debug, XdrCodec)]
pub struct ACCESS3resok {
    pub obj_attributes: post_op_attr,
    pub access: u32,
}

#[derive(Debug, XdrCodec)]
pub struct COMMIT3args {
    pub file: nfs_fh3,
    pub offset: offset3,
    pub count: count3,
}

#[derive(Debug, XdrCodec)]
pub struct COMMIT3resfail {
    pub file_wcc: wcc_data,
}

#[derive(Debug, XdrCodec)]
pub struct COMMIT3resok {
    pub file_wcc: wcc_data,
    pub verf: writeverf3,
}

#[derive(Debug, XdrCodec)]
pub struct CREATE3args<'a> {
    pub where_: diropargs3<'a>,
    pub how: createhow3,
}

#[derive(Debug, XdrCodec)]
pub struct CREATE3resfail {
    pub dir_wcc: wcc_data,
}

#[derive(Debug, XdrCodec)]
pub struct CREATE3resok {
    pub obj: post_op_fh3,
    pub obj_attributes: post_op_attr,
    pub dir_wcc: wcc_data,
}

#[derive(Clone, Debug, Eq, PartialEq, XdrCodec)]
pub struct FSINFO3args {
    pub fsroot: nfs_fh3,
}

#[derive(Debug, XdrCodec)]
pub struct FSINFO3resfail {
    pub obj_attributes: post_op_attr,
}

#[derive(Debug, XdrCodec)]
pub struct FSINFO3resok {
    pub obj_attributes: post_op_attr,
    pub rtmax: u32,
    pub rtpref: u32,
    pub rtmult: u32,
    pub wtmax: u32,
    pub wtpref: u32,
    pub wtmult: u32,
    pub dtpref: u32,
    pub maxfilesize: size3,
    pub time_delta: nfstime3,
    pub properties: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, XdrCodec)]
pub struct FSSTAT3args {
    pub fsroot: nfs_fh3,
}

#[derive(Debug, XdrCodec)]
pub struct FSSTAT3resfail {
    pub obj_attributes: post_op_attr,
}

#[derive(Debug, XdrCodec)]
pub struct FSSTAT3resok {
    pub obj_attributes: post_op_attr,
    pub tbytes: size3,
    pub fbytes: size3,
    pub abytes: size3,
    pub tfiles: size3,
    pub ffiles: size3,
    pub afiles: size3,
    pub invarsec: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, XdrCodec)]
pub struct GETATTR3args {
    pub object: nfs_fh3,
}

#[derive(Debug, XdrCodec)]
pub struct GETATTR3resok {
    pub obj_attributes: fattr3,
}

#[derive(Debug, Eq, PartialEq, XdrCodec)]
pub struct LINK3args<'a> {
    pub file: nfs_fh3,
    pub link: diropargs3<'a>,
}

#[derive(Debug, XdrCodec)]
pub struct LINK3resfail {
    pub file_attributes: post_op_attr,
    pub linkdir_wcc: wcc_data,
}

#[derive(Debug, XdrCodec)]
pub struct LINK3resok {
    pub file_attributes: post_op_attr,
    pub linkdir_wcc: wcc_data,
}

#[derive(Debug, Eq, PartialEq, XdrCodec)]
pub struct LOOKUP3args<'a> {
    pub what: diropargs3<'a>,
}

#[derive(Debug, Default, XdrCodec)]
pub struct LOOKUP3resfail {
    pub dir_attributes: post_op_attr,
}

#[derive(Debug, XdrCodec)]
pub struct LOOKUP3resok {
    pub object: nfs_fh3,
    pub obj_attributes: post_op_attr,
    pub dir_attributes: post_op_attr,
}

#[derive(Debug, XdrCodec)]
pub struct MKDIR3args<'a> {
    pub where_: diropargs3<'a>,
    pub attributes: sattr3,
}

#[derive(Debug, XdrCodec)]
pub struct MKDIR3resfail {
    pub dir_wcc: wcc_data,
}

#[derive(Debug, XdrCodec)]
pub struct MKDIR3resok {
    pub obj: post_op_fh3,
    pub obj_attributes: post_op_attr,
    pub dir_wcc: wcc_data,
}

#[derive(Debug, XdrCodec)]
pub struct MKNOD3args<'a> {
    pub where_: diropargs3<'a>,
    pub what: mknoddata3,
}

#[derive(Debug, XdrCodec)]
pub struct MKNOD3resfail {
    pub dir_wcc: wcc_data,
}

#[derive(Debug, XdrCodec)]
pub struct MKNOD3resok {
    pub obj: post_op_fh3,
    pub obj_attributes: post_op_attr,
    pub dir_wcc: wcc_data,
}

#[derive(Clone, Debug, Eq, PartialEq, XdrCodec)]
pub struct PATHCONF3args {
    pub object: nfs_fh3,
}

#[derive(Debug, XdrCodec)]
pub struct PATHCONF3resfail {
    pub obj_attributes: post_op_attr,
}

#[derive(Debug, XdrCodec)]
pub struct PATHCONF3resok {
    pub obj_attributes: post_op_attr,
    pub linkmax: u32,
    pub name_max: u32,
    pub no_trunc: bool,
    pub chown_restricted: bool,
    pub case_insensitive: bool,
    pub case_preserving: bool,
}

#[derive(Debug, XdrCodec)]
pub struct READ3args {
    pub file: nfs_fh3,
    pub offset: offset3,
    pub count: count3,
}

#[derive(Debug, XdrCodec)]
pub struct READ3resfail {
    pub file_attributes: post_op_attr,
}

#[derive(Debug, XdrCodec)]
pub struct READ3resok<'a> {
    pub file_attributes: post_op_attr,
    pub count: count3,
    pub eof: bool,
    pub data: Opaque<'a>,
}

#[derive(Debug, XdrCodec)]
pub struct READDIR3args {
    pub dir: nfs_fh3,
    pub cookie: cookie3,
    pub cookieverf: cookieverf3,
    pub count: count3,
}

#[derive(Debug, Default, XdrCodec)]
pub struct READDIR3resfail {
    pub dir_attributes: post_op_attr,
}

#[derive(Debug, XdrCodec)]
pub struct READDIR3resok<'a> {
    pub dir_attributes: post_op_attr,
    pub cookieverf: cookieverf3,
    pub reply: dirlist3<'a>,
}

#[derive(Debug, XdrCodec)]
pub struct READDIRPLUS3args {
    pub dir: nfs_fh3,
    pub cookie: cookie3,
    pub cookieverf: cookieverf3,
    pub dircount: count3,
    pub maxcount: count3,
}

#[derive(Default, Debug, XdrCodec)]
pub struct READDIRPLUS3resfail {
    pub dir_attributes: post_op_attr,
}

#[derive(Debug, XdrCodec)]
pub struct READDIRPLUS3resok<'a> {
    pub dir_attributes: post_op_attr,
    pub cookieverf: cookieverf3,
    pub reply: dirlistplus3<'a>,
}

#[derive(Clone, Debug, Eq, PartialEq, XdrCodec)]
pub struct READLINK3args {
    pub symlink: nfs_fh3,
}

#[derive(Default, Debug, XdrCodec)]
pub struct READLINK3resfail {
    pub symlink_attributes: post_op_attr,
}

#[derive(Debug, XdrCodec)]
pub struct READLINK3resok<'a> {
    pub symlink_attributes: post_op_attr,
    pub data: nfspath3<'a>,
}

#[derive(Debug, Eq, PartialEq, XdrCodec)]
pub struct REMOVE3args<'a> {
    pub object: diropargs3<'a>,
}

#[derive(Debug, XdrCodec)]
pub struct REMOVE3resfail {
    pub dir_wcc: wcc_data,
}

#[derive(Debug, XdrCodec)]
pub struct REMOVE3resok {
    pub dir_wcc: wcc_data,
}

#[derive(Debug, Eq, PartialEq, XdrCodec)]
pub struct RENAME3args<'a, 'b> {
    pub from: diropargs3<'a>,
    pub to: diropargs3<'b>,
}

#[derive(Debug, XdrCodec)]
pub struct RENAME3resfail {
    pub fromdir_wcc: wcc_data,
    pub todir_wcc: wcc_data,
}

#[derive(Debug, XdrCodec)]
pub struct RENAME3resok {
    pub fromdir_wcc: wcc_data,
    pub todir_wcc: wcc_data,
}

#[derive(Debug, Eq, PartialEq, XdrCodec)]
pub struct RMDIR3args<'a> {
    pub object: diropargs3<'a>,
}

#[derive(Debug, XdrCodec)]
pub struct RMDIR3resfail {
    pub dir_wcc: wcc_data,
}

#[derive(Debug, XdrCodec)]
pub struct RMDIR3resok {
    pub dir_wcc: wcc_data,
}

#[derive(Debug, XdrCodec)]
pub struct SETATTR3args {
    pub object: nfs_fh3,
    pub new_attributes: sattr3,
    pub guard: sattrguard3,
}

#[derive(Debug, XdrCodec)]
pub struct SETATTR3resfail {
    pub obj_wcc: wcc_data,
}

#[derive(Debug, XdrCodec)]
pub struct SETATTR3resok {
    pub obj_wcc: wcc_data,
}

#[derive(Debug, XdrCodec)]
pub struct SYMLINK3args<'a> {
    pub where_: diropargs3<'a>,
    pub symlink: symlinkdata3<'a>,
}

#[derive(Debug, XdrCodec)]
pub struct SYMLINK3resfail {
    pub dir_wcc: wcc_data,
}

#[derive(Debug, XdrCodec)]
pub struct SYMLINK3resok {
    pub obj: post_op_fh3,
    pub obj_attributes: post_op_attr,
    pub dir_wcc: wcc_data,
}

#[derive(Debug, XdrCodec)]
pub struct WRITE3args<'a> {
    pub file: nfs_fh3,
    pub offset: offset3,
    pub count: count3,
    pub stable: stable_how,
    pub data: Opaque<'a>,
}

#[derive(Debug, XdrCodec)]
pub struct WRITE3resfail {
    pub file_wcc: wcc_data,
}

#[derive(Debug, XdrCodec)]
pub struct WRITE3resok {
    pub file_wcc: wcc_data,
    pub count: count3,
    pub committed: stable_how,
    pub verf: writeverf3,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct cookieverf3(pub [u8; NFS3_COOKIEVERFSIZE]);

#[derive(Debug)]
pub enum createhow3 {
    UNCHECKED(sattr3),
    GUARDED(sattr3),
    EXCLUSIVE(createverf3),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, XdrCodec)]
pub enum createmode3 {
    UNCHECKED = 0,
    GUARDED = 1,
    EXCLUSIVE = 2,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct createverf3(pub [u8; NFS3_CREATEVERFSIZE]);

#[derive(Debug, XdrCodec)]
pub struct devicedata3 {
    pub dev_attributes: sattr3,
    pub spec: specdata3,
}

#[derive(Debug, Default, XdrCodec)]
pub struct dirlist3<'a> {
    pub entries: List<entry3<'a>>,
    pub eof: bool,
}

#[derive(Debug, XdrCodec)]
pub struct dirlistplus3<'a> {
    pub entries: List<entryplus3<'a>>,
    pub eof: bool,
}

#[derive(Debug, Eq, PartialEq, XdrCodec)]
pub struct diropargs3<'a> {
    pub dir: nfs_fh3,
    pub name: filename3<'a>,
}

#[derive(Debug, XdrCodec, PartialEq)]
pub struct entry3<'a> {
    pub fileid: fileid3,
    pub name: filename3<'a>,
    pub cookie: cookie3,
}

#[derive(Debug, XdrCodec)]
pub struct entryplus3<'a> {
    pub fileid: fileid3,
    pub name: filename3<'a>,
    pub cookie: cookie3,
    pub name_attributes: post_op_attr,
    pub name_handle: post_op_fh3,
}

#[derive(Debug, Clone, XdrCodec)]
pub struct fattr3 {
    pub type_: ftype3,
    pub mode: mode3,
    pub nlink: u32,
    pub uid: uid3,
    pub gid: gid3,
    pub size: size3,
    pub used: size3,
    pub rdev: specdata3,
    pub fsid: u64,
    pub fileid: fileid3,
    pub atime: nfstime3,
    pub mtime: nfstime3,
    pub ctime: nfstime3,
}

#[derive(Debug, Eq, PartialEq, XdrCodec)]
pub struct filename3<'a>(pub Opaque<'a>);

impl From<Vec<u8>> for filename3<'static> {
    fn from(name: Vec<u8>) -> Self {
        Self(Opaque::owned(name))
    }
}

impl<'a> From<&'a [u8]> for filename3<'a> {
    fn from(name: &'a [u8]) -> Self {
        Self(Opaque::borrowed(name))
    }
}

impl AsRef<[u8]> for filename3<'_> {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl filename3<'_> {
    pub fn clone_to_owned(&self) -> filename3<'static> {
        self.0.to_vec().into()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl PartialEq<[u8]> for filename3<'_> {
    fn eq(&self, other: &[u8]) -> bool {
        self.0.as_ref() == other
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, XdrCodec)]
pub enum ftype3 {
    NF3REG = 1,
    NF3DIR = 2,
    NF3BLK = 3,
    NF3CHR = 4,
    NF3LNK = 5,
    NF3SOCK = 6,
    NF3FIFO = 7,
}

#[derive(Debug)]
pub enum mknoddata3 {
    NF3CHR(devicedata3),
    NF3BLK(devicedata3),
    NF3SOCK(sattr3),
    NF3FIFO(sattr3),
    default,
}

#[derive(Clone, Debug, Eq, PartialEq, XdrCodec)]
pub struct nfs_fh3 {
    pub data: xdr_codec::Opaque<'static>,
}
impl Default for nfs_fh3 {
    fn default() -> Self {
        Self {
            data: xdr_codec::Opaque::borrowed(&[]),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, XdrCodec)]
pub struct nfspath3<'a>(pub Opaque<'a>);

impl From<Vec<u8>> for nfspath3<'static> {
    fn from(name: Vec<u8>) -> Self {
        Self(Opaque::owned(name))
    }
}

impl<'a> From<&'a [u8]> for nfspath3<'a> {
    fn from(name: &'a [u8]) -> Self {
        Self(Opaque::borrowed(name))
    }
}

impl AsRef<[u8]> for nfspath3<'_> {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl nfspath3<'_> {
    pub fn clone_to_owned(&self) -> nfspath3<'static> {
        self.0.to_vec().into()
    }
}

impl PartialEq<[u8]> for nfspath3<'_> {
    fn eq(&self, other: &[u8]) -> bool {
        self.0.as_ref() == other
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, XdrCodec)]
pub enum nfsstat3 {
    NFS3_OK = 0,
    NFS3ERR_PERM = 1,
    NFS3ERR_NOENT = 2,
    NFS3ERR_IO = 5,
    NFS3ERR_NXIO = 6,
    NFS3ERR_ACCES = 13,
    NFS3ERR_EXIST = 17,
    NFS3ERR_XDEV = 18,
    NFS3ERR_NODEV = 19,
    NFS3ERR_NOTDIR = 20,
    NFS3ERR_ISDIR = 21,
    NFS3ERR_INVAL = 22,
    NFS3ERR_FBIG = 27,
    NFS3ERR_NOSPC = 28,
    NFS3ERR_ROFS = 30,
    NFS3ERR_MLINK = 31,
    NFS3ERR_NAMETOOLONG = 63,
    NFS3ERR_NOTEMPTY = 66,
    NFS3ERR_DQUOT = 69,
    NFS3ERR_STALE = 70,
    NFS3ERR_REMOTE = 71,
    NFS3ERR_BADHANDLE = 10001,
    NFS3ERR_NOT_SYNC = 10002,
    NFS3ERR_BAD_COOKIE = 10003,
    NFS3ERR_NOTSUPP = 10004,
    NFS3ERR_TOOSMALL = 10005,
    NFS3ERR_SERVERFAULT = 10006,
    NFS3ERR_BADTYPE = 10007,
    NFS3ERR_JUKEBOX = 10008,
}

#[derive(Clone, Default, Debug, Eq, PartialEq, XdrCodec)]
pub struct nfstime3 {
    pub seconds: u32,
    pub nseconds: u32,
}

impl TryFrom<std::time::SystemTime> for nfstime3 {
    type Error = std::time::SystemTimeError;

    fn try_from(time: std::time::SystemTime) -> std::result::Result<Self, Self::Error> {
        time.duration_since(std::time::UNIX_EPOCH)
            .map(|duration| Self {
                seconds: duration.as_secs().min(u32::MAX as u64) as u32,
                nseconds: duration.subsec_nanos(),
            })
    }
}

#[derive(Debug, Clone, XdrCodec)]
pub struct sattr3 {
    pub mode: set_mode3,
    pub uid: set_uid3,
    pub gid: set_gid3,
    pub size: set_size3,
    pub atime: set_atime,
    pub mtime: set_mtime,
}

#[derive(Debug, Clone)]
pub enum set_atime {
    DONT_CHANGE,                  // = 0,
    SET_TO_SERVER_TIME,           // = 1,
    SET_TO_CLIENT_TIME(nfstime3), // = 2,
}

#[derive(Debug, Clone)]
pub enum set_mtime {
    DONT_CHANGE,                  // = 0,
    SET_TO_SERVER_TIME,           // = 1,
    SET_TO_CLIENT_TIME(nfstime3), // = 2,
}

#[derive(Clone, Default, Debug, Eq, PartialEq, XdrCodec)]
pub struct specdata3 {
    pub specdata1: u32,
    pub specdata2: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, XdrCodec)]
pub enum stable_how {
    UNSTABLE = 0,
    DATA_SYNC = 1,
    FILE_SYNC = 2,
}

#[derive(Debug, XdrCodec)]
pub struct symlinkdata3<'a> {
    pub symlink_attributes: sattr3,
    pub symlink_data: nfspath3<'a>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum time_how {
    DONT_CHANGE = 0,
    SET_TO_SERVER_TIME = 1,
    SET_TO_CLIENT_TIME = 2,
}

#[derive(Debug, XdrCodec)]
pub struct wcc_attr {
    pub size: size3,
    pub mtime: nfstime3,
    pub ctime: nfstime3,
}

#[derive(Debug, Default, XdrCodec)]
pub struct wcc_data {
    pub before: pre_op_attr,
    pub after: post_op_attr,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct writeverf3(pub [u8; NFS3_WRITEVERFSIZE]);

pub type cookie3 = u64;

pub type count3 = u32;

pub type fileid3 = u64;

pub type gid3 = u32;

pub type mode3 = u32;

pub type offset3 = u64;

pub type size3 = u64;

pub type uid3 = u32;

impl<Out: Write> Pack<Out> for cookieverf3 {
    fn pack(&self, out: &mut Out) -> Result<usize> {
        xdr_codec::pack_opaque_array(&self.0[..], self.0.len(), out)
    }
}

impl PackedSize for cookieverf3 {
    const PACKED_SIZE: Option<usize> = Some(NFS3_COOKIEVERFSIZE);

    fn count_packed_size(&self) -> usize {
        NFS3_COOKIEVERFSIZE
    }
}

impl<Out: Write> Pack<Out> for createhow3 {
    fn pack(&self, out: &mut Out) -> Result<usize> {
        Ok(match self {
            createhow3::UNCHECKED(val) => createmode3::UNCHECKED.pack(out)? + val.pack(out)?,
            createhow3::GUARDED(val) => createmode3::GUARDED.pack(out)? + val.pack(out)?,
            createhow3::EXCLUSIVE(val) => createmode3::EXCLUSIVE.pack(out)? + val.pack(out)?,
        })
    }
}

impl PackedSize for createhow3 {
    const PACKED_SIZE: Option<usize> = None;

    fn count_packed_size(&self) -> usize {
        4 + match self {
            createhow3::UNCHECKED(val) => val.packed_size(),
            createhow3::GUARDED(val) => val.packed_size(),
            createhow3::EXCLUSIVE(val) => val.packed_size(),
        }
    }
}

impl<Out: Write> Pack<Out> for createverf3 {
    fn pack(&self, out: &mut Out) -> Result<usize> {
        xdr_codec::pack_opaque_array(&self.0[..], self.0.len(), out)
    }
}

impl PackedSize for createverf3 {
    const PACKED_SIZE: Option<usize> = Some(NFS3_CREATEVERFSIZE);

    fn count_packed_size(&self) -> usize {
        NFS3_CREATEVERFSIZE
    }
}

impl<Out: Write> Pack<Out> for mknoddata3 {
    fn pack(&self, out: &mut Out) -> Result<usize> {
        Ok(match self {
            mknoddata3::NF3CHR(val) => ftype3::NF3CHR.pack(out)? + val.pack(out)?,
            mknoddata3::NF3BLK(val) => ftype3::NF3BLK.pack(out)? + val.pack(out)?,
            mknoddata3::NF3SOCK(val) => ftype3::NF3SOCK.pack(out)? + val.pack(out)?,
            mknoddata3::NF3FIFO(val) => ftype3::NF3FIFO.pack(out)? + val.pack(out)?,
            &mknoddata3::default => return Err(xdr_codec::Error::invalidcase(-1)),
        })
    }
}

impl PackedSize for mknoddata3 {
    const PACKED_SIZE: Option<usize> = None;

    fn count_packed_size(&self) -> usize {
        4 + match self {
            mknoddata3::NF3CHR(val) => val.packed_size(),
            mknoddata3::NF3BLK(val) => val.packed_size(),
            mknoddata3::NF3SOCK(val) => val.packed_size(),
            mknoddata3::NF3FIFO(val) => val.packed_size(),
            mknoddata3::default => 0,
        }
    }
}

impl<Out: Write> Pack<Out> for set_atime {
    fn pack(&self, out: &mut Out) -> Result<usize> {
        let len = match self {
            Self::DONT_CHANGE => 0.pack(out)?,
            Self::SET_TO_SERVER_TIME => 1.pack(out)?,
            Self::SET_TO_CLIENT_TIME(val) => 2.pack(out)? + val.pack(out)?,
        };
        Ok(len)
    }
}

impl PackedSize for set_atime {
    const PACKED_SIZE: Option<usize> = None;

    fn count_packed_size(&self) -> usize {
        4 + match self {
            Self::DONT_CHANGE => 0,
            Self::SET_TO_SERVER_TIME => 0,
            Self::SET_TO_CLIENT_TIME(val) => val.packed_size(),
        }
    }
}

impl<Out: Write> Pack<Out> for set_mtime {
    fn pack(&self, out: &mut Out) -> Result<usize> {
        let len = match self {
            Self::DONT_CHANGE => 0.pack(out)?,
            Self::SET_TO_SERVER_TIME => 1.pack(out)?,
            Self::SET_TO_CLIENT_TIME(val) => 2.pack(out)? + val.pack(out)?,
        };
        Ok(len)
    }
}

impl PackedSize for set_mtime {
    const PACKED_SIZE: Option<usize> = None;

    fn count_packed_size(&self) -> usize {
        4 + match self {
            Self::DONT_CHANGE => 0,
            Self::SET_TO_SERVER_TIME => 0,
            Self::SET_TO_CLIENT_TIME(val) => val.packed_size(),
        }
    }
}

impl<Out: Write> Pack<Out> for writeverf3 {
    fn pack(&self, out: &mut Out) -> Result<usize> {
        xdr_codec::pack_opaque_array(&self.0[..], self.0.len(), out)
    }
}

impl PackedSize for writeverf3 {
    const PACKED_SIZE: Option<usize> = Some(NFS3_WRITEVERFSIZE);

    fn count_packed_size(&self) -> usize {
        NFS3_WRITEVERFSIZE
    }
}

impl<In: Read> Unpack<In> for cookieverf3 {
    fn unpack(input: &mut In) -> Result<(Self, usize)> {
        let (buf, sz) = unpack_array::<NFS3_COOKIEVERFSIZE, _>(input)?;
        Ok((cookieverf3(buf), sz))
    }
}

impl<In: Read> Unpack<In> for createhow3 {
    fn unpack(input: &mut In) -> Result<(Self, usize)> {
        let mut sz = 0;
        let (v, dsz): (i32, usize) = Unpack::unpack(input)?;
        sz += dsz;

        match v {
            0 => {
                let (value, fsz) = Unpack::unpack(input)?;
                sz += fsz;
                Ok((Self::UNCHECKED(value), sz))
            }
            1 => {
                let (value, fsz) = Unpack::unpack(input)?;
                sz += fsz;
                Ok((Self::GUARDED(value), sz))
            }
            2 => {
                let (value, fsz) = Unpack::unpack(input)?;
                sz += fsz;
                Ok((Self::EXCLUSIVE(value), sz))
            }
            _ => Err(xdr_codec::Error::invalidcase(v)),
        }
    }
}

impl<In: Read> Unpack<In> for createverf3 {
    fn unpack(input: &mut In) -> Result<(Self, usize)> {
        let (buf, sz) = unpack_array::<NFS3_CREATEVERFSIZE, _>(input)?;
        Ok((createverf3(buf), sz))
    }
}

impl<In: Read> Unpack<In> for mknoddata3 {
    fn unpack(input: &mut In) -> Result<(Self, usize)> {
        let mut sz = 0;
        let (v, dsz): (i32, _) = Unpack::unpack(input)?;
        sz += dsz;

        let v = match v {
            4 => {
                let (val, fsz) = Unpack::unpack(input)?;
                sz += fsz;
                Self::NF3CHR(val)
            }
            3 => {
                let (val, fsz) = Unpack::unpack(input)?;
                sz += fsz;
                Self::NF3BLK(val)
            }
            6 => {
                let (val, fsz) = Unpack::unpack(input)?;
                sz += fsz;
                Self::NF3SOCK(val)
            }
            7 => {
                let (val, fsz) = Unpack::unpack(input)?;
                sz += fsz;
                Self::NF3FIFO(val)
            }
            _ => Self::default,
        };

        Ok((v, sz))
    }
}

impl<In: Read> Unpack<In> for set_atime {
    fn unpack(input: &mut In) -> Result<(Self, usize)> {
        let mut sz = 0;
        let (v, dsz): (i32, _) = Unpack::unpack(input)?;
        sz += dsz;

        match v {
            0 => Ok((Self::DONT_CHANGE, sz)),
            1 => Ok((Self::SET_TO_SERVER_TIME, sz)),
            2 => {
                let (v, fsz) = Unpack::unpack(input)?;
                sz += fsz;
                Ok((Self::SET_TO_CLIENT_TIME(v), sz))
            }
            _ => Err(xdr_codec::Error::invalidcase(v)),
        }
    }
}

impl<In: Read> Unpack<In> for set_mtime {
    fn unpack(input: &mut In) -> Result<(Self, usize)> {
        let mut sz = 0;
        let (v, dsz): (i32, _) = Unpack::unpack(input)?;
        sz += dsz;

        match v {
            0 => Ok((Self::DONT_CHANGE, sz)),
            1 => Ok((Self::SET_TO_SERVER_TIME, sz)),
            2 => {
                let (v, fsz) = Unpack::unpack(input)?;
                sz += fsz;
                Ok((Self::SET_TO_CLIENT_TIME(v), sz))
            }
            _ => Err(xdr_codec::Error::invalidcase(v)),
        }
    }
}

impl<In: Read> Unpack<In> for writeverf3 {
    fn unpack(input: &mut In) -> Result<(Self, usize)> {
        let (buf, sz) = unpack_array::<NFS3_WRITEVERFSIZE, _>(input)?;
        Ok((writeverf3(buf), sz))
    }
}

fn unpack_array<const N: usize, In: Read>(input: &mut In) -> Result<([u8; N], usize)> {
    // TODO: optimize with using MaybeUninit::uninit_array
    let mut buf = [0u8; N];
    let sz = xdr_codec::unpack_opaque_array(input, &mut buf, N)?;
    assert_eq!(sz, N);
    Ok((buf, sz))
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, XdrCodec)]
pub enum NFS_PROGRAM {
    NFSPROC3_NULL = 0,
    NFSPROC3_GETATTR = 1,
    NFSPROC3_SETATTR = 2,
    NFSPROC3_LOOKUP = 3,
    NFSPROC3_ACCESS = 4,
    NFSPROC3_READLINK = 5,
    NFSPROC3_READ = 6,
    NFSPROC3_WRITE = 7,
    NFSPROC3_CREATE = 8,
    NFSPROC3_MKDIR = 9,
    NFSPROC3_SYMLINK = 10,
    NFSPROC3_MKNOD = 11,
    NFSPROC3_REMOVE = 12,
    NFSPROC3_RMDIR = 13,
    NFSPROC3_RENAME = 14,
    NFSPROC3_LINK = 15,
    NFSPROC3_READDIR = 16,
    NFSPROC3_READDIRPLUS = 17,
    NFSPROC3_FSSTAT = 18,
    NFSPROC3_FSINFO = 19,
    NFSPROC3_PATHCONF = 20,
    NFSPROC3_COMMIT = 21,
}

impl std::convert::TryFrom<u32> for NFS_PROGRAM {
    type Error = crate::xdr_codec::Error;

    fn try_from(value: u32) -> std::result::Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::NFSPROC3_NULL),
            1 => Ok(Self::NFSPROC3_GETATTR),
            2 => Ok(Self::NFSPROC3_SETATTR),
            3 => Ok(Self::NFSPROC3_LOOKUP),
            4 => Ok(Self::NFSPROC3_ACCESS),
            5 => Ok(Self::NFSPROC3_READLINK),
            6 => Ok(Self::NFSPROC3_READ),
            7 => Ok(Self::NFSPROC3_WRITE),
            8 => Ok(Self::NFSPROC3_CREATE),
            9 => Ok(Self::NFSPROC3_MKDIR),
            10 => Ok(Self::NFSPROC3_SYMLINK),
            11 => Ok(Self::NFSPROC3_MKNOD),
            12 => Ok(Self::NFSPROC3_REMOVE),
            13 => Ok(Self::NFSPROC3_RMDIR),
            14 => Ok(Self::NFSPROC3_RENAME),
            15 => Ok(Self::NFSPROC3_LINK),
            16 => Ok(Self::NFSPROC3_READDIR),
            17 => Ok(Self::NFSPROC3_READDIRPLUS),
            18 => Ok(Self::NFSPROC3_FSSTAT),
            19 => Ok(Self::NFSPROC3_FSINFO),
            20 => Ok(Self::NFSPROC3_PATHCONF),
            21 => Ok(Self::NFSPROC3_COMMIT),
            _ => Err(crate::xdr_codec::ErrorKind::InvalidEnum(value as i32).into()),
        }
    }
}
