//! The face — ribbon as ONE scene: an oxblood crinkle-paint case whose
//! smoked window shows the machine running. The tape loop runs round
//! two aluminium rollers and along the head block (erase, record, three
//! playback heads, capstan); it moves at the motor's real speed (the
//! `tape loop` tap), its splice comes round once a loop, the rollers
//! turn with it, and the lamps under the live heads glow. Below it, an
//! aluminium control plate: fluted bakelite knobs, the HEADS chicken-
//! head, two footswitches.
//!
//! Static surfaces are one software-lit bake (render.rs, on
//! `stave_ui::photo`); the ribbon, roller spokes, indicators and
//! engraving are live vectors; selector, lamps and the glass's
//! reflection are sprites.
//!
//! The scene's declared params, in order (editor.rs):
//! `[rate, intensity, drive, heads, wear, erase, spring, mix, bypass,
//!   brake]`; its probes: the loop phase, the motor speed.

mod render;

use core::f64::consts::{PI, TAU};

use stave_ui::scene::{pt, Op, Path, Pt, Scene, SceneInputs, Stroke};
use stave_ui::style::Color;

/// The window, in points.
pub const W: f64 = 760.0;
pub const H: f64 = 520.0;
/// The case: x, y, w, h, corner radius.
const CASE: (f64, f64, f64, f64, f64) = (30.0, 24.0, 700.0, 460.0, 20.0);
/// The tape window: centre, half-size, corner radius.
const WINDOW: (f64, f64, f64, f64, f64) = (380.0, 140.0, 330.0, 96.0, 12.0);
/// The aluminium control plate: centre, half-size, radius.
const PLATE: (f64, f64, f64, f64, f64) = (380.0, 326.0, 330.0, 70.0, 8.0);

/// The tape path: two rollers (centres, radius) joined by straight
/// runs. The ribbon travels left→right along the BOTTOM run (past the
/// heads), up round the right roller, back along the top.
const ROLL_L: (f64, f64) = (130.0, 140.0);
const ROLL_R: (f64, f64) = (630.0, 140.0);
const ROLL_R_PT: f64 = 42.0;
/// Heads along the bottom run: x, legend.
const HEAD_X: [(f64, &str); 5] = [(250.0, "E"), (300.0, "R"), (370.0, "1"), (440.0, "2"), (510.0, "3")];
const CAPSTAN_X: f64 = 580.0;
const HEAD_LAMP_Y: f64 = 222.0;

/// Controls on the plate: centre x, radius, legend.
const CONTROLS_Y: f64 = 318.0;
const KNOBS: [(f64, f64, &str); 7] = [
    (92.0, 27.0, "RATE"),
    (172.0, 23.0, "INTENSITY"),
    (250.0, 23.0, "DRIVE"),
    (436.0, 23.0, "WEAR"),
    (514.0, 23.0, "ERASE"),
    (592.0, 23.0, "SPRING"),
    (668.0, 23.0, "MIX"),
];
/// Each knob's param slot (the HEADS selector sits between DRIVE and WEAR).
const KNOB_SLOTS: [usize; 7] = [0, 1, 2, 4, 5, 6, 7];
const HEADS_X: f64 = 343.0;
const HEADS_STOPS: [f64; 7] = [195.0, 220.0, 245.0, 270.0, 295.0, 320.0, 345.0];
const HEADS_NAMES: [&str; 7] = ["1", "2", "3", "1+2", "2+3", "1+3", "ALL"];
const HEAD_SETS: [[bool; 3]; 7] = [
    [true, false, false],
    [false, true, false],
    [false, false, true],
    [true, true, false],
    [false, true, true],
    [true, false, true],
    [true, true, true],
];

/// Footswitches (centre) and their lamps.
/// A footswitch: centre, lamp, legend.
type Foot = ((f64, f64), (f64, f64), &'static str);
const FOOTS: [Foot; 2] =
    [((118.0, 442.0), (168.0, 442.0), "BYPASS"), ((642.0, 442.0), (592.0, 442.0), "BRAKE")];
const FOOT_R: f64 = 20.0;

const SCREWS: [(f64, f64); 4] = [(52.0, 260.0), (708.0, 260.0), (52.0, 462.0), (708.0, 462.0)];

const P_HEADS: usize = 3;
const P_BYPASS: usize = 8;
const P_BRAKE: usize = 9;

const INK: Color = Color::rgba(0.09, 0.09, 0.10, 1.0);
const CREAM: Color = Color::rgba(0.94, 0.88, 0.78, 0.95);
const DECK_INK: Color = Color::rgba(0.78, 0.78, 0.76, 0.7);

fn knob_angle(v: f64) -> f64 {
    (135.0 + 270.0 * v.clamp(0.0, 1.0)).to_radians()
}

/// RATE turns on a log taper (its declared range is log).
fn norm(slot: usize, plain: f64, (lo, hi): (f64, f64)) -> f64 {
    if slot == 0 {
        (plain / lo).ln() / (hi / lo).ln()
    } else {
        (plain - lo) / (hi - lo)
    }
}

fn plain(slot: usize, n: f64, (lo, hi): (f64, f64)) -> f64 {
    let n = n.clamp(0.0, 1.0);
    if slot == 0 {
        lo * (hi / lo).powf(n)
    } else {
        lo + (hi - lo) * n
    }
}

/// The tape path's length, points.
fn path_len() -> f64 {
    2.0 * (ROLL_R.0 - ROLL_L.0) + TAU * ROLL_R_PT
}

/// A point `s` points along the path, and the path's direction there.
fn path_at(s: f64) -> (Pt, (f64, f64)) {
    let run = ROLL_R.0 - ROLL_L.0;
    let arc = PI * ROLL_R_PT;
    let s = s.rem_euclid(path_len());
    if s < run {
        (pt(ROLL_L.0 + s, ROLL_L.1 + ROLL_R_PT), (1.0, 0.0))
    } else if s < run + arc {
        let a = PI / 2.0 - (s - run) / ROLL_R_PT;
        (pt(ROLL_R.0 + ROLL_R_PT * a.cos(), ROLL_R.1 + ROLL_R_PT * a.sin()), (a.sin(), -a.cos()))
    } else if s < 2.0 * run + arc {
        (pt(ROLL_R.0 - (s - run - arc), ROLL_R.1 - ROLL_R_PT), (-1.0, 0.0))
    } else {
        let a = -PI / 2.0 - (s - 2.0 * run - arc) / ROLL_R_PT;
        (pt(ROLL_L.0 + ROLL_R_PT * a.cos(), ROLL_L.1 + ROLL_R_PT * a.sin()), (a.sin(), -a.cos()))
    }
}

/// The ribbon's outline as a closed polyline (the stadium).
fn path_poly(steps: usize) -> Path {
    let l = path_len();
    let pts: Vec<Pt> = (0..=steps).map(|i| path_at(l * i as f64 / steps as f64).0).collect();
    Path::polyline(&pts)
}

fn hash(i: u32) -> f64 {
    let mut h = i.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    f64::from(h) / 4_294_967_296.0
}

fn text(sc: &mut Scene, at: Pt, s: &str, size: f64, color: Color) {
    sc.ops.push(Op::Text { at, text: s.into(), size, color });
}

/// The moving machine: the ribbon with its oxide streaks and splice,
/// and the roller spokes, all at loop phase `phase`.
fn draw_tape(sc: &mut Scene, phase: f64) {
    let l = path_len();
    let travel = phase * l;
    // Roller spokes, turned by the tape passing over them.
    for &(cx, cy) in &[ROLL_L, ROLL_R] {
        let a0 = travel / ROLL_R_PT;
        for k in 0..3 {
            let a = a0 + TAU * f64::from(k) / 3.0;
            sc.stroke(
                Path::new().move_to(pt(cx + a.cos() * 9.0, cy + a.sin() * 9.0)).line_to(pt(cx + a.cos() * 31.0, cy + a.sin() * 31.0)),
                Stroke::round(Color::rgba(0.12, 0.12, 0.13, 0.85), 4.0),
            );
            sc.stroke(
                Path::new().move_to(pt(cx + a.cos() * 9.0 - 0.8, cy + a.sin() * 9.0 - 0.8)).line_to(pt(cx + a.cos() * 31.0 - 0.8, cy + a.sin() * 31.0 - 0.8)),
                Stroke::round(Color::rgba(1.0, 1.0, 1.0, 0.18), 1.0),
            );
        }
    }
    // The ribbon: shadow, oxide brown, a satin sheen.
    let ribbon = path_poly(160);
    let mut shadow = ribbon.clone();
    for seg in shadow.segs.iter_mut() {
        if let stave_ui::scene::Seg::MoveTo(p) | stave_ui::scene::Seg::LineTo(p) = seg {
            *p = pt(p.x + 2.0, p.y + 3.0);
        }
    }
    sc.stroke(shadow, Stroke::round(Color::rgba(0.0, 0.0, 0.0, 0.5), 10.0));
    sc.stroke(ribbon.clone(), Stroke::round(Color::rgba(0.25, 0.14, 0.08, 1.0), 8.0));
    sc.stroke(ribbon, Stroke::round(Color::rgba(0.55, 0.36, 0.22, 0.35), 2.5));
    // Oxide streaks fixed on the tape (irregular, so they never strobe).
    for i in 0..56u32 {
        let s = hash(i) * l + travel;
        let len = 4.0 + 10.0 * hash(i + 1000);
        let (p0, d) = path_at(s);
        let (p1, _) = path_at(s + len);
        let off = (hash(i + 2000) - 0.5) * 4.0;
        let n = (-d.1, d.0);
        sc.stroke(
            Path::new().move_to(pt(p0.x + n.0 * off, p0.y + n.1 * off)).line_to(pt(p1.x + n.0 * off, p1.y + n.1 * off)),
            Stroke::new(Color::rgba(0.12, 0.06, 0.03, 0.45 + 0.3 * hash(i + 3000)), 0.8),
        );
    }
    // The splice: a strip of pale leader tape, once round the loop.
    let s0 = travel;
    let pts: Vec<Pt> = (0..=8).map(|k| path_at(s0 + 22.0 * f64::from(k) / 8.0).0).collect();
    sc.stroke(Path::polyline(&pts), Stroke::new(Color::rgba(0.88, 0.85, 0.76, 0.95), 8.0));
}

/// The picture: bake, then the live parts in paint order.
pub fn build(inp: &SceneInputs<'_>) -> Scene {
    let mut sc = Scene::new();
    let art = render::art();
    sc.ops.push(Op::Image { at: pt(0.0, 0.0), w: W, h: H, image: art.face.clone() });

    // The machine behind the glass.
    let phase = inp.probes.first().copied().filter(|v| v.is_finite()).unwrap_or(0.0);
    draw_tape(&mut sc, phase);
    for &(x, label) in &HEAD_X {
        text(&mut sc, pt(x, 236.0 - 4.0), label, 8.0, DECK_INK);
    }
    // Lamps under the live playback heads.
    let set = HEAD_SETS[(inp.params[P_HEADS].round() as usize).min(6)];
    let (gw, gh) = render::GLOW_SPRITE;
    for (k, on) in set.iter().enumerate() {
        if *on {
            let x = HEAD_X[2 + k].0 + 16.0;
            sc.ops.push(Op::Image { at: pt(x - gw / 2.0, HEAD_LAMP_Y - gh / 2.0), w: gw, h: gh, image: art.glow_red.clone() });
        }
    }
    // The glass over it all.
    let (wx, wy, whw, whh, _) = WINDOW;
    sc.ops.push(Op::Image { at: pt(wx - whw, wy - whh), w: 2.0 * whw, h: 2.0 * whh, image: art.glass.clone() });

    // Knobs: a red line on the spun cap, legends below.
    for (k, &(x, r, label)) in KNOBS.iter().enumerate() {
        let slot = KNOB_SLOTS[k];
        let a = knob_angle(norm(slot, inp.params[slot], inp.ranges[slot]));
        let (dx, dy) = (a.cos(), a.sin());
        let cap = r * 0.6;
        sc.stroke(
            Path::new().move_to(pt(x + dx * cap * 0.2, CONTROLS_Y + dy * cap * 0.2)).line_to(pt(x + dx * r * 0.92, CONTROLS_Y + dy * r * 0.92)),
            Stroke::round(Color::rgba(0.86, 0.16, 0.12, 1.0), if r > 25.0 { 2.6 } else { 2.2 }),
        );
        text(&mut sc, pt(x, CONTROLS_Y + r + 18.0), label, if r > 25.0 { 11.0 } else { 9.5 }, INK);
    }
    // HEADS: the chicken-head at its stop, names round the arc.
    let stop = (inp.params[P_HEADS].round() as usize).min(6);
    let (pw, ph) = render::POINTER_SPRITE;
    sc.ops.push(Op::Image { at: pt(HEADS_X - pw / 2.0, CONTROLS_Y - ph / 2.0), w: pw, h: ph, image: art.pointer[stop].clone() });
    for (k, deg) in HEADS_STOPS.iter().enumerate() {
        let a = deg.to_radians();
        let col = if k == stop { Color::rgba(0.75, 0.1, 0.08, 1.0) } else { INK };
        text(&mut sc, pt(HEADS_X + a.cos() * 40.0, CONTROLS_Y + a.sin() * 40.0 + 3.0), HEADS_NAMES[k], 7.5, col);
    }
    text(&mut sc, pt(HEADS_X, CONTROLS_Y + 44.0), "HEADS", 9.5, INK);

    // Footswitch lamps + legends; the name.
    let lit = [inp.params[P_BYPASS] < 0.5, inp.params[P_BRAKE] > 0.5];
    for (k, &(_, (lx, ly), label)) in FOOTS.iter().enumerate() {
        if lit[k] {
            let img = if k == 0 { &art.glow_red } else { &art.glow_amber };
            sc.ops.push(Op::Image { at: pt(lx - gw / 2.0, ly - gh / 2.0), w: gw, h: gh, image: img.clone() });
        }
        text(&mut sc, pt(lx, ly + 20.0), label, 8.5, CREAM);
    }
    text(&mut sc, pt(380.0, 448.0), "ribbon", 32.0, CREAM);
    text(&mut sc, pt(380.0, 466.0), "tape echo  ·  spring  ·  erase head", 8.5, Color::rgba(0.94, 0.88, 0.78, 0.65));
    sc
}

/// The hit-map: knobs drag vertically (200 pt = full travel); HEADS
/// follows the pointer's angle to the nearest stop; a footswitch click
/// flips its latch.
pub fn drag(inp: &SceneInputs<'_>, from: Pt, now: Pt) -> Option<(usize, f64)> {
    for (k, &(x, r, _)) in KNOBS.iter().enumerate() {
        if (from.x - x).hypot(from.y - CONTROLS_Y) <= r + 6.0 {
            let slot = KNOB_SLOTS[k];
            let v = norm(slot, inp.params[slot], inp.ranges[slot]);
            let moved = v + (from.y - now.y) / 200.0 + (now.x - from.x) / 400.0;
            return Some((slot, plain(slot, moved, inp.ranges[slot])));
        }
    }
    if (from.x - HEADS_X).hypot(from.y - CONTROLS_Y) <= 48.0 && from.y <= CONTROLS_Y + 14.0 {
        let (dx, dy) = (now.x - HEADS_X, now.y - CONTROLS_Y);
        if dx.hypot(dy) < 6.0 {
            return Some((P_HEADS, inp.params[P_HEADS].round()));
        }
        let deg = dy.atan2(dx).to_degrees().rem_euclid(360.0);
        let nearest = (0..7)
            .min_by(|&a, &b| {
                let d = |k: usize| ((deg - HEADS_STOPS[k] + 540.0).rem_euclid(360.0) - 180.0).abs();
                d(a).total_cmp(&d(b))
            })
            .unwrap_or(6);
        return Some((P_HEADS, nearest as f64));
    }
    for (k, &((fx, fy), _, _)) in FOOTS.iter().enumerate() {
        if (from.x - fx).hypot(from.y - fy) <= FOOT_R + 4.0 {
            let slot = if k == 0 { P_BYPASS } else { P_BRAKE };
            return Some((slot, if inp.params[slot] > 0.5 { 0.0 } else { 1.0 }));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use stave_ui::scene::ScenePalette;

    const RANGES: [(f64, f64); 10] = [
        (0.35, 1.6), (0.0, 1.0), (0.0, 1.0), (0.0, 6.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0),
    ];
    const PARAMS: [f64; 10] = [1.0, 0.4, 0.25, 6.0, 0.0, 1.0, 0.3, 0.4, 0.0, 0.0];

    fn inputs<'a>(params: &'a [f64], probes: &'a [f64]) -> SceneInputs<'a> {
        let c = Color::rgba(0.0, 0.0, 0.0, 1.0);
        SceneInputs { params, ranges: &RANGES, probes, w: W, h: H, palette: ScenePalette { accent: c, ink_hi: c, ink_muted: c, axis: c, bg: c } }
    }

    #[test]
    fn the_path_is_closed_and_continuous() {
        let l = path_len();
        let (a, _) = path_at(0.0);
        let (b, _) = path_at(l - 1e-6);
        assert!((a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3);
        let mut prev = path_at(0.0).0;
        for i in 1..2000 {
            let p = path_at(l * f64::from(i) / 2000.0).0;
            assert!((p.x - prev.x).hypot(p.y - prev.y) < 1.0, "jump at {i}");
            prev = p;
        }
    }

    #[test]
    fn the_bottom_run_passes_the_heads_left_to_right() {
        let (p, d) = path_at(10.0);
        assert!(p.y > ROLL_L.1 && d.0 > 0.0);
    }

    #[test]
    fn the_tape_moves_with_the_loop_phase() {
        let a = build(&inputs(&PARAMS, &[0.0, 1.0]));
        let b = build(&inputs(&PARAMS, &[0.25, 1.0]));
        assert_ne!(a, b);
    }

    #[test]
    fn rate_turns_on_its_log_taper() {
        let inp = inputs(&PARAMS, &[]);
        let (x, _, _) = KNOBS[0];
        let (slot, v) = drag(&inp, pt(x, CONTROLS_Y), pt(x, CONTROLS_Y - 1000.0)).unwrap();
        assert_eq!(slot, 0);
        assert!((v - 1.6).abs() < 1e-9);
        assert!((plain(0, norm(0, 1.0, RANGES[0]), RANGES[0]) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn heads_follows_the_pointer_and_footswitches_latch() {
        let inp = inputs(&PARAMS, &[]);
        assert_eq!(drag(&inp, pt(HEADS_X, CONTROLS_Y), pt(HEADS_X - 40.0, CONTROLS_Y - 8.0)), Some((P_HEADS, 0.0)));
        assert_eq!(drag(&inp, pt(HEADS_X, CONTROLS_Y), pt(HEADS_X, CONTROLS_Y - 40.0)), Some((P_HEADS, 3.0)));
        let ((fx, fy), _, _) = FOOTS[1];
        assert_eq!(drag(&inp, pt(fx, fy), pt(fx, fy)), Some((P_BRAKE, 1.0)));
        assert_eq!(drag(&inp, pt(5.0, 5.0), pt(5.0, 50.0)), None);
    }
}
