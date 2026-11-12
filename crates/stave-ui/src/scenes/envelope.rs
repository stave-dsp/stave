//! The ADSR display's picture — a pure function of four knob values
//! (an envelope picture needs NO engine data) — and its EDITOR: [`drag`] maps a grab + displacement to
//! one param's new value, segment by segment (grab the attack rise
//! and drag right = longer attack; grab the shelf and drag
//! vertically = sustain; time segments move EXPONENTIALLY, 60 px per
//! doubling — the log-time feel every envelope editor converges on).
//! One derivation ([`breakpoints`]) serves the picture, the invariants,
//! and the drag geometry.
//!
//! Geometry: the time axis gives attack/decay/release each a width
//! proportional to log(1 + t/REF) — log because a 2 s release must
//! not crush a 5 ms attack invisible — plus a fixed sustain shelf.
//! Segments draw as quadratic béziers bowed toward their exponential
//! shape (attack convex-up toward the peak, decay/release sagging
//! toward their targets), which is what one-pole segments look like.

use crate::scene::{pt, Path, Pt, Scene, Stroke};
use crate::style::Color;

#[derive(Copy, Clone, Debug)]
pub struct EnvStyle {
    /// The curve's line of light.
    pub line: Color,
    /// The filled area under the curve (usually line at low alpha).
    pub fill: Color,
    /// The baseline + frame hairline.
    pub axis: Color,
}

/// Knob values in their DECLARED units: milliseconds and 0..1.
#[derive(Copy, Clone, Debug)]
pub struct EnvState {
    pub attack_ms: f32,
    pub decay_ms: f32,
    pub sustain: f32,
    pub release_ms: f32,
    /// The drawing rect (widget-local).
    pub origin: Pt,
    pub w: f64,
    pub h: f64,
}

/// Log-compressed segment width (docs above). REF = 30 ms reads
/// short plucks and long pads on one axis.
fn seg_w(ms: f32) -> f64 {
    (1.0 + f64::from(ms) / 30.0).ln()
}

/// The sustain shelf's share of the width — fixed, so time segments
/// stretch around a stable landmark.
const SHELF_FRAC: f64 = 0.22;

/// The x-positions of the segment boundaries, 0..1 across the
/// display: (attack peak, decay end / shelf start, shelf end /
/// release start) — release fills the remainder to 1. Exported for
/// the invariants AND the editor's drag handles — one derivation for
/// the picture, the tests, and the interaction.
pub fn breakpoints(a_ms: f32, d_ms: f32, r_ms: f32) -> (f64, f64, f64) {
    let (wa, wd, wr) = (seg_w(a_ms), seg_w(d_ms), seg_w(r_ms));
    let time_total = (wa + wd + wr).max(1e-9);
    let scale = (1.0 - SHELF_FRAC) / time_total;
    let x_peak = wa * scale;
    let x_shelf = x_peak + wd * scale;
    let x_release = x_shelf + SHELF_FRAC;
    (x_peak, x_shelf, x_release)
}

/// The whole picture: filled area + the curve + the baseline.
pub fn scene(st: &EnvState, style: &EnvStyle) -> Scene {
    let mut sc = Scene::new();
    let (x0, y0, w, h) = (st.origin.x, st.origin.y, st.w, st.h);
    let sus = f64::from(st.sustain.clamp(0.0, 1.0));
    let (bx_peak, bx_shelf, bx_rel) = breakpoints(st.attack_ms, st.decay_ms, st.release_ms);

    // Display space: y grows DOWN from the top (widget-local, the
    // scene renderer draws in the view's flipped coords).
    let x = |t: f64| x0 + t * w;
    let y = |level: f64| y0 + (1.0 - level) * (h - 2.0) + 1.0;

    let start = pt(x(0.0), y(0.0));
    let peak = pt(x(bx_peak), y(1.0));
    let shelf_l = pt(x(bx_shelf), y(sus));
    let shelf_r = pt(x(bx_rel), y(sus));
    let end = pt(x(1.0), y(0.0));

    // One-pole bows as quadratics: attack pulls toward its target
    // fast then eases in (control point up-front); decay/release
    // drop fast then drift (control point early + low).
    let curve = Path::new()
        .move_to(start)
        .quad_to(pt(start.x + (peak.x - start.x) * 0.35, peak.y), peak)
        .quad_to(pt(peak.x + (shelf_l.x - peak.x) * 0.35, shelf_l.y), shelf_l)
        .line_to(shelf_r)
        .quad_to(pt(shelf_r.x + (end.x - shelf_r.x) * 0.35, end.y), end);

    // The filled area closes along the baseline.
    let area = curve
        .clone()
        .line_to(pt(x(1.0), y(0.0)))
        .line_to(pt(x(0.0), y(0.0)))
        .close();
    sc.solid(area, style.fill);
    sc.stroke(
        Path::polyline(&[pt(x(0.0), y(0.0)), pt(x(1.0), y(0.0))]),
        Stroke::new(style.axis, 1.0),
    );
    sc.stroke(curve, Stroke::round(style.line, 1.6));
    // The handle dots: the three breakpoints.
    for p in [peak, shelf_l, shelf_r] {
        sc.solid(Path::circle(p, 3.0), style.line);
    }
    sc
}

/// The editor half: which param a drag moves, and to where.
/// The SEGMENT grabbed decides the param — no fiddly handle
/// targets: anywhere in the attack rise drags attack, the shelf
/// drags sustain (vertical, absolute), the decay and release falls
/// drag their times. Times move exponentially (60 px/doubling).
pub fn drag(inp: &crate::scene::SceneInputs<'_>, from: Pt, to: Pt) -> Option<(usize, f64)> {
    let (a, d, s, r) = (inp.params[0], inp.params[1], inp.params[2], inp.params[3]);
    let _ = s;
    let (bx_peak, bx_shelf, bx_rel) = breakpoints(a as f32, d as f32, r as f32);
    let t0 = ((from.x - 1.0) / (inp.w - 2.0).max(1.0)).clamp(0.0, 1.0);
    let dx = to.x - from.x;
    let clamp = |i: usize, v: f64| {
        let (lo, hi) = inp.ranges[i];
        Some((i, v.clamp(lo, hi)))
    };
    let scale = (dx / 60.0).exp2();
    if t0 < bx_peak {
        clamp(0, a * scale)
    } else if t0 < bx_shelf {
        clamp(1, d * scale)
    } else if t0 < bx_rel {
        // The shelf: sustain from the ABSOLUTE y (y-down space).
        let level = 1.0 - ((to.y - 1.0) / (inp.h - 2.0).max(1.0));
        clamp(2, level)
    } else {
        clamp(3, r * scale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Op, Seg};

    fn style() -> EnvStyle {
        EnvStyle {
            line: Color::rgba(0.5, 0.7, 1.0, 1.0),
            fill: Color::rgba(0.5, 0.7, 1.0, 0.15),
            axis: Color::rgba(0.3, 0.3, 0.35, 1.0),
        }
    }

    fn state(a: f32, d: f32, s: f32, r: f32) -> EnvState {
        EnvState {
            attack_ms: a,
            decay_ms: d,
            sustain: s,
            release_ms: r,
            origin: pt(0.0, 0.0),
            w: 200.0,
            h: 80.0,
        }
    }

    /// The curve is the 1.6-width stroke; its first quad's endpoint
    /// is the attack peak.
    fn curve_of(sc: &Scene) -> &crate::scene::Path {
        sc.ops
            .iter()
            .find_map(|op| match op {
                Op::Stroke { path, stroke } if (stroke.width - 1.6).abs() < 1e-9 => Some(path),
                _ => None,
            })
            .expect("curve present")
    }

    fn peak_x(sc: &Scene) -> f64 {
        match curve_of(sc).segs[1] {
            Seg::QuadTo(_, p) => p.x,
            _ => panic!("attack is a quad"),
        }
    }

    /// THE invariant this display exists for: the attack knob moves the
    /// envelope path's first control point (the
    /// scene-tree check promised in the proposal, delivered).
    #[test]
    fn the_attack_knob_moves_the_first_breakpoint() {
        let fast = scene(&state(1.0, 200.0, 0.6, 300.0), &style());
        let slow = scene(&state(500.0, 200.0, 0.6, 300.0), &style());
        assert!(
            peak_x(&slow) > peak_x(&fast) + 10.0,
            "attack 1 ms → peak {:.1}; 500 ms → {:.1}",
            peak_x(&fast),
            peak_x(&slow)
        );
    }

    /// Sustain sets the shelf height, exactly.
    #[test]
    fn sustain_sets_the_shelf() {
        for s in [0.2f32, 0.8] {
            let sc = scene(&state(5.0, 100.0, s, 200.0), &style());
            let Seg::LineTo(shelf_r) = curve_of(&sc).segs[3] else {
                panic!("shelf is a line")
            };
            let expect = (1.0 - f64::from(s)) * 78.0 + 1.0;
            assert!(
                (shelf_r.y - expect).abs() < 1e-9,
                "sustain {s}: shelf y {} vs {expect}",
                shelf_r.y
            );
        }
    }

    /// Breakpoints are ordered and span the display for any knobs.
    #[test]
    fn breakpoints_are_sane_across_the_ranges() {
        for a in [0.5f32, 2000.0] {
            for d in [1.0f32, 4000.0] {
                for r in [2.0f32, 8000.0] {
                    let (p, s, e) = breakpoints(a, d, r);
                    assert!(
                        0.0 < p && p < s && s < e && e < 1.0,
                        "a={a} d={d} r={r}: {p} {s} {e}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_scene_is_pure() {
        let st = state(5.0, 120.0, 0.6, 200.0);
        assert_eq!(scene(&st, &style()), scene(&st, &style()));
    }

    /// THE GESTURE-REPLAY INVARIANT: drags land params exactly where
    /// the geometry says — 60 px right in a time segment DOUBLES its
    /// value; the shelf's y IS the sustain; everything clamps to the
    /// DECLARED range.
    #[test]
    fn drags_land_where_the_handles_went() {
        use crate::scene::{SceneInputs, ScenePalette};
        let params = [10.0f64, 200.0, 0.6, 400.0];
        let ranges = [(0.5, 2_000.0), (1.0, 4_000.0), (0.0, 1.0), (2.0, 8_000.0)];
        let palette = ScenePalette {
            accent: Color::rgba(1.0, 1.0, 1.0, 1.0),
            ink_hi: Color::rgba(1.0, 1.0, 1.0, 1.0),
            ink_muted: Color::rgba(0.5, 0.5, 0.5, 1.0),
            axis: Color::rgba(0.3, 0.3, 0.3, 1.0),
            bg: Color::rgba(0.0, 0.0, 0.0, 1.0),
        };
        let inp = SceneInputs {
            params: &params,
            ranges: &ranges,
            probes: &[],
            w: 202.0,
            h: 82.0,
            palette,
        };
        let (bx_peak, bx_shelf, bx_rel) =
            breakpoints(params[0] as f32, params[1] as f32, params[3] as f32);
        let _ = bx_shelf;
        let x_of = |t: f64| 1.0 + t * 200.0;
        // Attack segment, 60 px right → attack doubles.
        let grab = pt(x_of(bx_peak * 0.5), 40.0);
        let (idx, v) = drag(&inp, grab, pt(grab.x + 60.0, 40.0)).unwrap();
        assert_eq!(idx, 0);
        assert!((v - 20.0).abs() < 1e-9, "attack 10 → {v}");
        // Shelf, dragged to 25% height → sustain 0.75.
        let shelf_x = x_of((bx_shelf + bx_rel) * 0.5);
        let (idx, v) = drag(&inp, pt(shelf_x, 40.0), pt(shelf_x, 1.0 + 0.25 * 80.0)).unwrap();
        assert_eq!(idx, 2);
        assert!((v - 0.75).abs() < 1e-9, "sustain → {v}");
        // Release segment, 120 px LEFT → release quarters, clamped
        // to the declared floor if it undershoots.
        let rel_x = x_of((bx_rel + 1.0) * 0.5);
        let (idx, v) = drag(&inp, pt(rel_x, 40.0), pt(rel_x - 120.0, 40.0)).unwrap();
        assert_eq!(idx, 3);
        assert!((v - 100.0).abs() < 1e-9, "release 400 → {v}");
        // Clamp invariant: a huge rightward attack drag pins at 2000.
        let (_, v) = drag(&inp, grab, pt(grab.x + 6_000.0, 40.0)).unwrap();
        assert!((v - 2_000.0).abs() < 1e-9, "clamped to declaration: {v}");
    }
}
