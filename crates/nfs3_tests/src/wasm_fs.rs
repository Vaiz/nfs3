#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use wasmer_vfs::{FileSystem, OpenOptionsConfig};

    #[test]
    fn test_perf() -> anyhow::Result<()> {
        let vfs = wasmer_vfs::mem_fs::FileSystem::default();
        let file_options = OpenOptionsConfig {
            read: true,
            write: true,
            create_new: true,
            append: false,
            truncate: false,
            create: false,
        };

        let start = std::time::Instant::now();
        for i in 0..1000 {
            let mut file = vfs
                .new_open_options()
                .options(file_options.clone())
                .open(format!("/file_{}", i))?;
            file.write_all(b"Hello, World!")?;
            file.flush()?;
        }
        let elapsed = start.elapsed();
        println!("Elapsed: {:?}", elapsed);

        let start = std::time::Instant::now();
        for i in 0..1000 {
            let path = PathBuf::from(format!("/dir_{i}"));
            vfs.create_dir(&path)?;
        }
        let elapsed = start.elapsed();
        println!("Elapsed: {:?}", elapsed);

        Ok(())
    }
}
