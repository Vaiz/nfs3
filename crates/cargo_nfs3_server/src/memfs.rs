const CAT: &str = r"
    /\_____/\
   /  o   o  \
  ( ==  ^  == )
   )         (
  (           )
 ( (  )   (  ) )
(__(__)___(__)__)
";

const WRITABLE_README: &str = r"
This is in memory filesystem for NFSv3 server.
It contains a few files and directories for testing purposes.

WARNING: It stores data in memory, so it will be lost when the server is stopped.
         The total size of the filesystem is limited by the available memory.
";

const READONLY_README: &str = r"
This is in memory filesystem for NFSv3 server.
It contains a few files and directories for testing purposes.
It is read-only, so you cannot modify it.
";

pub fn default_config(readonly: bool) -> nfs3_server::memfs::MemFsConfig {
    let readme = if readonly {
        READONLY_README
    } else {
        WRITABLE_README
    };

    let mut config = nfs3_server::memfs::MemFsConfig::default();
    config.add_file("/README.txt", readme.as_bytes());
    config.add_file("/cat.txt", CAT.as_bytes());
    config.add_dir("/new folder");
    config.add_dir("/new folder (1)");
    config.add_dir("/new folder (2)");
    config
}
