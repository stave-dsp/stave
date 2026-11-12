#[test]
fn passes_validation() {
    stave_test::validation::assert_passes(stave_host::entry_points!(stave_opal_vst3));
}
