//! Owned-editor bindings (colocated): the tree resolves against the
//! table; every widget agrees with its param's declared endpoint.

use stave_test::editor::{endpoints_agree, owned_tree_resolves};

#[test]
fn tree_resolves() {
    owned_tree_resolves::<stave_forge_vst3::Forge>("forge");
}

#[test]
fn widgets_agree_with_declared_endpoints() {
    endpoints_agree::<stave_forge_vst3::Forge>("forge");
}

/// Forge: pad axes + power + selectors + rails present and bound.
#[test]
fn tree_binds_its_chrome() {
    let named = owned_tree_resolves::<stave_forge_vst3::Forge>("forge");
    let has = |k: &str, n: &str| named.iter().any(|(kk, nn)| *kk == k && nn == n);
    assert!(has("power", "bypass"));
    assert!(has("pad", "bias") && has("pad", "drive"));
    assert!(has("selector", "character") && has("selector", "stack"));
    assert!(has("knob", "input") && has("knob", "output"));
}
