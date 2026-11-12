//! Renderer parity: every scene widget in a plugin's editor rendered
//! through CoreGraphics (the reference) and through the portable
//! software renderer (`stave-raster` + CoreText text), compared pixel by
//! pixel — so the renderer that ships on every platform is proven to
//! draw what the Mac drew. Two probe states per scene: idle (all probes
//! 0) and lit (all 0.6), so glows, meters and moving parts are drawn.
//!
//! Writes `build/raster-parity/<plugin>-<state>-{cg,raster,diff}.ppm`
//! for eyes; returns the measured differences for the caller to bound.

use stave_core::ui::{Dim, Kind};
use stave_core::{Plugin, UiNode, Widget};
use stave_ui::scene::{SceneInputs, ScenePalette};
use stave_ui::style::Color;

/// The measured difference between the two renders of one scene —
/// SHAPES (every op but text: the rasteriser's own work) and the whole
/// picture WITH text (text is CoreText either way, but drawn through a
/// scaled context in one and at device size in the other, so hinting
/// shifts glyph edges by fractions of a pixel).
#[derive(Debug, Clone, Copy)]
pub struct Parity {
    pub shapes: Diff,
    pub with_text: Diff,
}

#[derive(Debug, Clone, Copy)]
pub struct Diff {
    /// Mean absolute difference over every channel of every pixel (0..255).
    pub mean: f64,
    /// Share of pixels whose largest channel difference exceeds 24.
    pub off: f64,
    /// The largest channel difference anywhere.
    pub max: u8,
}

fn scenes(n: &UiNode, out: &mut Vec<(Widget, f64, f64)>) {
    if let Kind::Widget(w @ Widget::Scene { .. }) = &n.kind {
        if let (Dim::Px(wd), Dim::Px(ht)) = (n.style.width, n.style.height) {
            out.push((w.clone(), wd, ht));
        }
    }
    for c in &n.children {
        scenes(c, out);
    }
}

fn write_ppm(path: &std::path::Path, px: &[u8], w: usize, h: usize) {
    let mut out = format!("P6 {w} {h} 255\n").into_bytes();
    for p in px.chunks(4) {
        out.extend_from_slice(&p[..3]);
    }
    let _ = std::fs::write(path, out);
}

#[allow(clippy::too_many_arguments)]
fn compare(scene: &stave_ui::scene::Scene, sw: f64, sh: f64, scale: f64, pw: usize, ph: usize, dir: &std::path::Path, stem: &str) -> Diff {
    let cg = stave_ui_cocoa::offscreen::render_cg(scene, sw, sh, scale);
    let raster = stave_ui_cocoa::offscreen::render_raster(scene, sw, sh, scale);
    assert_eq!(cg.len(), raster.len());
    let (mut sum, mut off, mut max) = (0u64, 0usize, 0u8);
    let mut diff = vec![0u8; cg.len()];
    for (i, (a, b)) in cg.chunks(4).zip(raster.chunks(4)).enumerate() {
        let mut m = 0u8;
        for c in 0..4 {
            let d = a[c].abs_diff(b[c]);
            sum += u64::from(d);
            m = m.max(d);
        }
        max = max.max(m);
        if m > 24 {
            off += 1;
        }
        let v = (u16::from(m) * 4).min(255) as u8;
        diff[i * 4..i * 4 + 4].copy_from_slice(&[v, v, v, 255]);
    }
    write_ppm(&dir.join(format!("{stem}-cg.ppm")), &cg, pw, ph);
    write_ppm(&dir.join(format!("{stem}-raster.ppm")), &raster, pw, ph);
    write_ppm(&dir.join(format!("{stem}-diff.ppm")), &diff, pw, ph);
    Diff { mean: sum as f64 / cg.len() as f64, off: off as f64 / (pw * ph) as f64, max }
}

/// Render every scene of `P`'s editor both ways and measure.
pub fn parity<P: Plugin>(name: &str) -> Vec<Parity> {
    let plugin = P::new();
    let table = plugin.param_table();
    let tree = plugin.editor_ui().unwrap_or_else(|| panic!("{name}: no editor tree"));
    let mut found = Vec::new();
    scenes(&tree, &mut found);
    assert!(!found.is_empty(), "{name}: no sized scene widgets");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../build/raster-parity");
    let _ = std::fs::create_dir_all(&dir);
    let scale = 2.0;
    let mut results = Vec::new();
    for (k, (w, sw, sh)) in found.iter().enumerate() {
        let Widget::Scene { params, probes, build, .. } = w else { continue };
        let ps: Vec<&dyn stave_core::params::AnyParam> =
            params.iter().map(|id| table.by_id(*id).unwrap_or_else(|| panic!("{name}: param {id:?}"))).collect();
        let values: Vec<f64> = ps.iter().map(|p| p.plain()).collect();
        let ranges: Vec<(f64, f64)> = ps.iter().map(|p| (p.normalized_to_plain(0.0), p.normalized_to_plain(1.0))).collect();
        for (state, level) in [("idle", 0.0), ("lit", 0.6)] {
            let pr = vec![level; probes.len()];
            let c = Color::rgba(0.5, 0.5, 0.5, 1.0);
            let scene = build(&SceneInputs {
                params: &values,
                ranges: &ranges,
                probes: &pr,
                w: *sw,
                h: *sh,
                palette: ScenePalette { accent: c, ink_hi: c, ink_muted: c, axis: c, bg: c },
            });
            let (pw, ph) = ((sw * scale).round() as usize, (sh * scale).round() as usize);
            let mut shapes_only = scene.clone();
            shapes_only.ops.retain(|op| !matches!(op, stave_ui::scene::Op::Text { .. }));
            let stem = format!("{name}-{k}-{state}");
            let shapes = compare(&shapes_only, *sw, *sh, scale, pw, ph, &dir, &format!("{stem}-shapes"));
            let with_text = compare(&scene, *sw, *sh, scale, pw, ph, &dir, &stem);
            let t = std::time::Instant::now();
            for _ in 0..5 {
                let _ = stave_ui_cocoa::offscreen::render_raster(&scene, *sw, *sh, scale);
            }
            let soft = t.elapsed().as_secs_f64() * 1000.0 / 5.0;
            let t = std::time::Instant::now();
            for _ in 0..5 {
                let _ = stave_ui_cocoa::offscreen::render_cg(&scene, *sw, *sh, scale);
            }
            let cg_ms = t.elapsed().as_secs_f64() * 1000.0 / 5.0;
            println!("{stem}: software frame {soft:.2} ms, CoreGraphics {cg_ms:.2} ms, at {pw}×{ph}");
            if std::env::var("STAVE_RASTER_PROFILE").is_ok() {
                use stave_ui::scene::Op;
                for (label, keep) in [
                    ("images", (|o: &Op| matches!(o, Op::Image { .. })) as fn(&Op) -> bool),
                    ("fills", |o: &Op| matches!(o, Op::Fill { .. })),
                    ("strokes", |o: &Op| matches!(o, Op::Stroke { .. })),
                    ("text", |o: &Op| matches!(o, Op::Text { .. })),
                ] {
                    let mut part = scene.clone();
                    part.ops.retain(keep);
                    let t = std::time::Instant::now();
                    for _ in 0..5 {
                        let _ = stave_ui_cocoa::offscreen::render_raster(&part, *sw, *sh, scale);
                    }
                    println!("  {label:<8} {:>3} ops  {:.2} ms", part.ops.len(), t.elapsed().as_secs_f64() * 1000.0 / 5.0);
                }
            }
            println!(
                "{stem}: shapes mean {:.3} off {:.3}% max {}  |  with text mean {:.3} off {:.3}% max {}",
                shapes.mean, 100.0 * shapes.off, shapes.max, with_text.mean, 100.0 * with_text.off, with_text.max
            );
            results.push(Parity { shapes, with_text });
        }
    }
    results
}
