//! The rasterizer on any polygon — NaN, the infinities and points far off
//! the canvas included — through the public path (a Scene fill rendered
//! onto a Canvas): no index outside the coverage mask or the canvas, no
//! panic. (Kani proves single edges on canvases up to 2×2; this reaches
//! big canvases and many-edged polygons.)
#![no_main]

use libfuzzer_sys::fuzz_target;
use stave_raster::{render, Canvas, NoText};
use stave_ui::scene::{Path, Pt, Scene};
use stave_ui::Color;

fuzz_target!(|input: (u8, u8, f64, Vec<(f64, f64)>)| {
    let (w, h, scale, points) = input;
    let pts: Vec<Pt> = points.into_iter().take(64).map(|(x, y)| Pt { x, y }).collect();
    if pts.is_empty() {
        return;
    }
    let mut scene = Scene::new();
    scene.solid(Path::polyline(&pts).close(), Color::rgba(1.0, 0.5, 0.25, 0.75));
    let mut canvas = Canvas::new(usize::from(w % 65), usize::from(h % 65));
    // Any scale a host could hand over, NaN and zero included.
    render(&scene, if scale.is_finite() { scale.clamp(-8.0, 8.0) } else { scale }, &mut canvas, &mut NoText);
});
