// Test to verify that nfs3_types is properly re-exported from nfs3_client

#[test]
fn test_nfs3_types_reexport_from_client() {
    // Test creating types using the re-exported path from nfs3_client
    let _handle = nfs3_client::nfs3_types::nfs3::nfs_fh3::default();
    let _auth = nfs3_client::nfs3_types::rpc::opaque_auth::default();

    // Also test accessing modules - just verify the path compiles
    let _ = std::mem::size_of::<nfs3_client::nfs3_types::xdr_codec::Void>();
}
