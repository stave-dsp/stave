//! The validation suite — the price of admission to plugins/.

#[test]
fn passes_validation() {
    stave_test::validation::assert_passes(stave_host::entry_points!(stave_revenant_vst3));
}
