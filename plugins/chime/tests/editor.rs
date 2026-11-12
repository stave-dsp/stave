//! Owned-editor bindings: the tree resolves against the table; widgets
//! agree with declared endpoints.

use stave_test::editor::{endpoints_agree, owned_tree_resolves};

#[test]
fn tree_resolves() {
    owned_tree_resolves::<stave_chime_vst3::Chime>("chime");
}

#[test]
fn widgets_agree_with_declared_endpoints() {
    endpoints_agree::<stave_chime_vst3::Chime>("chime");
}
