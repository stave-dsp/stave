//! The bake — aeolian photographed, in software, with the shared
//! studio (`stave_ui::photo`): a case in glossy verdigris enamel,
//! mottled with patina and worn to copper at the edges; a brass-
//! bezelled harp window recessed into it, lined in black felt, with
//! brass bridges and string pins; knurled brass knobs with spun caps;
//! the register's brass skirt; push-button bezels; faceted amber
//! jewels; slotted brass screws; a brass footswitch. Moving parts are
//! sprites (chicken-head per stop, button caps, the jewel glow).

extern crate alloc;
use alloc::vec::Vec;
use core::f64::consts::TAU;
use std::sync::OnceLock;

use stave_ui::photo::*;
use stave_ui::scene::Image;

use super::{
    knob_angle, string_x, BRIDGE_BOT, BRIDGE_TOP, BUTTONS, BUTTON_LAMP_DY, CASE, FOOT, FOOT_JEWEL, FOOT_R, H, KNOBS,
    N_STRINGS, REGISTER, REGISTER_STOPS, SCREWS, W, WINDOW,
};

/// Pixels per point.
const S: f64 = 2.0;
pub(super) const GLOW_SPRITE: (f64, f64) = (44.0, 44.0);
pub(super) const POINTER_SPRITE: (f64, f64) = (72.0, 72.0);
pub(super) const BUTTON_SPRITE: (f64, f64) = (30.0, 30.0);
/// How much of the case's front face the camera sees, points.
const WALL: f64 = 10.0;
/// Brass, as the chrome studio sees it.
const BRASS: Rgb = [0.98, 0.76, 0.42];
const IVORY: u32 = 0xEDE3CC;

pub(super) struct Art {
    pub face: Image,
    pub pointer: Vec<Image>,
    pub button_up: Image,
    pub button_down: Image,
    pub glow: Image,
}

pub(super) fn art() -> &'static Art {
    static ART: OnceLock<Art> = OnceLock::new();
    ART.get_or_init(|| Art {
        face: bake(W, H, S, face_px),
        pointer: REGISTER_STOPS.iter().map(|deg| bake_sprite(POINTER_SPRITE, S, |x, y| pointer_px(x, y, deg.to_radians()))).collect(),
        button_up: bake_sprite(BUTTON_SPRITE, S, |x, y| button_px(x, y, false)),
        button_down: bake_sprite(BUTTON_SPRITE, S, |x, y| button_px(x, y, true)),
        glow: bake_sprite(GLOW_SPRITE, S, glow_px),
    })
}

fn cov(d: f64) -> f64 {
    cover(d, S)
}

fn case_sd(x: f64, y: f64) -> f64 {
    let (cx, cy, w, h, r) = CASE;
    sd_rrect(x, y, cx + w / 2.0, cy + h / 2.0, w / 2.0, h / 2.0, r)
}

fn window_sd(x: f64, y: f64) -> f64 {
    let (cx, cy, hw, hh, r) = WINDOW;
    sd_rrect(x, y, cx, cy, hw, hh, r)
}

fn dome(dx: f64, dy: f64, r: f64, flat: f64) -> V3 {
    let rr = dx.hypot(dy);
    let k = (rr / (r * flat)).min(0.99);
    let (ux, uy) = if rr > 1e-6 { (dx / rr, dy / rr) } else { (0.0, 0.0) };
    v3(ux * k, uy * k, (1.0 - k * k).sqrt())
}

// ---------------------------------------------------------------- desk

fn desk(x: f64, y: f64) -> Rgb {
    let felt = 0.85 + 0.15 * vnoise(x, y, 0.9, 7) + 0.08 * (hash2((x * S) as i64, (y * S) as i64, 3) - 0.5);
    let pool = (-(((x - 170.0) / 440.0).powi(2) + ((y - 60.0) / 360.0).powi(2))).exp();
    let vign = 1.0 - 0.45 * smoothstep(280.0, 600.0, ((x - 350.0).powi(2) + (y - 250.0).powi(2)).sqrt());
    let base = mul(hex(0x25262A), felt * (0.55 + 0.7 * pool) * vign);
    let shadow = 1.0 - 0.78 * (1.0 - smoothstep(-16.0, 50.0, case_sd(x - 11.0, y - 21.0)));
    let contact = 1.0 - 0.6 * (1.0 - smoothstep(0.0, 6.0, case_sd(x, y).min(case_sd(x, y - WALL))));
    mul(base, shadow * contact)
}

// ---------------------------------------------------------------- case

/// Verdigris enamel: teal mottled with patina blooms and pale crust.
fn enamel(x: f64, y: f64) -> Rgb {
    let big = vnoise(x, y, 0.012, 101);
    let mid = vnoise(x, y, 0.05, 102);
    let fine = vnoise(x, y, 0.4, 103);
    let mut c = mix(hex(0x2F6A61), hex(0x4C8C7E), smoothstep(0.25, 0.75, big));
    c = mix(c, hex(0x22504A), 0.35 * smoothstep(0.55, 0.85, mid));
    c = mix(c, hex(0x8FBFA9), 0.22 * smoothstep(0.7, 0.95, fine * mid));
    mul(c, 0.96 + 0.08 * (hash2((x * S) as i64, (y * S) as i64, 104) - 0.5))
}

fn case_px(x: f64, y: f64, d: f64) -> Rgb {
    const BEVEL: f64 = 10.0;
    let (cx, cy, w, h, r) = CASE;
    let mut n = v3(0.0, 0.0, 1.0);
    let mut wear = 0.0;
    if d > -BEVEL {
        let (gx, gy) = grad_rrect(x, y, cx + w / 2.0, cy + h / 2.0, w / 2.0, h / 2.0, r);
        let k = ((BEVEL + d) / BEVEL).clamp(0.0, 0.999);
        n = v3(gx * k, gy * k, (1.0 - k * k).sqrt());
        // Handling wears the enamel through to the copper beneath,
        // most where the bevel turns.
        let chips = vnoise(x, y, 0.6, 105) * vnoise(x, y, 0.13, 108);
        wear = 0.85 * smoothstep(0.42, 0.6, chips) * smoothstep(0.55, 0.9, k);
    }
    let gx = (vnoise(x, y, 1.4, 106) - 0.5) * 0.04;
    let gy = (vnoise(x, y, 1.4, 107) - 0.5) * 0.04;
    let n = v3(n.x + gx, n.y + gy, n.z).norm();
    let fall = 1.07 - 0.18 * (((x - cx) / w) * 0.45 + ((y - cy) / h) * 0.55);
    let paint = plastic(enamel(x, y), n, 0.32, 0.72);
    let copper = chrome(n, [0.85, 0.5, 0.32]);
    let mut c = mul(mix(paint, copper, wear), fall);

    // The engraved border, inset.
    let line = cov((d + 14.0).abs() - 0.6);
    c = mix(c, mul(hex(0xEDE3C8), 0.85), line * 0.8);

    // Contact shadows: knobs, the register, buttons, footswitch.
    let mut occ = 1.0;
    for &((kx, ky), r) in &KNOBS {
        occ *= 1.0 - 0.6 * (1.0 - smoothstep(-6.0, 14.0, (x - kx - 5.0).hypot(y - ky - 8.0) - r));
    }
    occ *= 1.0 - 0.5 * (1.0 - smoothstep(-5.0, 12.0, (x - REGISTER.0 - 4.0).hypot(y - REGISTER.1 - 6.0) - 24.0));
    for &(bx, by) in &BUTTONS {
        occ *= 1.0 - 0.4 * (1.0 - smoothstep(-2.0, 6.0, (x - bx - 2.0).hypot(y - by - 3.0) - 12.0));
    }
    occ *= 1.0 - 0.5 * (1.0 - smoothstep(-6.0, 12.0, (x - FOOT.0 - 4.0).hypot(y - FOOT.1 - 6.0) - FOOT_R));
    // The window's brass bezel stands proud of the enamel.
    occ *= 1.0 - 0.45 * (1.0 - smoothstep(-2.0, 8.0, window_sd(x - 3.0, y - 5.0) - 6.0));
    mul(c, occ)
}

/// The case's front face (the camera sits a little in front).
fn case_wall(x: f64, y: f64) -> Rgb {
    let (_, cy, _, h, _) = CASE;
    let t = ((y - (cy + h - 14.0)) / (WALL + 14.0)).clamp(0.0, 1.0);
    let n = v3(0.0, 0.85, 0.5).norm();
    mul(plastic(mul(enamel(x, y * 2.0), 0.9), n, 0.2, 0.6), 0.78 - 0.38 * t)
}

// ---------------------------------------------------------- the window

/// The harp window: a brass bezel tube around a felt-lined recess,
/// its far wall in view, brass bridges with a pin per string.
fn window_px(x: f64, y: f64, dw: f64) -> Option<(Rgb, f64)> {
    const BEZEL: f64 = 6.0;
    if dw > BEZEL + 0.5 {
        return None;
    }
    let (cx, cy, hw, hh, r) = WINDOW;
    if dw > 0.0 {
        // The bezel: a half-round brass rail.
        let (gx, gy) = grad_rrect(x, y, cx, cy, hw, hh, r);
        let t = (dw / BEZEL) * 2.0 - 1.0; // −1 inner edge … 1 outer edge
        let n = v3(gx * t * 0.95, gy * t * 0.95, (1.0 - (t * 0.95).powi(2)).sqrt());
        let c = mul(chrome(n, BRASS), 0.92 + 0.08 * vnoise(x, y, 0.8, 111));
        return Some((c, cov(dw - BEZEL)));
    }
    // Inside: felt, the far (upper) wall, shadows from the rim.
    let felt = 0.8 + 0.2 * vnoise(x, y, 1.1, 112) + 0.1 * (hash2((x * S) as i64, (y * S) as i64, 113) - 0.5);
    let mut c = mul(hex(0x121315), felt);
    let from_top = y - (cy - hh);
    if from_top < 9.0 {
        let n = v3(0.0, 0.9, 0.45).norm();
        let wall = mul(plastic(mul(enamel(x, y), 0.55), n, 0.15, 0.6), 0.5 + 0.05 * from_top);
        c = mix(wall, c, smoothstep(7.0, 9.0, from_top));
    }
    let rim = 1.0 - 0.6 * smoothstep(-4.0, 2.0, window_sd(x - 4.0, y - 7.0));
    let ao = 1.0 - 0.4 * (1.0 - smoothstep(0.0, 10.0, -dw));
    c = mul(c, rim * ao);
    // The bridges: brass bars across, a domed pin at every string.
    for &by in &[BRIDGE_TOP, BRIDGE_BOT] {
        // The bar's shadow on the felt below it.
        if y > by + 4.0 {
            c = mul(c, 1.0 - 0.5 * (1.0 - smoothstep(0.0, 6.0, y - by - 4.0)));
        }
        let dy = y - by;
        if dy.abs() < 5.0 && (x - cx).abs() < hw - 10.0 {
            let t = (dy / 4.0).clamp(-0.99, 0.99);
            let n = v3(0.0, t, (1.0 - t * t).sqrt());
            c = mix(c, chrome(n, BRASS), cov(dy.abs() - 4.0) * cov((x - cx).abs() - (hw - 12.0)));
        }
        for s in 0..N_STRINGS {
            let (px, py) = (x - string_x(s), y - by);
            let pr = px.hypot(py);
            if pr < 3.4 {
                c = mix(c, chrome(dome(px, py, 2.8, 1.0), [1.0, 0.85, 0.6]), cov(pr - 2.8));
            }
        }
    }
    Some((c, 1.0))
}

// --------------------------------------------------------------- parts

/// A brass knob: knurled skirt falling away, spun cap.
fn knob(dx: f64, dy: f64, r: f64) -> Rgb {
    let cap_r = r * 0.7;
    let rr = dx.hypot(dy).max(1e-6);
    let th = dy.atan2(dx);
    let (ux, uy) = (dx / rr, dy / rr);
    if rr > cap_r {
        let t = ((rr - cap_r) / (r - cap_r)).clamp(0.0, 1.0);
        let tilt = 0.35 + 1.05 * t * t;
        let ridge = (th * if r > 25.0 { 56.0 } else { 40.0 }).sin();
        let n = v3(ux * tilt.sin() - uy * 0.3 * ridge, uy * tilt.sin() + ux * 0.3 * ridge, tilt.cos()).norm();
        let groove = 1.0 - 0.7 * (1.0 - smoothstep(0.0, 1.3, rr - cap_r));
        mul(chrome(n, [0.9, 0.68, 0.36]), groove * (1.0 - 0.35 * smoothstep(0.8, 1.0, t)))
    } else {
        let rim = smoothstep(cap_r - 2.2, cap_r - 0.3, rr);
        let n = v3(ux * (0.1 * rr / cap_r + rim * 0.9), uy * (0.1 * rr / cap_r + rim * 0.9), 1.0).norm();
        let kl = key();
        let streak = (0.5 + 0.5 * (2.0 * (th - kl.y.atan2(kl.x))).cos()).powi(3);
        let rings = 0.92 + 0.08 * hash2((rr * 14.0) as i64, 1, 9);
        let spun = (0.35 + 0.6 * streak) * rings;
        let metal = mix([spun * 0.98, spun * 0.78, spun * 0.46], chrome(n, BRASS), 0.35 + 0.4 * rim);
        mul(metal, 1.0 - 0.25 * (1.0 - smoothstep(0.0, 2.2, rr)))
    }
}

/// The register's brass skirt (the chicken-head is a sprite) and its
/// five ivory stop dots.
fn register_base(dx: f64, dy: f64) -> Option<(Rgb, f64)> {
    let rr = dx.hypot(dy);
    if rr < 24.5 {
        let t = (rr / 24.0).clamp(0.0, 1.0);
        let ridge = (dy.atan2(dx) * 48.0).sin() * smoothstep(0.75, 0.85, t);
        let (ux, uy) = if rr > 1e-6 { (dx / rr, dy / rr) } else { (0.0, 0.0) };
        let tilt = 0.25 + 0.9 * t * t;
        let n = v3(ux * tilt.sin() - uy * 0.25 * ridge, uy * tilt.sin() + ux * 0.25 * ridge, tilt.cos()).norm();
        return Some((chrome(n, [0.88, 0.66, 0.35]), cov(rr - 24.0)));
    }
    for deg in REGISTER_STOPS {
        let a = deg.to_radians();
        let d = (dx - a.cos() * 30.0).hypot(dy - a.sin() * 30.0) - 1.6;
        if d < 0.5 {
            return Some((mul(hex(IVORY), 0.85), cov(d)));
        }
    }
    None
}

/// A push-button's brass bezel ring.
fn button_bezel(dx: f64, dy: f64) -> Option<(Rgb, f64)> {
    let rr = dx.hypot(dy);
    if !(7.5..=12.5).contains(&rr) {
        return None;
    }
    let t = ((rr - 10.0) / 2.5).clamp(-1.0, 1.0);
    let (ux, uy) = (dx / rr, dy / rr);
    let n = v3(ux * t * 0.9, uy * t * 0.9, (1.0 - (t * 0.9).powi(2)).sqrt());
    let inner = 1.0 - 0.7 * (1.0 - smoothstep(7.5, 8.6, rr));
    Some((mul(chrome(n, BRASS), inner), cov(rr - 12.0)))
}

/// A faceted amber jewel lamp (unlit) in a brass collar.
fn jewel(dx: f64, dy: f64) -> Option<(Rgb, f64)> {
    let rr = dx.hypot(dy);
    if rr > 8.0 {
        return None;
    }
    let (ux, uy) = if rr > 1e-6 { (dx / rr, dy / rr) } else { (0.0, 0.0) };
    if rr > 5.6 {
        let t = ((rr - 6.8) / 1.2).clamp(-1.0, 1.0);
        let n = v3(ux * t, uy * t, (1.0 - t * t).sqrt());
        return Some((chrome(n, BRASS), cov(rr - 7.8)));
    }
    // Eight facets around a flat table: each facet's normal is fixed,
    // so the key light catches some and not others — the sparkle.
    let sector = ((dy.atan2(dx) / (TAU / 8.0)).floor() + 0.5) * (TAU / 8.0);
    let n = if rr < 2.4 { v3(0.0, 0.0, 1.0) } else { v3(sector.cos() * 0.55, sector.sin() * 0.55, 0.83).norm() };
    Some((plastic([0.20, 0.07, 0.01], n, 1.2, 0.05), 1.0))
}

fn screw(dx: f64, dy: f64) -> Option<(Rgb, f64)> {
    const R: f64 = 6.0;
    let rr = dx.hypot(dy);
    if rr > R + 0.5 {
        return None;
    }
    let mut c = chrome(dome(dx, dy, R, 1.6), [0.85, 0.65, 0.36]);
    let a = 0.4f64;
    let u = -dx * a.sin() + dy * a.cos();
    c = mix(c, [0.03, 0.02, 0.01], cov((u.abs() - 0.9).max(rr - R * 0.8)));
    Some((c, cov(rr - R)))
}

fn footswitch(dx: f64, dy: f64) -> Option<(Rgb, f64)> {
    let rr = dx.hypot(dy);
    const BTN: f64 = 13.0;
    if rr < BTN + 1.0 {
        let edge = smoothstep(BTN - 1.6, BTN, rr);
        let n = dome(dx, dy, BTN, 1.7);
        let n = v3(n.x * (1.0 + edge), n.y * (1.0 + edge), n.z).norm();
        return Some((mul(chrome(n, [0.98, 0.8, 0.5]), 1.0 - 0.45 * edge), cov(rr - BTN)));
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
    Some((mul(chrome(n, [0.88, 0.68, 0.38]), ao * (0.94 + 0.06 * (rr * 5.0).sin())), cov(dh)))
}

fn face_px(x: f64, y: f64) -> Rgb {
    let d = case_sd(x, y);
    let mut c = desk(x, y);
    let dw = case_sd(x, y - WALL);
    if dw < 1.0 && d > -1.0 {
        c = mix(c, case_wall(x, y), cov(dw));
    }
    if d >= 1.0 {
        return c;
    }
    c = mix(c, case_px(x, y, d), cov(d));
    // Engraved knob scales: an arc of dots around each knob.
    for &((kx, ky), r) in &KNOBS {
        let sr = r + 8.0;
        if (x - kx).abs() > sr + 3.0 || (y - ky).abs() > sr + 3.0 {
            continue;
        }
        for k in 0..=10 {
            let a = knob_angle(f64::from(k) / 10.0);
            let dd = (x - kx - a.cos() * sr).hypot(y - ky - a.sin() * sr) - if k % 5 == 0 { 1.6 } else { 1.0 };
            c = mix(c, mul(hex(0xEDE3C8), 0.85), cov(dd));
        }
    }
    let mut lay = |p: Option<(Rgb, f64)>| {
        if let Some((pc, a)) = p {
            c = mix(c, pc, a);
        }
    };
    lay(window_px(x, y, window_sd(x, y)));
    for &(sx, sy) in &SCREWS {
        lay(screw(x - sx, y - sy));
    }
    for &((kx, ky), r) in &KNOBS {
        let (dx, dy) = (x - kx, y - ky);
        let rr = dx.hypot(dy);
        if rr < r + 1.0 {
            lay(Some((knob(dx, dy, r), cov(rr - r))));
        }
    }
    lay(register_base(x - REGISTER.0, y - REGISTER.1));
    for &(bx, by) in &BUTTONS {
        lay(button_bezel(x - bx, y - by));
        lay(jewel(x - bx, y - by - BUTTON_LAMP_DY));
    }
    lay(jewel(x - FOOT_JEWEL.0, y - FOOT_JEWEL.1));
    lay(footswitch(x - FOOT.0, y - FOOT.1));
    c
}

// ------------------------------------------------------------- sprites

/// Lay `c` at coverage `a` over the accumulated sprite pixel.
fn lay_px(col: &mut Rgb, alpha: &mut f64, c: Rgb, a: f64) {
    let na = a + *alpha * (1.0 - a);
    if na > 0.0 {
        for k in 0..3 {
            col[k] = (c[k] * a + col[k] * *alpha * (1.0 - a)) / na;
        }
    }
    *alpha = na;
}

/// The chicken-head's outline at angle `a`: a round hub with a
/// tapering pointer and a short tail. Signed distance.
fn chicken_sd(dx: f64, dy: f64, a: f64) -> f64 {
    let (u, v) = (dx * a.cos() + dy * a.sin(), -dx * a.sin() + dy * a.cos());
    let hub = u.hypot(v) - 11.0;
    let len = 25.0;
    let half = 8.0 * (1.0 - (u / len).clamp(0.0, 1.0)) + 2.2;
    let blade = (v.abs() - half).max(u - len).max(-u);
    let tail = (u + 4.0).hypot(v) - 9.0;
    hub.min(blade).min(tail)
}

fn pointer_px(dx: f64, dy: f64, a: f64) -> (Rgb, f64) {
    let mut col: Rgb = [0.0; 3];
    let mut alpha = 0.5 * (1.0 - smoothstep(-2.0, 6.0, chicken_sd(dx - 3.5, dy - 6.0, a)));
    let d = chicken_sd(dx, dy, a);
    if d < 0.5 {
        let e = 0.3;
        let gx = chicken_sd(dx + e, dy, a) - chicken_sd(dx - e, dy, a);
        let gy = chicken_sd(dx, dy + e, a) - chicken_sd(dx, dy - e, a);
        let gl = gx.hypot(gy).max(1e-9);
        // Moulded: a soft rounded edge, a gentle crown.
        let b = smoothstep(-4.0, 0.0, d) * 0.85;
        let n = v3(gx / gl * b, gy / gl * b, 1.0).norm();
        let mut c = plastic(hex(IVORY), n, 0.35, 0.45);
        // The pointer's engraved line.
        let (u, v) = (dx * a.cos() + dy * a.sin(), -dx * a.sin() + dy * a.cos());
        if u > 6.0 && u < 22.0 {
            c = mix(c, [0.15, 0.1, 0.06], cov(v.abs() - 0.8) * 0.85);
        }
        lay_px(&mut col, &mut alpha, c, cov(d));
    }
    (col, alpha)
}

/// An ivory push-button cap: raised, or latched down (lower, shaded).
fn button_px(dx: f64, dy: f64, down: bool) -> (Rgb, f64) {
    let r = 7.6;
    let rr = dx.hypot(dy);
    let mut col: Rgb = [0.0; 3];
    let lift = if down { 0.6 } else { 2.2 };
    let mut alpha = 0.45 * (1.0 - smoothstep(-1.0, 3.0, (dx - lift).hypot(dy - lift * 1.4) - r));
    if rr < r + 0.5 {
        let n = dome(dx, dy, r, if down { 1.9 } else { 1.35 });
        let c = mul(plastic(hex(IVORY), n, 0.45, 0.4), if down { 0.82 } else { 1.0 });
        lay_px(&mut col, &mut alpha, c, cov(rr - r));
    }
    (col, alpha)
}

/// A lit jewel: amber facets ablaze, and the bloom on the enamel.
fn glow_px(dx: f64, dy: f64) -> (Rgb, f64) {
    let amber = hex(0xFFA62B);
    let rr = dx.hypot(dy);
    let halo = 0.5 * (-(rr / 8.0).powi(2)).exp() + 0.22 * (-(rr / 18.0).powi(2)).exp();
    let lens = cov(rr - 5.6);
    let sector = (dy.atan2(dx) / (TAU / 8.0)).floor() as i64;
    let facet = 0.8 + 0.35 * hash2(sector, 0, 121);
    let hot = (1.0 - rr / 5.6).clamp(0.0, 1.0);
    let lens_c = mul(mix(amber, [2.2, 1.9, 1.4], hot * hot), facet * 1.4);
    let c = mix(mul(amber, 1.3), lens_c, lens);
    (c, (halo + lens).min(1.0))
}

#[cfg(test)]
mod preview {
    /// Dump the bake to build/aeolian-face.ppm (ignored; for eyes).
    #[test]
    #[ignore]
    fn dump_face() {
        let img = &super::art().face;
        let mut out = format!("P6 {} {} 255\n", img.px_w, img.px_h).into_bytes();
        for px in img.rgba.chunks(4) {
            out.extend_from_slice(&px[..3]);
        }
        std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/../../build/aeolian-face.ppm"), out).unwrap();
    }
}
