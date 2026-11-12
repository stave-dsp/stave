//! The pedal — aeolian's face as ONE scene, in its own language: a
//! verdigris-enamelled case with brass hardware, and across its top a
//! HARP WINDOW where the twelve strings themselves are strung. Each
//! string vibrates and glows with its own ring (the `ring <note>`
//! taps), in time with the harp's clock (`harp clock`) — the display
//! IS the instrument.
//!
//! Static surfaces are one software-lit bake (render.rs, memoized at
//! 2×, on `stave_ui::photo`); moving hardware is sprites (the
//! register's chicken-head, the push-button caps, the jewel glows);
//! strings, indicators and engraving are live vectors.
//!
//! The scene's declared params, in order (editor.rs):
//! `[wind, sustain, tone, drift, mix, register, air, sway, bypass]`;
//! its probes: the twelve string taps, then the clock.

mod render;

use core::f64::consts::{PI, TAU};

use stave_ui::scene::{pt, Brush, Op, Path, Pt, Scene, SceneInputs, Stroke};
use stave_ui::style::Color;

/// The window, in points.
pub const W: f64 = 700.0;
pub const H: f64 = 500.0;
/// The case: x, y, w, h, corner radius.
const CASE: (f64, f64, f64, f64, f64) = (28.0, 26.0, 644.0, 440.0, 24.0);

/// The harp window: centre, half-size, corner radius.
const WINDOW: (f64, f64, f64, f64, f64) = (350.0, 150.0, 150.0, 88.0, 10.0);
/// Where the strings are held (the brass bridges) and their spacing.
const BRIDGE_TOP: f64 = 82.0;
const BRIDGE_BOT: f64 = 218.0;
const STRING_X0: f64 = 218.0;
const STRING_PITCH: f64 = 24.0;
const NOTES: [&str; 12] = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];

/// Knobs: centre, radius (WIND, SUSTAIN big; TONE, DRIFT, MIX small).
const KNOBS: [((f64, f64), f64); 5] = [
    ((112.0, 150.0), 33.0),
    ((588.0, 150.0), 33.0),
    ((112.0, 322.0), 22.0),
    ((226.0, 322.0), 22.0),
    ((588.0, 322.0), 22.0),
];
const KNOB_LABELS: [&str; 5] = ["WIND", "SUSTAIN", "tone", "drift", "mix"];

/// The REGISTER selector: centre, and its five stops (y-down degrees).
const REGISTER: (f64, f64) = (350.0, 330.0);
const REGISTER_STOPS: [f64; 5] = [210.0, 240.0, 270.0, 300.0, 330.0];
const REGISTER_NAMES: [&str; 5] = ["C2", "C3", "C4", "C5", "C6"];

/// AIR and SWAY push-buttons, and their jewel lamps above.
const BUTTONS: [(f64, f64); 2] = [(462.0, 330.0), (518.0, 330.0)];
const BUTTON_LAMP_DY: f64 = -26.0;
const BUTTON_LABELS: [&str; 2] = ["air", "sway"];

/// The footswitch and its jewel.
const FOOT: (f64, f64) = (588.0, 420.0);
const FOOT_R: f64 = 21.0;
const FOOT_JEWEL: (f64, f64) = (536.0, 420.0);

const SCREWS: [(f64, f64); 4] = [(54.0, 52.0), (646.0, 52.0), (54.0, 440.0), (646.0, 440.0)];

/// Param slots in the scene's declaration order.
const P_REGISTER: usize = 5;
const P_AIR: usize = 6;
const P_BYPASS: usize = 8;
const N_STRINGS: usize = 12;

/// Engraving: cream fill in the enamel.
const ENGRAVE: Color = Color::rgba(0.93, 0.89, 0.78, 0.95);
const ENGRAVE_SOFT: Color = Color::rgba(0.93, 0.89, 0.78, 0.6);

/// A knob's travel: 7:30 → 4:30, clockwise, y-down radians.
fn knob_angle(v: f64) -> f64 {
    (135.0 + 270.0 * v.clamp(0.0, 1.0)).to_radians()
}

fn string_x(c: usize) -> f64 {
    STRING_X0 + c as f64 * STRING_PITCH
}

fn text(sc: &mut Scene, at: Pt, s: &str, size: f64, color: Color) {
    sc.ops.push(Op::Text { at, text: s.into(), size, color });
}

/// One string at ring `level` (0..1), clock `t` seconds: a standing
/// wave between the bridges, its blur envelope, and a glow as it sings.
fn draw_string(sc: &mut Scene, c: usize, level: f64, t: f64) {
    const SEGS: usize = 28;
    let x = string_x(c);
    let (y0, y1) = (BRIDGE_TOP + 4.0, BRIDGE_BOT - 4.0);
    // Wound bronze in the bass turning to bright steel in the treble.
    let k = c as f64 / 11.0;
    let base = Color::rgba(0.80 - 0.05 * k, 0.58 + 0.17 * k, 0.34 + 0.40 * k, 1.0);
    let width = 1.9 - 0.8 * k;
    let amp = 4.5 * level.powf(1.3);
    let phase = (t * (6.0 + 0.85 * c as f64) * TAU).sin();
    let pts = |off: f64, a: f64| -> Vec<Pt> {
        (0..=SEGS)
            .map(|i| {
                let s = i as f64 / SEGS as f64;
                pt(x + off + a * (PI * s).sin(), y0 + (y1 - y0) * s)
            })
            .collect()
    };
    // Shadow on the felt.
    sc.stroke(Path::polyline(&pts(2.2, amp * phase * 0.8)), Stroke::round(Color::rgba(0.0, 0.0, 0.0, 0.55), width + 1.2));
    if level > 0.02 {
        // The blur a fast string leaves in the eye, and its bloom.
        let mut env = Path::polyline(&pts(0.0, amp));
        for p in pts(0.0, -amp).into_iter().rev() {
            env = env.line_to(p);
        }
        let glow = Color::rgba(1.0, 0.86, 0.6, 0.10 + 0.22 * level);
        sc.ops.push(Op::Fill { path: env.close(), brush: Brush::Solid(glow) });
        sc.stroke(Path::polyline(&pts(0.0, amp * phase)), Stroke::round(Color::rgba(1.0, 0.8, 0.5, 0.25 * level), width + 5.0));
    }
    let lit = |v: f64, w: f64| v + (w - v) * 0.6 * level;
    let col = Color::rgba(lit(base.r, 1.0), lit(base.g, 0.95), lit(base.b, 0.85), 1.0);
    sc.stroke(Path::polyline(&pts(0.0, amp * phase)), Stroke::round(col, width));
    sc.stroke(
        Path::polyline(&pts(-0.35 * width, amp * phase)),
        Stroke::round(Color::rgba(1.0, 1.0, 1.0, 0.45 + 0.3 * level), 0.55),
    );
}

/// The picture: bake, then the live parts in paint order.
pub fn build(inp: &SceneInputs<'_>) -> Scene {
    let mut sc = Scene::new();
    let art = render::art();
    sc.ops.push(Op::Image { at: pt(0.0, 0.0), w: W, h: H, image: art.face.clone() });

    // The strings.
    let t = inp.probes.get(N_STRINGS).copied().filter(|v| v.is_finite()).unwrap_or(0.0);
    for c in 0..N_STRINGS {
        let level = inp.probes.get(c).copied().filter(|v| v.is_finite()).unwrap_or(0.0).clamp(0.0, 1.0);
        draw_string(&mut sc, c, level, t);
        let sharp = NOTES[c].len() > 1;
        text(&mut sc, pt(string_x(c), WINDOW.1 + WINDOW.3 + 18.0), NOTES[c], if sharp { 8.0 } else { 10.0 }, if sharp { ENGRAVE_SOFT } else { ENGRAVE });
    }

    // Knobs: an engraved line on the spun cap, an ivory dot on the skirt.
    for (i, &((cx, cy), r)) in KNOBS.iter().enumerate() {
        let (lo, hi) = inp.ranges[i];
        let a = knob_angle((inp.params[i] - lo) / (hi - lo));
        let (dx, dy) = (a.cos(), a.sin());
        let cap = r * 0.68;
        sc.stroke(
            Path::new().move_to(pt(cx + dx * cap * 0.25, cy + dy * cap * 0.25)).line_to(pt(cx + dx * cap * 0.92, cy + dy * cap * 0.92)),
            Stroke::round(Color::rgba(0.16, 0.10, 0.04, 0.9), if r > 25.0 { 2.6 } else { 2.0 }),
        );
        sc.solid(Path::circle(pt(cx + dx * r * 0.86, cy + dy * r * 0.86), if r > 25.0 { 2.2 } else { 1.7 }), Color::rgba(0.95, 0.92, 0.84, 1.0));
        let big = r > 25.0;
        text(&mut sc, pt(cx, cy + r + if big { 26.0 } else { 20.0 }), KNOB_LABELS[i], if big { 13.0 } else { 11.0 }, ENGRAVE);
    }

    // REGISTER: the chicken-head at its stop, the stop names on the arc.
    let stop = (inp.params[P_REGISTER].round() as usize).min(4);
    let (rw, rh) = render::POINTER_SPRITE;
    sc.ops.push(Op::Image { at: pt(REGISTER.0 - rw / 2.0, REGISTER.1 - rh / 2.0), w: rw, h: rh, image: art.pointer[stop].clone() });
    for (k, deg) in REGISTER_STOPS.iter().enumerate() {
        let a = deg.to_radians();
        let col = if k == stop { Color::rgba(1.0, 0.86, 0.55, 1.0) } else { ENGRAVE_SOFT };
        text(&mut sc, pt(REGISTER.0 + a.cos() * 44.0, REGISTER.1 + a.sin() * 44.0 + 3.0), REGISTER_NAMES[k], 9.0, col);
    }
    text(&mut sc, pt(REGISTER.0, REGISTER.1 + 44.0), "register", 11.0, ENGRAVE);

    // AIR / SWAY: the cap (raised or latched down) and its jewel.
    let (bw, bh) = render::BUTTON_SPRITE;
    let (gw, gh) = render::GLOW_SPRITE;
    for (k, &(bx, by)) in BUTTONS.iter().enumerate() {
        let on = inp.params[P_AIR + k] > 0.5;
        let cap = if on { &art.button_down } else { &art.button_up };
        sc.ops.push(Op::Image { at: pt(bx - bw / 2.0, by - bh / 2.0), w: bw, h: bh, image: cap.clone() });
        if on {
            sc.ops.push(Op::Image { at: pt(bx - gw / 2.0, by + BUTTON_LAMP_DY - gh / 2.0), w: gw, h: gh, image: art.glow.clone() });
        }
        text(&mut sc, pt(bx, by + 30.0), BUTTON_LABELS[k], 11.0, ENGRAVE);
    }

    // The footswitch jewel.
    if inp.params[P_BYPASS] < 0.5 {
        sc.ops.push(Op::Image { at: pt(FOOT_JEWEL.0 - gw / 2.0, FOOT_JEWEL.1 - gh / 2.0), w: gw, h: gh, image: art.glow.clone() });
    }

    // The name, and a gust etched through it.
    text(&mut sc, pt(176.0, 428.0), "aeolian", 34.0, ENGRAVE);
    text(&mut sc, pt(176.0, 446.0), "a harp the music plays", 8.5, ENGRAVE_SOFT);
    for (k, dy) in [0.0f64, 9.0, 18.0].iter().enumerate() {
        let x0 = 268.0 + 6.0 * k as f64;
        let y = 404.0 + dy;
        sc.stroke(
            Path::new()
                .move_to(pt(x0, y))
                .cubic_to(pt(x0 + 40.0, y - 10.0), pt(x0 + 70.0, y + 10.0), pt(x0 + 120.0 - 20.0 * k as f64, y - 2.0)),
            Stroke::round(Color::rgba(0.93, 0.89, 0.78, 0.35 - 0.08 * k as f64), 1.2),
        );
    }
    sc
}

/// The hit-map: knobs drag vertically (200 pt = full travel); the
/// register follows the pointer's angle to the nearest stop; a button
/// or the footswitch flips its latch.
pub fn drag(inp: &SceneInputs<'_>, from: Pt, now: Pt) -> Option<(usize, f64)> {
    for (i, &((cx, cy), r)) in KNOBS.iter().enumerate() {
        if (from.x - cx).hypot(from.y - cy) <= r + 6.0 {
            let (lo, hi) = inp.ranges[i];
            let v = (inp.params[i] - lo) / (hi - lo);
            let moved = (v + (from.y - now.y) / 200.0 + (now.x - from.x) / 400.0).clamp(0.0, 1.0);
            return Some((i, lo + (hi - lo) * moved));
        }
    }
    let (rx, ry) = REGISTER;
    if (from.x - rx).hypot(from.y - ry) <= 50.0 && from.y <= ry + 16.0 {
        let (dx, dy) = (now.x - rx, now.y - ry);
        if dx.hypot(dy) < 6.0 {
            return Some((P_REGISTER, inp.params[P_REGISTER].round()));
        }
        let deg = dy.atan2(dx).to_degrees().rem_euclid(360.0);
        let nearest = (0..5)
            .min_by(|&a, &b| {
                let d = |k: usize| ((deg - REGISTER_STOPS[k] + 540.0).rem_euclid(360.0) - 180.0).abs();
                d(a).total_cmp(&d(b))
            })
            .unwrap_or(2);
        return Some((P_REGISTER, nearest as f64));
    }
    for (k, &(bx, by)) in BUTTONS.iter().enumerate() {
        if (from.x - bx).hypot(from.y - by) <= 14.0 {
            let slot = P_AIR + k;
            return Some((slot, if inp.params[slot] > 0.5 { 0.0 } else { 1.0 }));
        }
    }
    if (from.x - FOOT.0).hypot(from.y - FOOT.1) <= FOOT_R + 4.0 {
        return Some((P_BYPASS, if inp.params[P_BYPASS] > 0.5 { 0.0 } else { 1.0 }));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use stave_ui::scene::ScenePalette;

    const RANGES: [(f64, f64); 9] =
        [(0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.0, 4.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0)];
    const PARAMS: [f64; 9] = [0.5, 0.6, 0.6, 0.3, 0.5, 2.0, 1.0, 0.0, 0.0];

    fn inputs<'a>(params: &'a [f64], probes: &'a [f64]) -> SceneInputs<'a> {
        let c = Color::rgba(0.0, 0.0, 0.0, 1.0);
        SceneInputs {
            params,
            ranges: &RANGES,
            probes,
            w: W,
            h: H,
            palette: ScenePalette { accent: c, ink_hi: c, ink_muted: c, axis: c, bg: c },
        }
    }

    #[test]
    fn knobs_drag_up_and_clamp() {
        let inp = inputs(&PARAMS, &[]);
        let ((cx, cy), _) = KNOBS[1];
        assert_eq!(drag(&inp, pt(cx, cy), pt(cx, cy - 1000.0)), Some((1, 1.0)));
        let (i, v) = drag(&inp, pt(cx, cy), pt(cx, cy - 20.0)).unwrap();
        assert_eq!(i, 1);
        assert!((v - 0.7).abs() < 1e-9);
    }

    #[test]
    fn the_register_follows_the_pointer_to_the_nearest_stop() {
        let inp = inputs(&PARAMS, &[]);
        let (rx, ry) = REGISTER;
        // Grab the knob, swing to the far left / far right / straight up.
        assert_eq!(drag(&inp, pt(rx, ry), pt(rx - 40.0, ry + 5.0)), Some((P_REGISTER, 0.0)));
        assert_eq!(drag(&inp, pt(rx, ry), pt(rx + 40.0, ry + 5.0)), Some((P_REGISTER, 4.0)));
        assert_eq!(drag(&inp, pt(rx, ry), pt(rx, ry - 40.0)), Some((P_REGISTER, 2.0)));
        // A click on the C3 legend selects C3; a still click keeps the stop.
        let a = REGISTER_STOPS[1].to_radians();
        let c3 = pt(rx + a.cos() * 44.0, ry + a.sin() * 44.0);
        assert_eq!(drag(&inp, c3, c3), Some((P_REGISTER, 1.0)));
        assert_eq!(drag(&inp, pt(rx, ry), pt(rx, ry)), Some((P_REGISTER, 2.0)));
    }

    #[test]
    fn buttons_and_footswitch_flip() {
        let inp = inputs(&PARAMS, &[]);
        let (bx, by) = BUTTONS[0];
        assert_eq!(drag(&inp, pt(bx, by), pt(bx, by)), Some((P_AIR, 0.0)));
        let (bx, by) = BUTTONS[1];
        assert_eq!(drag(&inp, pt(bx, by), pt(bx, by)), Some((P_AIR + 1, 1.0)));
        assert_eq!(drag(&inp, pt(FOOT.0, FOOT.1), pt(FOOT.0, FOOT.1)), Some((P_BYPASS, 1.0)));
    }

    #[test]
    fn a_ringing_string_moves_and_glows() {
        let mut still = [0.0; 13];
        still[12] = 0.37;
        let mut sing = still;
        sing[4] = 1.0;
        let a = build(&inputs(&PARAMS, &still));
        let b = build(&inputs(&PARAMS, &sing));
        // The singing string adds its blur envelope + bloom…
        assert!(b.ops.len() > a.ops.len());
        // …and its line is no longer straight.
        let bent = b.ops.iter().any(|op| match op {
            Op::Stroke { path, .. } => path.segs.windows(2).any(|w| match (w[0], w[1]) {
                (stave_ui::scene::Seg::LineTo(p), stave_ui::scene::Seg::LineTo(q)) => {
                    (p.x - q.x).abs() > 0.05 && (p.x - string_x(4)).abs() < 8.0
                }
                _ => false,
            }),
            _ => false,
        });
        assert!(bent, "the E string did not move");
    }

    #[test]
    fn strings_sit_inside_the_window() {
        let (cx, _, hw, _, _) = WINDOW;
        assert!(string_x(0) > cx - hw + 8.0 && string_x(11) < cx + hw - 8.0);
    }

    #[test]
    fn empty_space_grabs_nothing() {
        let inp = inputs(&PARAMS, &[]);
        assert_eq!(drag(&inp, pt(5.0, 5.0), pt(5.0, 50.0)), None);
    }
}
