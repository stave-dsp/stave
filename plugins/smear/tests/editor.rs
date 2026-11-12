//! Owned-editor bindings (colocated): the tree resolves against the
//! table; every widget agrees with its param's declared endpoint.

use stave_test::editor::{endpoints_agree, owned_tree_resolves};

#[test]
fn tree_resolves() {
    owned_tree_resolves::<stave_smear_vst3::Smear>("smear");
}

#[test]
fn widgets_agree_with_declared_endpoints() {
    endpoints_agree::<stave_smear_vst3::Smear>("smear");
}
