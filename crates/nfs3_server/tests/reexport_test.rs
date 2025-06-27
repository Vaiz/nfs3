// Test to verify that nfs3_types is properly re-exported from nfs3_server

#[test]
fn test_nfs3_types_reexport_from_server() {
    // Test creating types using the re-exported path from nfs3_server
    let _handle = nfs3_server::nfs3_types::nfs3::nfs_fh3::default();
    let _auth = nfs3_server::nfs3_types::rpc::opaque_auth::default();
    
    // Also test accessing modules
    let _void = nfs3_server::nfs3_types::xdr_codec::Void;
}