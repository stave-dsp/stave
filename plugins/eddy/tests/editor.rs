//! Owned-editor bindings: the tree resolves against the table; widgets
//! agree with declared endpoints.

use stave_test::editor::{endpoints_agree, owned_tree_resolves};

#[test]
fn tree_resolves() {
    owned_tree_resolves::<stave_eddy_vst3::Eddy>("eddy");
}

#[test]
fn widgets_agree_with_declared_endpoints() {
    endpoints_agree::<stave_eddy_vst3::Eddy>("eddy");
}
