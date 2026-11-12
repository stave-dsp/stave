//! The portable software renderer draws this face as CoreGraphics does
//! (stave_test::raster). Writes build/raster-parity/revenant-*.ppm.

#[cfg(target_os = "macos")]
#[test]
fn the_software_renderer_matches_coregraphics() {
    for p in stave_test::raster::parity::<stave_revenant_vst3::Revenant>("revenant") {
        // Measured 2026-11-10: mean ≤ 0.02 of 255, ≤ 0.003% of pixels off
        // by more than 24 (anti-aliasing at edges: exact area vs CG's).
        assert!(p.with_text.mean < 0.1 && p.with_text.off < 0.0002, "revenant: {p:?}");
    }
}
