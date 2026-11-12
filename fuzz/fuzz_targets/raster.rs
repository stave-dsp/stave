//! The rasterizer's coverage accumulation on any polygon — NaN, the
//! infinities and points far off the canvas included: no index outside
//! the mask, no panic, every pixel's coverage in [0, 1]. (Kani proves
//! the same for single edges on canvases up to 3×3; this reaches big
//! canvases and many-edged polygons.)
#![no_main]

use libfuzzer_sys::fuzz_target;
use stave_raster::Mask;
use stave_ui::scene::Pt;

fuzz_target!(|input: (u8, u8, Vec<(f64, f64)>)| {
    let (w, h, points) = input;
    let (cw, ch) = (usize::from(w % 65), usize::from(h % 65));
    let pts: Vec<Pt> = points.into_iter().take(64).map(|(x, y)| Pt { x, y }).collect();
    if pts.is_empty() {
        return;
    }
    let b = pts.iter().fold((f64::MAX, f64::MAX, f64::MIN, f64::MIN), |b, p| {
        (b.0.min(p.x), b.1.min(p.y), b.2.max(p.x), b.3.max(p.y))
    });
    let Some(mut mask) = Mask::new(b, cw, ch) else { return };
    mask.polygon(&pts);
    mask.finish();
    assert!(mask.cov.iter().all(|c| (0.0..=1.0).contains(c)));
});
