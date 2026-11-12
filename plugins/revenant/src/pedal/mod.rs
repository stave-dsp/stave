//! The face — revenant as a laboratory instrument: a hammertone case
//! with a PORTHOLE onto the living network (24 of its neurons, glowing
//! cyan as they fire positive and violet negative, joined by filaments
//! that carry light when both ends are active) and an analog SURPRISE
//! meter whose needle swings with how wrong its last guesses were.
//!
//! The case, porthole fluid, meter face and hardware are one software-
//! lit bake (render.rs, on `stave_ui::photo`); the neurons and the
//! needle are live (sprites at eight glow levels, vectors); both glass
//! panes lay their reflections OVER the live parts, as glass does.
//!
//! The scene's declared params, in order (editor.rs):
//! `[learn, memory, chaos, horizon, dream, mix, bypass, sleep]`; its
//! probes: SURPRISE, then the 24 neurons.

mod render;

use core::f64::consts::TAU;

use stave_ui::scene::{pt, Op, Path, Pt, Scene, SceneInputs, Stroke};
use stave_ui::style::Color;

pub const W: f64 = 720.0;
pub const H: f64 = 540.0;
const CASE: (f64, f64, f64, f64, f64) = (28.0, 24.0, 664.0, 488.0, 18.0);

/// The porthole: centre, glass radius, ring width.
const PORT: (f64, f64, f64) = (222.0, 196.0, 112.0);
const PORT_RING: f64 = 16.0;
/// The meter: centre, half-size; needle pivot, length, sweep (y-down °).
const METER: (f64, f64, f64, f64) = (520.0, 168.0, 138.0, 84.0);
const PIVOT: (f64, f64) = (520.0, 236.0);
const NEEDLE: f64 = 112.0;
const SWEEP: (f64, f64) = (218.0, 322.0);

/// Knobs along the bottom: x, legend, slot.
const KNOB_Y: f64 = 392.0;
const KNOB_R: f64 = 25.0;
const KNOBS: [(f64, &str); 6] =
    [(108.0, "LEARN"), (206.0, "MEMORY"), (304.0, "CHAOS"), (402.0, "HORIZON"), (500.0, "DREAM"), (598.0, "MIX")];
/// HORIZON's range is log.
const LOG_SLOT: usize = 3;

/// Footswitches: centre, lamp, legend.
/// A footswitch: centre, lamp, legend.
type Foot = ((f64, f64), (f64, f64), &'static str);
const FOOTS: [Foot; 2] =
    [((108.0, 466.0), (158.0, 466.0), "BYPASS"), ((612.0, 466.0), (562.0, 466.0), "SLEEP")];
const FOOT_R: f64 = 20.0;
const SCREWS: [(f64, f64); 4] = [(50.0, 46.0), (670.0, 46.0), (50.0, 490.0), (670.0, 490.0)];

const P_BYPASS: usize = 6;
const P_SLEEP: usize = 7;
pub(crate) const N_SHOWN: usize = 24;

const ENGRAVE: Color = Color::rgba(0.93, 0.93, 0.90, 0.92);
const ENGRAVE_SOFT: Color = Color::rgba(0.93, 0.93, 0.90, 0.6);
const METER_INK: Color = Color::rgba(0.12, 0.11, 0.10, 1.0);

fn knob_angle(v: f64) -> f64 {
    (135.0 + 270.0 * v.clamp(0.0, 1.0)).to_radians()
}

fn norm(slot: usize, plain: f64, (lo, hi): (f64, f64)) -> f64 {
    if slot == LOG_SLOT {
        (plain / lo).ln() / (hi / lo).ln()
    } else {
        (plain - lo) / (hi - lo)
    }
}

fn plain(slot: usize, n: f64, (lo, hi): (f64, f64)) -> f64 {
    let n = n.clamp(0.0, 1.0);
    if slot == LOG_SLOT {
        lo * (hi / lo).powf(n)
    } else {
        lo + (hi - lo) * n
    }
}

/// Where neuron `i` floats in the porthole: a golden-angle spiral,
/// jittered — a fixed body plan, the same creature every time.
pub(crate) fn neuron_pos(i: usize) -> (f64, f64) {
    let golden = TAU * (1.0 - 1.0 / 1.618_034);
    let r = 88.0 * ((i as f64 + 0.6) / N_SHOWN as f64).sqrt();
    let a = i as f64 * golden;
    let j = |k: u32| {
        let mut h = (i as u32 + 1).wrapping_mul(0x9E37_79B9) ^ k;
        h ^= h >> 15;
        h = h.wrapping_mul(0x2C1B_3C6D);
        (f64::from(h >> 8) / 16_777_216.0 - 0.5) * 12.0
    };
    (PORT.0 + r * a.cos() + j(1), PORT.1 + r * a.sin() + j(2))
}

/// The filaments: each neuron to its two nearest neighbours.
pub(crate) fn filaments() -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for i in 0..N_SHOWN {
        let (xi, yi) = neuron_pos(i);
        let mut d: Vec<(f64, usize)> = (0..N_SHOWN)
            .filter(|&j| j != i)
            .map(|j| {
                let (xj, yj) = neuron_pos(j);
                ((xi - xj).hypot(yi - yj), j)
            })
            .collect();
        d.sort_by(|a, b| a.0.total_cmp(&b.0));
        for &(_, j) in d.iter().take(2) {
            let e = if i < j { (i, j) } else { (j, i) };
            if !out.contains(&e) {
                out.push(e);
            }
        }
    }
    out
}

fn text(sc: &mut Scene, at: Pt, s: &str, size: f64, color: Color) {
    sc.ops.push(Op::Text { at, text: s.into(), size, color });
}

pub fn build(inp: &SceneInputs<'_>) -> Scene {
    let mut sc = Scene::new();
    let art = render::art();
    sc.ops.push(Op::Image { at: pt(0.0, 0.0), w: W, h: H, image: art.face.clone() });

    // The organism: filaments carry light between active neurons…
    let act = |i: usize| inp.probes.get(1 + i).copied().filter(|v| v.is_finite()).unwrap_or(0.0).clamp(-1.0, 1.0);
    for (i, j) in filaments() {
        let a = (act(i).abs() * act(j).abs()).sqrt();
        if a > 0.01 {
            let (xi, yi) = neuron_pos(i);
            let (xj, yj) = neuron_pos(j);
            sc.stroke(
                Path::new().move_to(pt(xi, yi)).line_to(pt(xj, yj)),
                Stroke::round(Color::rgba(0.45, 0.95, 1.0, 0.12 + 0.55 * a), 0.6 + 1.6 * a),
            );
        }
    }
    // …and each neuron glows with its own activity.
    let (gw, gh) = render::GLOW_SPRITE;
    for i in 0..N_SHOWN {
        let a = act(i);
        let level = (a.abs().powf(0.6) * render::GLOW_LEVELS as f64).round() as usize;
        if level > 0 {
            let (x, y) = neuron_pos(i);
            let set = if a >= 0.0 { &art.glow_pos } else { &art.glow_neg };
            sc.ops.push(Op::Image { at: pt(x - gw / 2.0, y - gh / 2.0), w: gw, h: gh, image: set[level - 1].clone() });
        }
    }
    let (px, py, pr) = PORT;
    sc.ops.push(Op::Image { at: pt(px - pr, py - pr), w: 2.0 * pr, h: 2.0 * pr, image: art.port_glass.clone() });

    // The meter's needle (its shadow first), then its glass.
    let s = inp.probes.first().copied().filter(|v| v.is_finite()).unwrap_or(0.0).clamp(0.0, 1.0);
    let a = (SWEEP.0 + (SWEEP.1 - SWEEP.0) * s).to_radians();
    let tip = pt(PIVOT.0 + a.cos() * NEEDLE, PIVOT.1 + a.sin() * NEEDLE);
    sc.stroke(Path::new().move_to(pt(PIVOT.0 + 2.5, PIVOT.1 + 3.5)).line_to(pt(tip.x + 2.5, tip.y + 3.5)), Stroke::round(Color::rgba(0.0, 0.0, 0.0, 0.25), 2.0));
    sc.stroke(Path::new().move_to(pt(PIVOT.0, PIVOT.1)).line_to(tip), Stroke::round(Color::rgba(0.08, 0.07, 0.06, 1.0), 1.6));
    let red = pt(PIVOT.0 + a.cos() * (NEEDLE - 16.0), PIVOT.1 + a.sin() * (NEEDLE - 16.0));
    sc.stroke(Path::new().move_to(red).line_to(tip), Stroke::round(Color::rgba(0.78, 0.12, 0.08, 1.0), 1.6));
    let (mx, my, mhw, mhh) = METER;
    text(&mut sc, pt(mx, my + 14.0), "SURPRISE", 11.0, METER_INK);
    text(&mut sc, pt(mx - 96.0, my + 30.0), "calm", 8.5, METER_INK);
    text(&mut sc, pt(mx + 96.0, my + 30.0), "lost", 8.5, Color::rgba(0.7, 0.12, 0.08, 1.0));
    sc.ops.push(Op::Image { at: pt(mx - mhw, my - mhh), w: 2.0 * mhw, h: 2.0 * mhh, image: art.meter_glass.clone() });

    // Knobs: a white index on the black dome.
    for (k, &(x, label)) in KNOBS.iter().enumerate() {
        let a = knob_angle(norm(k, inp.params[k], inp.ranges[k]));
        let (dx, dy) = (a.cos(), a.sin());
        sc.stroke(
            Path::new().move_to(pt(x + dx * 6.0, KNOB_Y + dy * 6.0)).line_to(pt(x + dx * (KNOB_R - 6.0), KNOB_Y + dy * (KNOB_R - 6.0))),
            Stroke::round(Color::rgba(0.96, 0.95, 0.92, 1.0), 2.4),
        );
        text(&mut sc, pt(x, KNOB_Y + KNOB_R + 22.0), label, 10.0, ENGRAVE);
    }

    // Footswitch lamps, legends, the name.
    let lit = [inp.params[P_BYPASS] < 0.5, inp.params[P_SLEEP] > 0.5];
    let (lw, lh) = render::LAMP_SPRITE;
    for (k, &(_, (lx, ly), label)) in FOOTS.iter().enumerate() {
        if lit[k] {
            let img = if k == 0 { &art.lamp_red } else { &art.lamp_blue };
            sc.ops.push(Op::Image { at: pt(lx - lw / 2.0, ly - lh / 2.0), w: lw, h: lh, image: img.clone() });
        }
        text(&mut sc, pt(lx, ly + 20.0), label, 8.5, ENGRAVE);
    }
    text(&mut sc, pt(360.0, 472.0), "revenant", 30.0, ENGRAVE);
    text(&mut sc, pt(360.0, 490.0), "it learns you  ·  it keeps playing", 8.5, ENGRAVE_SOFT);
    text(&mut sc, pt(PORT.0, PORT.1 + PORT.2 + PORT_RING + 16.0), "96 neurons  ·  echo-state reservoir", 8.0, ENGRAVE_SOFT);
    sc
}

pub fn drag(inp: &SceneInputs<'_>, from: Pt, now: Pt) -> Option<(usize, f64)> {
    for (k, &(x, _)) in KNOBS.iter().enumerate() {
        if (from.x - x).hypot(from.y - KNOB_Y) <= KNOB_R + 6.0 {
            let v = norm(k, inp.params[k], inp.ranges[k]);
            let moved = v + (from.y - now.y) / 200.0 + (now.x - from.x) / 400.0;
            return Some((k, plain(k, moved, inp.ranges[k])));
        }
    }
    for (k, &((fx, fy), _, _)) in FOOTS.iter().enumerate() {
        if (from.x - fx).hypot(from.y - fy) <= FOOT_R + 4.0 {
            let slot = if k == 0 { P_BYPASS } else { P_SLEEP };
            return Some((slot, if inp.params[slot] > 0.5 { 0.0 } else { 1.0 }));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use stave_ui::scene::ScenePalette;

    const RANGES: [(f64, f64); 8] = [(0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.25, 25.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0)];
    const PARAMS: [f64; 8] = [0.6, 0.6, 0.45, 3.0, 0.0, 0.5, 0.0, 0.0];

    fn inputs<'a>(params: &'a [f64], probes: &'a [f64]) -> SceneInputs<'a> {
        let c = Color::rgba(0.0, 0.0, 0.0, 1.0);
        SceneInputs { params, ranges: &RANGES, probes, w: W, h: H, palette: ScenePalette { accent: c, ink_hi: c, ink_muted: c, axis: c, bg: c } }
    }

    #[test]
    fn every_neuron_floats_inside_the_porthole() {
        for i in 0..N_SHOWN {
            let (x, y) = neuron_pos(i);
            assert!((x - PORT.0).hypot(y - PORT.1) < PORT.2 - 8.0, "neuron {i}");
        }
        assert!(filaments().len() >= N_SHOWN);
    }

    #[test]
    fn activity_lights_the_organism_and_surprise_moves_the_needle() {
        let calm = [0.0; 25];
        let mut busy = [0.0; 25];
        busy[0] = 0.9;
        for v in busy.iter_mut().skip(1) {
            *v = 0.7;
        }
        let a = build(&inputs(&PARAMS, &calm));
        let b = build(&inputs(&PARAMS, &busy));
        assert!(b.ops.len() > a.ops.len() + N_SHOWN);
        assert_ne!(a, b);
    }

    #[test]
    fn horizon_turns_on_its_log_taper_and_footswitches_latch() {
        let inp = inputs(&PARAMS, &[]);
        let (x, _) = KNOBS[3];
        let (slot, v) = drag(&inp, pt(x, KNOB_Y), pt(x, KNOB_Y - 1000.0)).unwrap();
        assert_eq!(slot, 3);
        assert!((v - 25.0).abs() < 1e-9);
        let ((fx, fy), _, _) = FOOTS[1];
        assert_eq!(drag(&inp, pt(fx, fy), pt(fx, fy)), Some((P_SLEEP, 1.0)));
        assert_eq!(drag(&inp, pt(5.0, 5.0), pt(5.0, 50.0)), None);
    }
}
