//! Properties the rasterizer keeps for every shape (ledger T7): the ink a
//! fill lays down is its area, wherever it sits — under sub-pixel and
//! whole-pixel translation — and at any scale (area × scale²). A stroke's
//! ink is the area it covers, counted once (ledger T9).

use stave_raster::{render, Canvas, NoText};
use stave_ui::scene::{pt, Cap, Path, Pt, Scene, Stroke};
use stave_ui::style::Color;

/// Total coverage (alpha, in pixels) of `path` filled opaque white.
fn ink(path: Path, scale: f64, size: usize) -> f64 {
    let mut s = Scene::new();
    s.solid(path, Color::rgba(1.0, 1.0, 1.0, 1.0));
    ink_of(&s, scale, size)
}

/// Total coverage (alpha, in pixels) of `s`.
fn ink_of(s: &Scene, scale: f64, size: usize) -> f64 {
    let mut c = Canvas::new(size, size);
    render(s, scale, &mut c, &mut NoText);
    c.pixels().chunks_exact(4).map(|p| f64::from(p[3]) / 255.0).sum()
}

/// A small deterministic generator.
fn lcg(seed: &mut u64) -> f64 {
    *seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
    (*seed >> 11) as f64 / (1u64 << 53) as f64
}

fn triangle(a: Pt, b: Pt, c: Pt) -> Path {
    Path::new().move_to(a).line_to(b).line_to(c).close()
}

fn area(a: Pt, b: Pt, c: Pt) -> f64 {
    ((b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y)).abs() / 2.0
}

#[test]
#[cfg_attr(miri, ignore)]
fn a_fills_ink_is_its_area_under_any_translation() {
    let mut seed = 0x5EED;
    for _ in 0..200 {
        let mut p = || pt(4.0 + lcg(&mut seed) * 20.0, 4.0 + lcg(&mut seed) * 20.0);
        let (a, b, c) = (p(), p(), p());
        let want = area(a, b, c);
        let (dx, dy) = (lcg(&mut seed) * 6.0, lcg(&mut seed) * 6.0);
        let shift = |q: Pt| pt(q.x + dx, q.y + dy);
        let here = ink(triangle(a, b, c), 1.0, 36);
        let moved = ink(triangle(shift(a), shift(b), shift(c)), 1.0, 36);
        // 8-bit alpha rounds each pixel by up to half a count.
        let tolerance = 0.02 + 0.004 * (want.sqrt() * 4.0 + 4.0);
        assert!((here - want).abs() < tolerance, "ink {here} for area {want}");
        assert!((moved - here).abs() < tolerance, "translation by ({dx}, {dy}) changed the ink: {here} → {moved}");
    }
}

#[test]
#[cfg_attr(miri, ignore)]
fn ink_scales_with_the_square_of_the_scale() {
    let r = || Path::rect(pt(1.25, 2.5), 7.5, 3.75);
    let one = ink(r(), 1.0, 64);
    for scale in [2.0, 3.0, 1.5] {
        let s = ink(r(), scale, 64);
        assert!((s / (scale * scale) - one).abs() < 0.05, "at {scale}×: {s} vs {one}");
    }
    assert!((one - 7.5 * 3.75).abs() < 0.05);
}

#[test]
#[cfg_attr(miri, ignore)]
fn a_mitered_corner_inks_its_arms_once() {
    // Two butt-ended arms meeting in a miter cover width × (sum of the
    // arm lengths) at any turn: the miter's outside wedge and the inside
    // overlap have the same area. Pieces that overlap would ink more.
    // (Turns up to 120°, so the inside edges cross within the arms.)
    let mut seed = 0x7_0C0E;
    for _ in 0..100 {
        let (l1, l2) = (4.0 + lcg(&mut seed) * 10.0, 4.0 + lcg(&mut seed) * 10.0);
        let width = 1.0 + lcg(&mut seed) * 3.0;
        let heading = lcg(&mut seed) * core::f64::consts::TAU;
        let sign = if lcg(&mut seed) < 0.5 { -1.0 } else { 1.0 };
        let turn = (20.0 + lcg(&mut seed) * 100.0).to_radians() * sign;
        let v = pt(20.0 + lcg(&mut seed), 20.0 + lcg(&mut seed));
        let a = pt(v.x - l1 * heading.cos(), v.y - l1 * heading.sin());
        let b = pt(v.x + l2 * (heading + turn).cos(), v.y + l2 * (heading + turn).sin());
        let mut s = Scene::new();
        s.stroke(Path::new().move_to(a).line_to(v).line_to(b), Stroke::new(Color::rgba(1.0, 1.0, 1.0, 1.0), width));
        let (got, want) = (ink_of(&s, 1.0, 40), width * (l1 + l2));
        assert!((got - want).abs() < 0.15, "ink {got} for area {want} (turn {turn})");
    }
}

#[test]
#[cfg_attr(miri, ignore)]
fn a_capped_line_inks_its_body_and_caps_once() {
    let mut seed = 0xCA9;
    // (Round caps are whole discs over the ends, which still overlap the
    // line's end in edge pixels; ledger T9.)
    for cap in [Cap::Butt, Cap::Square] {
        for _ in 0..40 {
            let len = 3.0 + lcg(&mut seed) * 12.0;
            let width = 1.0 + lcg(&mut seed) * 4.0;
            let heading = lcg(&mut seed) * core::f64::consts::TAU;
            let a = pt(20.0 + lcg(&mut seed), 20.0 + lcg(&mut seed));
            let b = pt(a.x + len * heading.cos(), a.y + len * heading.sin());
            let mut s = Scene::new();
            let stroke = Stroke { cap, ..Stroke::new(Color::rgba(1.0, 1.0, 1.0, 1.0), width) };
            s.stroke(Path::new().move_to(a).line_to(b), stroke);
            let got = ink_of(&s, 1.0, 40);
            let want = width * len + if cap == Cap::Square { width * width } else { 0.0 };
            assert!((got - want).abs() < 0.15, "{cap:?}: ink {got}, want {want}");
        }
    }
}
