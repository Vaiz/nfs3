use std::fmt::Debug;

use nfs3_client::{MountClient, Nfs3Client, Nfs3Connection, PortmapperClient};

#[test]
fn client_debug_does_not_require_transport_debug() {
    struct Transport;

    const fn assert_debug<T: Debug>() {}

    assert_debug::<MountClient<Transport>>();
    assert_debug::<Nfs3Client<Transport>>();
    assert_debug::<Nfs3Connection<Transport>>();
    assert_debug::<PortmapperClient<Transport>>();
}
