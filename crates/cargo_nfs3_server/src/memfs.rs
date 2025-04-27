fn default_config() -> nfs3_server::memfs::MemFsConfig {
    const CAT: &str = r"
    /\_____/\
   /  o   o  \
  ( ==  ^  == )
   )         (
  (           )
 ( (  )   (  ) )
(__(__)___(__)__)
";

    let mut config = nfs3_server::memfs::MemFsConfig::default();
    config.add_file("/a.txt", b"hello world\n");
    config.add_file("/b.txt", b"Greetings\n");
    config.add_file("/cat.txt", CAT.as_bytes());
    config.add_dir("/a directory");
    for i in 0..10 {
        config.add_file(&format!("/a directory/{i}.txt"), i.to_string().as_bytes());
    }
    config
}
