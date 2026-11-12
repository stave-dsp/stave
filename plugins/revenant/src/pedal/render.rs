//! The bake — revenant photographed, in software, with the shared
//! studio (`stave_ui::photo`): a blue-grey HAMMERTONE case (paint
//! dimpled by a field of shallow round dents — cellular noise for a
//! height map), a chrome-ringed porthole onto a dark fluid lit from
//! within (motes adrift, a caustic shimmer), a cream analog meter with
//! its scale, black domed knobs on chrome skirts, chrome footswitches.
//! Sprites: neuron glows (cyan +, violet −) at eight levels, lamps,
//! and both panes of glass.

extern crate alloc;
use alloc::vec::Vec;
use std::sync::OnceLock;

use stave_ui::photo::*;
use stave_ui::scene::Image;

use super::{
    filaments, knob_angle, neuron_pos, CASE, FOOTS, FOOT_R, H, KNOBS, KNOB_R, KNOB_Y, METER, N_SHOWN, PIVOT, PORT, PORT_RING,
    SCREWS, SWEEP, W,
};

const S: f64 = 2.0;
pub(super) const GLOW_SPRITE: (f64, f64) = (36.0, 36.0);
pub(super) const GLOW_LEVELS: usize = 8;
pub(super) const LAMP_SPRITE: (f64, f64) = (40.0, 40.0);
const WALL: f64 = 10.0;

pub(super) struct Art {
    pub face: Image,
    pub port_glass: Image,
    pub meter_glass: Image,
    pub glow_pos: Vec<Image>,
    pub glow_neg: Vec<Image>,
    pub lamp_red: Image,
    pub lamp_blue: Image,
}

pub(super) fn art() -> &'static Art {
    static ART: OnceLock<Art> = OnceLock::new();
    ART.get_or_init(|| {
        let cyan = [0.35, 1.0, 1.0];
        let violet = [0.75, 0.4, 1.0];
        let (_, _, pr) = PORT;
        let (_, _, mhw, mhh) = METER;
        Art {
            face: bake(W, H, S, face_px),
            port_glass: bake_sprite((2.0 * pr, 2.0 * pr), S, port_glass_px),
            meter_glass: bake_sprite((2.0 * mhw, 2.0 * mhh), S, meter_glass_px),
            glow_pos: (1..=GLOW_LEVELS).map(|k| bake_sprite(GLOW_SPRITE, S, |x, y| neuron_px(x, y, cyan, k))).collect(),
            glow_neg: (1..=GLOW_LEVELS).map(|k| bake_sprite(GLOW_SPRITE, S, |x, y| neuron_px(x, y, violet, k))).collect(),
            lamp_red: bake_sprite(LAMP_SPRITE, S, |x, y| lamp_px(x, y, hex(0xFF2A1A))),
            lamp_blue: bake_sprite(LAMP_SPRITE, S, |x, y| lamp_px(x, y, hex(0x4AA8FF))),
        }
    })
}

fn cov(d: f64) -> f64 {
    cover(d, S)
}

fn case_sd(x: f64, y: f64) -> f64 {
    let (cx, cy, w, h, r) = CASE;
    sd_rrect(x, y, cx + w / 2.0, cy + h / 2.0, w / 2.0, h / 2.0, r)
}

fn meter_sd(x: f64, y: f64) -> f64 {
    let (mx, my, hw, hh) = METER;
    sd_rrect(x, y, mx, my, hw, hh, 8.0)
}

fn dome(dx: f64, dy: f64, r: f64, flat: f64) -> V3 {
    let rr = dx.hypot(dy);
    let k = (rr / (r * flat)).min(0.99);
    let (ux, uy) = if rr > 1e-6 { (dx / rr, dy / rr) } else { (0.0, 0.0) };
    v3(ux * k, uy * k, (1.0 - k * k).sqrt())
}

fn desk(x: f64, y: f64) -> Rgb {
    let felt = 0.85 + 0.15 * vnoise(x, y, 0.9, 7) + 0.08 * (hash2((x * S) as i64, (y * S) as i64, 3) - 0.5);
    let pool = (-(((x - 170.0) / 440.0).powi(2) + ((y - 60.0) / 380.0).powi(2))).exp();
    let vign = 1.0 - 0.45 * smoothstep(300.0, 600.0, ((x - 360.0).powi(2) + (y - 270.0).powi(2)).sqrt());
    let base = mul(hex(0x232427), felt * (0.55 + 0.7 * pool) * vign);
    let shadow = 1.0 - 0.78 * (1.0 - smoothstep(-16.0, 50.0, case_sd(x - 11.0, y - 21.0)));
    let contact = 1.0 - 0.6 * (1.0 - smoothstep(0.0, 6.0, case_sd(x, y).min(case_sd(x, y - WALL))));
    mul(base, shadow * contact)
}

// ---------------------------------------------------------- hammertone

/// The hammertone's dents: distance to the nearest of a jittered grid
/// of points (cellular noise), shaped into shallow bowls. Returns the
/// surface slope (dh/dx, dh/dy).
fn hammer_slope(x: f64, y: f64) -> (f64, f64) {
    const CELL: f64 = 3.6;
    let (gx, gy) = ((x / CELL).floor() as i64, (y / CELL).floor() as i64);
    let (mut best, mut bx, mut by) = (f64::MAX, 0.0, 0.0);
    for j in -1..=1 {
        for i in -1..=1 {
            let (cx, cy) = (gx + i, gy + j);
            let px = (cx as f64 + hash2(cx, cy, 301)) * CELL;
            let py = (cy as f64 + hash2(cx, cy, 302)) * CELL;
            let d = (x - px).hypot(y - py);
            if d < best {
                best = d;
                bx = px;
                by = py;
            }
        }
    }
    // A bowl: slope grows toward the dent's rim, then flattens.
    let r = best / CELL;
    let k = 0.22 * r * (1.0 - smoothstep(0.55, 0.85, r));
    let (ux, uy) = if best > 1e-6 { ((x - bx) / best, (y - by) / best) } else { (0.0, 0.0) };
    (ux * k, uy * k)
}

fn case_px(x: f64, y: f64, d: f64) -> Rgb {
    const BEVEL: f64 = 10.0;
    let (cx, cy, w, h, r) = CASE;
    let mut n = v3(0.0, 0.0, 1.0);
    if d > -BEVEL {
        let (gx, gy) = grad_rrect(x, y, cx + w / 2.0, cy + h / 2.0, w / 2.0, h / 2.0, r);
        let k = ((BEVEL + d) / BEVEL).clamp(0.0, 0.999);
        n = v3(gx * k, gy * k, (1.0 - k * k).sqrt());
    }
    let (hx, hy) = hammer_slope(x, y);
    let n = v3(n.x - hx, n.y - hy, n.z).norm();
    // Metallic paint: a blue-grey base with flake that catches the studio.
    let paint = plastic(hex(0x46525E), n, 0.18, 0.45);
    let flake = mul(chrome(n, [0.55, 0.62, 0.7]), 0.22);
    let fall = 1.07 - 0.18 * (((x - cx) / w) * 0.45 + ((y - cy) / h) * 0.55);
    let mut c = mul(add(mul(paint, 0.85), flake), fall);
    // Shadows the raised hardware casts.
    let mut occ = 1.0;
    let (px, py, pr) = PORT;
    occ *= 1.0 - 0.5 * (1.0 - smoothstep(-3.0, 12.0, (x - px - 5.0).hypot(y - py - 8.0) - pr - PORT_RING));
    occ *= 1.0 - 0.45 * (1.0 - smoothstep(-2.0, 9.0, meter_sd(x - 4.0, y - 6.0) - 6.0));
    for &(kx, _) in &KNOBS {
        occ *= 1.0 - 0.55 * (1.0 - smoothstep(-5.0, 13.0, (x - kx - 4.5).hypot(y - KNOB_Y - 7.5) - KNOB_R));
    }
    for &((fx, fy), _, _) in &FOOTS {
        occ *= 1.0 - 0.5 * (1.0 - smoothstep(-6.0, 12.0, (x - fx - 4.0).hypot(y - fy - 6.0) - FOOT_R));
    }
    c = mul(c, occ);
    // Engraved knob scales.
    for &(kx, _) in &KNOBS {
        let sr = KNOB_R + 8.0;
        if (x - kx).abs() > sr + 3.0 || (y - KNOB_Y).abs() > sr + 3.0 {
            continue;
        }
        for k in 0..=10 {
            let a = knob_angle(f64::from(k) / 10.0);
            let dd = (x - kx - a.cos() * sr).hypot(y - KNOB_Y - a.sin() * sr) - if k % 5 == 0 { 1.5 } else { 0.9 };
            c = mix(c, [0.82, 0.83, 0.82], cov(dd) * 0.85);
        }
    }
    c
}

fn case_wall(y: f64) -> Rgb {
    let (_, cy, _, h, _) = CASE;
    let t = ((y - (cy + h - 14.0)) / (WALL + 14.0)).clamp(0.0, 1.0);
    let n = v3(0.0, 0.85, 0.5).norm();
    mul(plastic(hex(0x3C4650), n, 0.2, 0.5), 0.78 - 0.38 * t)
}

// ------------------------------------------------------------ porthole

/// The porthole: a chrome ring, and behind the glass a dark fluid lit
/// from within — the organism's medium. (Neurons and filaments are
/// live; their resting traces are baked faintly, like a slide.)
fn porthole(x: f64, y: f64) -> Option<(Rgb, f64)> {
    let (px, py, pr) = PORT;
    let (dx, dy) = (x - px, y - py);
    let r = dx.hypot(dy);
    if r > pr + PORT_RING + 0.5 {
        return None;
    }
    if r > pr {
        // The ring: a rolled chrome torus.
        let t = ((r - pr) / PORT_RING) * 2.0 - 1.0;
        let (ux, uy) = (dx / r, dy / r);
        let n = v3(ux * t * 0.9, uy * t * 0.9, (1.0 - (t * 0.9).powi(2)).sqrt());
        let c = mul(chrome(n, [0.86, 0.87, 0.9]), 0.95 + 0.05 * (r * 3.0).sin());
        return Some((c, cov(r - pr - PORT_RING)));
    }
    // The fluid: deep blue-black, brighter at the heart, motes adrift,
    // a slow caustic shimmer.
    let depth = (r / pr).powi(2);
    let mut c = mix([0.02, 0.06, 0.08], [0.004, 0.008, 0.014], depth);
    let caustic = vnoise(x, y, 0.05, 311) * vnoise(x, y, 0.11, 312);
    c = add(c, mul([0.05, 0.16, 0.18], smoothstep(0.25, 0.6, caustic) * (1.0 - depth)));
    let mote = smoothstep(0.985, 1.0, hash2((x * 0.9) as i64, (y * 0.9) as i64, 313));
    c = add(c, mul([0.2, 0.4, 0.45], mote * 0.5));
    // The resting body plan: faint filaments and soma, like a slide.
    for (i, j) in filaments() {
        let (xi, yi) = neuron_pos(i);
        let (xj, yj) = neuron_pos(j);
        let (vx, vy) = (xj - xi, yj - yi);
        let t = (((x - xi) * vx + (y - yi) * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
        let d = (x - xi - vx * t).hypot(y - yi - vy * t);
        c = add(c, mul([0.1, 0.22, 0.26], 0.35 * (1.0 - smoothstep(0.0, 1.2, d))));
    }
    for i in 0..N_SHOWN {
        let (nx, ny) = neuron_pos(i);
        let d = (x - nx).hypot(y - ny);
        c = add(c, mul([0.12, 0.25, 0.3], 0.6 * (1.0 - smoothstep(1.5, 3.2, d))));
    }
    // The ring's shadow on the fluid, upper left.
    c = mul(c, 1.0 - 0.6 * smoothstep(pr - 14.0, pr, (dx + 5.0).hypot(dy + 7.0)));
    Some((c, 1.0))
}

// --------------------------------------------------------------- meter

fn meter(x: f64, y: f64) -> Option<(Rgb, f64)> {
    let d = meter_sd(x, y);
    if d > 6.5 {
        return None;
    }
    if d > 0.0 {
        let (mx, my, hw, hh) = METER;
        let (gx, gy) = grad_rrect(x, y, mx, my, hw, hh, 8.0);
        let t = (d / 6.0) * 2.0 - 1.0;
        let n = v3(gx * t * 0.9, gy * t * 0.9, (1.0 - (t * 0.9).powi(2)).sqrt());
        return Some((chrome(n, [0.85, 0.86, 0.88]), cov(d - 6.0)));
    }
    // The face: cream card, the scale arc, ticks, a red zone.
    let paper = 0.95 + 0.04 * vnoise(x, y, 0.5, 321);
    let mut c = mul(hex(0xEDE6D3), paper);
    let (dx, dy) = (x - PIVOT.0, y - PIVOT.1);
    let r = dx.hypot(dy);
    let ang = dy.atan2(dx).to_degrees().rem_euclid(360.0);
    let in_sweep = ang >= SWEEP.0 - 1.0 && ang <= SWEEP.1 + 1.0;
    if in_sweep {
        let arc_r = 96.0;
        c = mix(c, [0.08; 3], cov((r - arc_r).abs() - 0.7));
        let red_from = SWEEP.0 + 0.75 * (SWEEP.1 - SWEEP.0);
        if ang >= red_from {
            c = mix(c, hex(0xC0281C), cov((r - arc_r - 3.0).abs() - 2.0));
        }
        for k in 0..=20 {
            let ta = SWEEP.0 + (SWEEP.1 - SWEEP.0) * f64::from(k) / 20.0;
            let len = if k % 5 == 0 { 9.0 } else { 5.0 };
            let along = (ang - ta).to_radians() * r;
            if r > arc_r && r < arc_r + len {
                c = mix(c, [0.08; 3], cov(along.abs() - 0.6));
            }
        }
    }
    // The pivot's boss.
    let pb = (x - PIVOT.0).hypot(y - PIVOT.1);
    if pb < 7.0 {
        c = mix(c, chrome(dome(x - PIVOT.0, y - PIVOT.1, 6.5, 1.2), [0.3, 0.3, 0.32]), cov(pb - 6.5));
    }
    // The bezel's shadow on the card.
    c = mul(c, 1.0 - 0.35 * smoothstep(-6.0, 0.0, meter_sd(x - 3.0, y - 5.0)));
    Some((c, 1.0))
}

// ---------------------------------------------------------- hardware

/// A black domed knob on a knurled chrome skirt.
fn knob(dx: f64, dy: f64) -> Option<(Rgb, f64)> {
    let r = dx.hypot(dy);
    if r > KNOB_R + 0.5 {
        return None;
    }
    let dome_r = KNOB_R * 0.74;
    let (ux, uy) = if r > 1e-6 { (dx / r, dy / r) } else { (0.0, 0.0) };
    if r > dome_r {
        let t = ((r - dome_r) / (KNOB_R - dome_r)).clamp(0.0, 1.0);
        let tilt = 0.4 + 0.9 * t * t;
        let ridge = (dy.atan2(dx) * 50.0).sin() * 0.3;
        let n = v3(ux * tilt.sin() - uy * ridge, uy * tilt.sin() + ux * ridge, tilt.cos()).norm();
        let c = mul(chrome(n, [0.85, 0.86, 0.9]), 1.0 - 0.5 * (1.0 - smoothstep(0.0, 1.2, r - dome_r)));
        return Some((c, cov(r - KNOB_R)));
    }
    let n = dome(dx, dy, dome_r, 1.25);
    Some((plastic([0.015, 0.015, 0.017], n, 1.1, 0.12), 1.0))
}

fn screw(dx: f64, dy: f64) -> Option<(Rgb, f64)> {
    const R: f64 = 5.5;
    let rr = dx.hypot(dy);
    if rr > R + 0.5 {
        return None;
    }
    let mut c = chrome(dome(dx, dy, R, 1.6), [0.85, 0.85, 0.87]);
    let a = 0.3f64;
    let (u, v) = (dx * a.cos() + dy * a.sin(), -dx * a.sin() + dy * a.cos());
    c = mix(c, [0.02; 3], cov((u.abs().min(v.abs()) - 0.8).max(rr - R * 0.6)));
    Some((c, cov(rr - R)))
}

fn footswitch(dx: f64, dy: f64) -> Option<(Rgb, f64)> {
    let rr = dx.hypot(dy);
    const BTN: f64 = 12.5;
    if rr < BTN + 1.0 {
        let edge = smoothstep(BTN - 1.6, BTN, rr);
        let n = dome(dx, dy, BTN, 1.7);
        let n = v3(n.x * (1.0 + edge), n.y * (1.0 + edge), n.z).norm();
        return Some((mul(chrome(n, [0.93, 0.93, 0.96]), 1.0 - 0.45 * edge), cov(rr - BTN)));
    }
    let dh = sd_hex(dx, dy, FOOT_R - 3.0);
    if dh >= 0.5 {
        return None;
    }
    let e = 0.3;
    let gx = sd_hex(dx + e, dy, FOOT_R - 3.0) - sd_hex(dx - e, dy, FOOT_R - 3.0);
    let gy = sd_hex(dx, dy + e, FOOT_R - 3.0) - sd_hex(dx, dy - e, FOOT_R - 3.0);
    let gl = gx.hypot(gy).max(1e-9);
    let b = smoothstep(-2.0, 0.0, dh) * 0.85;
    let n = v3(gx / gl * b, gy / gl * b, 1.0).norm();
    let ao = 1.0 - 0.5 * (1.0 - smoothstep(0.0, 3.0, rr - BTN));
    Some((mul(chrome(n, [0.82, 0.82, 0.85]), ao), cov(dh)))
}

fn lamp_seat(dx: f64, dy: f64) -> Option<(Rgb, f64)> {
    let rr = dx.hypot(dy);
    if rr > 6.0 {
        return None;
    }
    if rr > 4.2 {
        return Some((chrome(dome(dx, dy, 6.0, 1.0), [0.6, 0.6, 0.62]), cov(rr - 5.8)));
    }
    Some((plastic([0.05, 0.05, 0.06], dome(dx, dy, 4.2, 1.1), 1.3, 0.04), 1.0))
}

fn face_px(x: f64, y: f64) -> Rgb {
    let d = case_sd(x, y);
    let mut c = desk(x, y);
    let dw = case_sd(x, y - WALL);
    if dw < 1.0 && d > -1.0 {
        c = mix(c, case_wall(y), cov(dw));
    }
    if d >= 1.0 {
        return c;
    }
    c = mix(c, case_px(x, y, d), cov(d));
    let mut lay = |p: Option<(Rgb, f64)>| {
        if let Some((pc, a)) = p {
            c = mix(c, pc, a);
        }
    };
    lay(porthole(x, y));
    lay(meter(x, y));
    for &(sx, sy) in &SCREWS {
        lay(screw(x - sx, y - sy));
    }
    for &(kx, _) in &KNOBS {
        lay(knob(x - kx, y - KNOB_Y));
    }
    for &((fx, fy), (lx, ly), _) in &FOOTS {
        lay(footswitch(x - fx, y - fy));
        lay(lamp_seat(x - lx, y - ly));
    }
    c
}

// ------------------------------------------------------------- sprites

/// A neuron firing at `level`: a hot soma and a soft bioluminescent halo.
fn neuron_px(dx: f64, dy: f64, color: Rgb, level: usize) -> (Rgb, f64) {
    let k = level as f64 / GLOW_LEVELS as f64;
    let r = dx.hypot(dy);
    let soma = 1.5 + 2.0 * k;
    let core = cov(r - soma);
    let halo = k * (0.55 * (-(r / (3.0 + 4.0 * k)).powi(2)).exp() + 0.25 * (-(r / (8.0 + 6.0 * k)).powi(2)).exp());
    let hot = (1.0 - r / soma).clamp(0.0, 1.0);
    let c = mix(mul(color, 1.1 + k), [1.8, 2.0, 2.0], hot * k);
    (c, (halo + core * (0.4 + 0.6 * k)).min(1.0))
}

fn lamp_px(dx: f64, dy: f64, color: Rgb) -> (Rgb, f64) {
    let rr = dx.hypot(dy);
    let halo = 0.5 * (-(rr / 6.5).powi(2)).exp() + 0.2 * (-(rr / 15.0).powi(2)).exp();
    let lens = cov(rr - 4.2);
    let hot = (1.0 - rr / 4.2).clamp(0.0, 1.0);
    let lens_c = mix(mul(color, 1.4), [2.1, 2.0, 1.9], hot * hot);
    let c = mix(mul(color, 1.2), lens_c, lens);
    (c, (halo + lens).min(1.0))
}

/// The porthole's domed glass: a curved window reflection, a rim glint.
fn port_glass_px(dx: f64, dy: f64) -> (Rgb, f64) {
    let (_, _, pr) = PORT;
    let r = dx.hypot(dy);
    if r > pr {
        return ([0.0; 3], 0.0);
    }
    let n = dome(dx, dy, pr, 1.25);
    let k = key();
    let spec = n.dot((k + v3(0.0, 0.0, 1.0)).norm()).max(0.0).powf(40.0) * 0.5;
    let window = (-(((dx + 40.0) / 34.0).powi(2) + ((dy + 52.0) / 16.0).powi(2))).exp() * 0.28;
    let rim = (1.0 - smoothstep(0.0, 4.0, pr - r)) * 0.18;
    let a = (0.04 + spec + window + rim).min(1.0);
    let c = mix([0.02, 0.03, 0.04], [0.95, 0.97, 1.0], ((spec + window + rim) / a).min(1.0));
    (c, a)
}

/// The meter's flat glass: a diagonal sheen.
fn meter_glass_px(dx: f64, dy: f64) -> (Rgb, f64) {
    let (_, _, hw, hh) = METER;
    if sd_rrect(dx, dy, 0.0, 0.0, hw, hh, 8.0) > 0.0 {
        return ([0.0; 3], 0.0);
    }
    let u = (dx + hw) * 0.6 + (dy + hh) * 0.8;
    let band = (-((u - 70.0) / 26.0).powi(2)).exp() * 0.2 + (-((u - 120.0) / 8.0).powi(2)).exp() * 0.08;
    let a = (0.03 + band).min(1.0);
    (mix([0.0; 3], [1.0; 3], (band / a).min(1.0)), a)
}

#[cfg(test)]
mod preview {
    #[test]
    #[ignore]
    fn dump_face() {
        let img = &super::art().face;
        let mut out = format!("P6 {} {} 255\n", img.px_w, img.px_h).into_bytes();
        for px in img.rgba.chunks(4) {
            out.extend_from_slice(&px[..3]);
        }
        std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/../../build/revenant-face.ppm"), out).unwrap();
    }
}
