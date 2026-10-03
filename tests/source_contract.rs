// Regression guards for the native changes that replace unsafe runtime shims.
#[test]
fn pinned_source_contains_decoded_reply_verifier_fix() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = std::fs::read_to_string(root.join("vendor/ntirpc/src/clnt_generic.c")).unwrap();
    let copy = source
        .find("cc->cc_verf = req->rq_msg.RPCM_ack.ar_verf;")
        .unwrap();
    assert!(
        source[copy..]
            .find("AUTH_VALIDATE(cc->cc_auth, &(cc->cc_verf))")
            .is_some()
    );
}

#[test]
fn gss_allocation_is_locally_zeroed_without_global_hook() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = std::fs::read_to_string(root.join("vendor/ntirpc/src/auth_gss.c")).unwrap();
    assert!(source.contains("struct rpc_gss_data *gd = mem_zalloc(sizeof(*gd));"));
    let helpers = std::fs::read_to_string(root.join("src/auth_helpers.c")).unwrap();
    assert!(!helpers.contains("TIRPC_PUT_PARAMETERS"));
    assert!(!helpers.contains("xp_ops"));
    assert!(!helpers.contains("rpc_dplx"));
}
