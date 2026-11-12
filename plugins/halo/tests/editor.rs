use stave_test::editor::{endpoints_agree, owned_tree_resolves};

#[test]
fn tree_resolves() {
    owned_tree_resolves::<stave_halo_vst3::Halo>("halo");
}

#[test]
fn widgets_agree_with_declared_endpoints() {
    endpoints_agree::<stave_halo_vst3::Halo>("halo");
}
