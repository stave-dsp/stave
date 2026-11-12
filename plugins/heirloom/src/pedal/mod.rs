//! The pedal — heirloom's whole face as ONE scene: a software-lit
//! enclosure bake (render.rs, memoized at 2×) under live vector
//! parts (knob indicators, silkscreen text), toggle / LED sprites,
//! and the record's label, re-shaded each paint at the engine's
//! platter angle (the `spin` probe).
//!
//! Geometry lives here once; the bake, the picture and the hit-map
//! all read it, so what you see is what you grab.
//!
//! The scene's declared params, in order (editor.rs declares them):
//! `[time, repeats, mix, wobble, rpm, age, machine, warp, media,
//!   bypass, hold]`.

mod render;
mod shade;

use stave_ui::scene::{pt, Op, Path, Pt, Scene, SceneInputs, Stroke};
use stave_ui::style::Color;

/// The window, in points.
pub const W: f64 = 520.0;
pub const H: f64 = 700.0;
/// The enclosure: x, y, w, h, corner radius.
const PEDAL: (f64, f64, f64, f64, f64) = (50.0, 50.0, 420.0, 600.0, 20.0);

/// Knob centres (row 1 then row 2), skirt and cap radii, scale ring.
const KNOBS: [(f64, f64); 6] =
    [(135.0, 178.0), (260.0, 178.0), (385.0, 178.0), (135.0, 400.0), (260.0, 400.0), (385.0, 400.0)];
const KNOB_R: f64 = 27.0;
const CAP_R: f64 = 19.5;
const SCALE_R: f64 = 37.0;
const KNOB_LABELS: [&str; 6] = ["TIME", "REPEATS", "MIX", "WOBBLE", "RPM", "AGE"];
/// Which knobs turn on a log taper (their declared ranges are log).
const KNOB_LOG: [bool; 6] = [true, false, false, false, true, false];

/// Three-way toggles: centres, names, position words (up/mid/down).
const TOGGLES: [(f64, f64); 3] = [(135.0, 292.0), (260.0, 292.0), (385.0, 292.0)];
const TOGGLE_NAMES: [&str; 3] = ["MACHINE", "WARP", "MEDIA"];
const TOGGLE_WORDS: [[&str; 3]; 3] =
    [["REVERSE", "OCTAVE", "BLOOM"], ["SINE", "WARP", "DRIFT"], ["VINYL", "TAPE", "VHS"]];
/// Lever throw from centre, points (sprite geometry agrees).
const TOGGLE_HIT: (f64, f64) = (20.0, 28.0);

/// Footswitches + their LEDs: (switch centre, LED centre).
const FOOT: [((f64, f64), (f64, f64)); 2] = [((150.0, 588.0), (150.0, 528.0)), ((370.0, 588.0), (370.0, 528.0))];
const FOOT_R: f64 = 24.0;
const FOOT_LABELS: [&str; 2] = ["BYPASS", "HOLD"];

/// The record graphic: centre, radius, label radius.
const RECORD: (f64, f64, f64, f64) = (260.0, 680.0, 215.0, 80.0);

/// Param slots in the scene's declaration order.
const P_MACHINE: usize = 6;
const P_BYPASS: usize = 9;
const P_HOLD: usize = 10;

/// Silkscreen inks.
const INK: Color = Color::rgba(0.16, 0.14, 0.12, 1.0);
const CREAM_INK: Color = Color::rgba(0.93, 0.89, 0.80, 0.92);
const ENGRAVE: Color = Color::rgba(0.07, 0.07, 0.08, 0.92);
const SKIRT_DOT: Color = Color::rgba(0.96, 0.95, 0.92, 1.0);

/// A knob's travel: 7:30 → 4:30, clockwise, in y-down screen radians.
fn knob_angle(v: f64) -> f64 {
    (135.0 + 270.0 * v.clamp(0.0, 1.0)).to_radians()
}

fn norm_of(plain: f64, (lo, hi): (f64, f64), log: bool) -> f64 {
    if log {
        (plain / lo).ln() / (hi / lo).ln()
    } else {
        (plain - lo) / (hi - lo)
    }
}

fn plain_of(norm: f64, (lo, hi): (f64, f64), log: bool) -> f64 {
    let n = norm.clamp(0.0, 1.0);
    if log {
        lo * (hi / lo).powf(n)
    } else {
        lo + (hi - lo) * n
    }
}

fn text(sc: &mut Scene, at: Pt, s: &str, size: f64, color: Color) {
    sc.ops.push(Op::Text { at, text: s.into(), size, color });
}

/// The picture: bake, then the live parts in paint order.
pub fn build(inp: &SceneInputs<'_>) -> Scene {
    let mut sc = Scene::new();
    let art = render::art();
    sc.ops.push(Op::Image { at: pt(0.0, 0.0), w: W, h: H, image: art.face.clone() });

    // The record's label, turned to the engine's platter angle.
    let spin = inp.probes.first().copied().filter(|a| a.is_finite()).unwrap_or(0.0);
    let (lx, ly, lw, lh) = render::label_rect();
    sc.ops.push(Op::Image { at: pt(lx, ly), w: lw, h: lh, image: render::label(spin) });

    // Title + tagline, silkscreened.
    text(&mut sc, pt(260.0, 100.0), "H E I R L O O M", 25.0, INK);
    text(&mut sc, pt(260.0, 117.0), "reverse machines  ·  played off worn media", 7.5, Color::rgba(0.42, 0.24, 0.15, 1.0));

    // Knobs: the rotating indicator (an engraved line on the spun
    // cap, a painted dot on the skirt) + labels.
    for (i, &(cx, cy)) in KNOBS.iter().enumerate() {
        let v = norm_of(inp.params[i], inp.ranges[i], KNOB_LOG[i]);
        let a = knob_angle(v);
        let (dx, dy) = (a.cos(), a.sin());
        let c = pt(cx, cy);
        sc.stroke(
            Path::new().move_to(pt(cx + dx * 6.0, cy + dy * 6.0)).line_to(pt(cx + dx * (CAP_R - 1.5), cy + dy * (CAP_R - 1.5))),
            Stroke::round(ENGRAVE, 2.2),
        );
        // The engraving's lit lower lip.
        sc.stroke(
            Path::new()
                .move_to(pt(cx + dx * 6.0 + 0.6, cy + dy * 6.0 + 0.8))
                .line_to(pt(cx + dx * (CAP_R - 1.5) + 0.6, cy + dy * (CAP_R - 1.5) + 0.8)),
            Stroke::round(Color::rgba(1.0, 1.0, 1.0, 0.22), 0.8),
        );
        sc.solid(Path::circle(pt(c.x + dx * 23.5, c.y + dy * 23.5), 1.7), SKIRT_DOT);
        text(&mut sc, pt(cx, cy + KNOB_R + 24.0), KNOB_LABELS[i], 10.0, INK);
    }

    // Toggles: the lever sprite for the current position + words.
    for (t, &(cx, cy)) in TOGGLES.iter().enumerate() {
        let pos = (inp.params[P_MACHINE + t].round() as usize).min(2);
        let (sw, sh) = render::TOGGLE_SPRITE;
        sc.ops.push(Op::Image { at: pt(cx - sw / 2.0, cy - sh / 2.0), w: sw, h: sh, image: art.toggle[pos].clone() });
        text(&mut sc, pt(cx, cy - 34.0), TOGGLE_NAMES[t], 9.0, INK);
        for (k, word) in TOGGLE_WORDS[t].iter().enumerate() {
            let lit = k == pos;
            let col = if lit { Color::rgba(0.62, 0.27, 0.13, 1.0) } else { Color::rgba(0.16, 0.14, 0.12, 0.55) };
            let x = cx + 17.0 + 2.45 * word.len() as f64;
            text(&mut sc, pt(x, cy - 11.0 + 13.0 * k as f64), word, 7.0, col);
        }
    }

    // LEDs: lit = the glow sprite over the baked dark lens.
    let lit = [inp.params[P_BYPASS] < 0.5, inp.params[P_HOLD] > 0.5];
    let (gw, gh) = render::GLOW_SPRITE;
    for (k, &((fx, _), (lx, ly))) in FOOT.iter().enumerate() {
        if lit[k] {
            sc.ops.push(Op::Image { at: pt(lx - gw / 2.0, ly - gh / 2.0), w: gw, h: gh, image: art.glow[k].clone() });
        }
        text(&mut sc, pt(fx, ly + 23.0), FOOT_LABELS[k], 7.5, CREAM_INK);
    }
    sc
}

/// The hit-map: knobs drag vertically (200 pt = full travel), a
/// toggle click steps the lever toward the click, a footswitch click
/// flips its latch. Pure: the grab point decides the target.
pub fn drag(inp: &SceneInputs<'_>, from: Pt, now: Pt) -> Option<(usize, f64)> {
    for (i, &(cx, cy)) in KNOBS.iter().enumerate() {
        if (from.x - cx).hypot(from.y - cy) <= KNOB_R + 6.0 {
            let v = norm_of(inp.params[i], inp.ranges[i], KNOB_LOG[i]);
            let moved = v + (from.y - now.y) / 200.0 + (now.x - from.x) / 400.0;
            return Some((i, plain_of(moved, inp.ranges[i], KNOB_LOG[i])));
        }
    }
    for (t, &(cx, cy)) in TOGGLES.iter().enumerate() {
        if (from.x - cx).abs() <= TOGGLE_HIT.0 && (from.y - cy).abs() <= TOGGLE_HIT.1 {
            let slot = P_MACHINE + t;
            let cur = inp.params[slot].round();
            let step = if from.y < cy { -1.0 } else { 1.0 };
            return Some((slot, (cur + step).clamp(0.0, 2.0)));
        }
    }
    for (k, &((fx, fy), _)) in FOOT.iter().enumerate() {
        if (from.x - fx).hypot(from.y - fy) <= FOOT_R + 4.0 {
            let slot = if k == 0 { P_BYPASS } else { P_HOLD };
            return Some((slot, if inp.params[slot] > 0.5 { 0.0 } else { 1.0 }));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use stave_ui::scene::ScenePalette;

    fn inputs<'a>(params: &'a [f64], ranges: &'a [(f64, f64)]) -> SceneInputs<'a> {
        let c = Color::rgba(0.0, 0.0, 0.0, 1.0);
        SceneInputs {
            params,
            ranges,
            probes: &[],
            w: W,
            h: H,
            palette: ScenePalette { accent: c, ink_hi: c, ink_muted: c, axis: c, bg: c },
        }
    }

    const RANGES: [(f64, f64); 11] = [
        (50.0, 1200.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.1, 8.0), (0.0, 1.0),
        (0.0, 2.0), (0.0, 2.0), (0.0, 2.0), (0.0, 1.0), (0.0, 1.0),
    ];
    const PARAMS: [f64; 11] = [450.0, 0.5, 0.45, 0.3, 0.8, 0.25, 1.0, 0.0, 2.0, 0.0, 0.0];

    #[test]
    fn dragging_up_turns_a_knob_up_and_the_log_taper_round_trips() {
        let inp = inputs(&PARAMS, &RANGES);
        let (cx, cy) = KNOBS[0];
        let (i, v) = drag(&inp, pt(cx, cy), pt(cx, cy - 40.0)).unwrap();
        assert_eq!(i, 0);
        assert!(v > 450.0);
        let back = plain_of(norm_of(450.0, RANGES[0], true), RANGES[0], true);
        assert!((back - 450.0).abs() < 1e-9);
    }

    #[test]
    fn a_click_steps_the_toggle_toward_it_and_clamps() {
        let inp = inputs(&PARAMS, &RANGES);
        let (cx, cy) = TOGGLES[0];
        assert_eq!(drag(&inp, pt(cx, cy - 10.0), pt(cx, cy - 10.0)), Some((6, 0.0)));
        assert_eq!(drag(&inp, pt(cx, cy + 10.0), pt(cx, cy + 10.0)), Some((6, 2.0)));
        let (cx, cy) = TOGGLES[2];
        assert_eq!(drag(&inp, pt(cx, cy + 10.0), pt(cx, cy + 10.0)), Some((8, 2.0)));
    }

    #[test]
    fn footswitches_flip_their_latch() {
        let inp = inputs(&PARAMS, &RANGES);
        let ((fx, fy), _) = FOOT[0];
        assert_eq!(drag(&inp, pt(fx, fy), pt(fx, fy)), Some((P_BYPASS, 1.0)));
        let ((fx, fy), _) = FOOT[1];
        assert_eq!(drag(&inp, pt(fx, fy), pt(fx, fy)), Some((P_HOLD, 1.0)));
    }

    #[test]
    fn the_indicator_follows_the_knob() {
        let lo = build(&inputs(&[50.0, 0.5, 0.45, 0.3, 0.8, 0.25, 1.0, 0.0, 2.0, 0.0, 0.0], &RANGES));
        let hi = build(&inputs(&[1200.0, 0.5, 0.45, 0.3, 0.8, 0.25, 1.0, 0.0, 2.0, 0.0, 0.0], &RANGES));
        let first_stroke = |s: &Scene| {
            s.ops.iter().find_map(|op| match op {
                Op::Stroke { path, .. } => Some(path.segs.clone()),
                _ => None,
            })
        };
        assert_ne!(first_stroke(&lo), first_stroke(&hi));
    }

    #[test]
    fn empty_space_grabs_nothing() {
        let inp = inputs(&PARAMS, &RANGES);
        assert_eq!(drag(&inp, pt(10.0, 10.0), pt(10.0, 40.0)), None);
    }
}

#[cfg(test)]
mod preview {
    /// Dump the bake (no vectors/text) to build/heirloom-face.ppm.
    #[test]
    #[ignore]
    fn dump_face() {
        let art = super::render::art();
        let img = &art.face;
        let mut out = format!("P6 {} {} 255\n", img.px_w, img.px_h).into_bytes();
        for px in img.rgba.chunks(4) {
            out.extend_from_slice(&px[..3]);
        }
        std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/../../build/heirloom-face.ppm"), out).unwrap();
    }

    /// Dump the record area with the label composited at two angles
    /// (build/heirloom-label-{0,1}.ppm) and time one label render.
    #[test]
    #[ignore]
    fn dump_label() {
        let face = &super::render::art().face;
        let (lx, ly, _, _) = super::render::label_rect();
        for (k, spin) in [0.0, 1.3].iter().enumerate() {
            let t = std::time::Instant::now();
            let lab = super::render::label(*spin);
            println!("label render {:?}", t.elapsed());
            let mut rgba = face.rgba.to_vec();
            let (ox, oy) = ((lx * 2.0) as usize, (ly * 2.0) as usize);
            for j in 0..lab.px_h as usize {
                for i in 0..lab.px_w as usize {
                    let s = (j * lab.px_w as usize + i) * 4;
                    if lab.rgba[s + 3] > 0 {
                        let d = ((oy + j) * face.px_w as usize + ox + i) * 4;
                        rgba[d..d + 3].copy_from_slice(&lab.rgba[s..s + 3]);
                    }
                }
            }
            // Crop to the bottom third.
            let (w, y0) = (face.px_w as usize, 900usize);
            let h = face.px_h as usize - y0;
            let mut out = format!("P6 {w} {h} 255\n").into_bytes();
            for px in rgba[y0 * w * 4..].chunks(4) {
                out.extend_from_slice(&px[..3]);
            }
            let path = format!("{}/../../build/heirloom-label-{k}.ppm", env!("CARGO_MANIFEST_DIR"));
            std::fs::write(path, out).unwrap();
        }
    }
}
