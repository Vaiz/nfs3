use nfs3_types::{
    nfs3::{Nfs3Result, fattr3, nfs_fh3, nfsstat3},
    xdr_codec::Opaque,
};

/// Core trait needed for `JustClientExt` to operate
pub trait JustClient {
    type IO: nfs3_client::io::AsyncRead + nfs3_client::io::AsyncWrite + Send;

    /// Get a mutable reference to the underlying NFS3 connection
    ///
    /// This is the only required method for using `JustClientExt` trait methods.
    fn client(&mut self) -> &mut nfs3_client::Nfs3Client<Self::IO>;
}

/// Extension trait providing simplified NFS3 client operations for testing purposes
#[expect(async_fn_in_trait)]
pub trait JustClientExt: JustClient {
    /// Lookup a file on the NFS server and return its filehandle
    async fn just_lookup(&mut self, dir: &nfs_fh3, filename: &str) -> Result<nfs_fh3, nfsstat3> {
        use nfs3_types::nfs3::{LOOKUP3args, diropargs3};

        let result = self
            .client()
            .lookup(&LOOKUP3args {
                what: diropargs3 {
                    dir: dir.clone(),
                    name: filename.as_bytes().into(),
                },
            })
            .await
            .expect("failed to lookup a file");

        match result {
            Nfs3Result::Ok(ok) => Ok(ok.object),
            Nfs3Result::Err((status, _)) => Err(status),
        }
    }

    async fn just_getattr(&mut self, file: &nfs_fh3) -> Result<fattr3, nfsstat3> {
        use nfs3_types::nfs3::{GETATTR3args, Nfs3Result};

        let result = self
            .client()
            .getattr(&GETATTR3args {
                object: file.clone(),
            })
            .await
            .expect("failed to getattr");

        match result {
            Nfs3Result::Ok(ok) => Ok(ok.obj_attributes),
            Nfs3Result::Err((status, _)) => Err(status),
        }
    }

    /// Create a file on the NFS server with the given content
    async fn just_create(
        &mut self,
        dir: &nfs_fh3,
        filename: &str,
        content: &[u8],
    ) -> Result<nfs_fh3, nfsstat3> {
        use nfs3_types::nfs3::{
            CREATE3args, Nfs3Result, WRITE3args, createhow3, sattr3, stable_how,
        };

        let create_result = self
            .client()
            .create(&CREATE3args {
                where_: nfs3_types::nfs3::diropargs3 {
                    dir: dir.clone(),
                    name: filename.as_bytes().into(),
                },
                how: createhow3::UNCHECKED(sattr3::default()),
            })
            .await
            .expect("failed to create file");

        let file_handle = match create_result {
            Nfs3Result::Ok(ok) => ok.obj.unwrap(),
            Nfs3Result::Err((status, _)) => return Err(status),
        };

        if !content.is_empty() {
            let write_result = self
                .client()
                .write(&WRITE3args {
                    file: file_handle.clone(),
                    offset: 0,
                    count: content.len() as u32,
                    stable: stable_how::UNSTABLE,
                    data: Opaque::owned(content.to_vec()),
                })
                .await
                .expect("failed to write to file");

            match write_result {
                Nfs3Result::Ok(_) => {}
                Nfs3Result::Err((status, _)) => return Err(status),
            }
        }

        Ok(file_handle)
    }

    /// Create a directory on the NFS server
    async fn just_mkdir(&mut self, dir: &nfs_fh3, dirname: &str) -> Result<nfs_fh3, nfsstat3> {
        use nfs3_types::nfs3::{MKDIR3args, Nfs3Result, sattr3};

        let result = self
            .client()
            .mkdir(&MKDIR3args {
                where_: nfs3_types::nfs3::diropargs3 {
                    dir: dir.clone(),
                    name: dirname.as_bytes().into(),
                },
                attributes: sattr3::default(),
            })
            .await
            .expect("failed to mkdir");

        match result {
            Nfs3Result::Ok(ok) => Ok(ok.obj.unwrap()),
            Nfs3Result::Err((status, _)) => Err(status),
        }
    }

    /// Read the entire contents of a file from the NFS server
    async fn just_read(&mut self, file: &nfs_fh3) -> Result<Vec<u8>, nfsstat3> {
        use nfs3_types::nfs3::{Nfs3Result, READ3args};

        let mut offset = 0u64;
        let mut result = Vec::new();

        loop {
            let read_result = self
                .client()
                .read(&READ3args {
                    file: file.clone(),
                    offset,
                    count: 1024 * 1024,
                })
                .await
                .expect("failed to read file");

            match read_result {
                Nfs3Result::Ok(ok) => {
                    result.extend_from_slice(&ok.data.0);
                    if ok.eof || ok.count == 0 {
                        break;
                    }
                    offset += ok.count as u64;
                }
                Nfs3Result::Err((status, _)) => return Err(status),
            }
        }

        Ok(result)
    }
}

impl<T> JustClientExt for T where T: JustClient {}
