//! The bake — ribbon photographed, in software, with the shared studio
//! (`stave_ui::photo`): an oxblood crinkle-paint case; a recessed tape
//! deck behind smoked glass (dark brushed deck plate, aluminium
//! rollers, the chrome head block, capstan and rubber pinch roller); a
//! brushed-aluminium control plate with fluted bakelite knobs; chrome
//! footswitches and screws. Sprites: the HEADS chicken-head per stop,
//! the lamps' glow, and the glass's own reflection (laid OVER the
//! moving tape, as real glass is).

extern crate alloc;
use alloc::vec::Vec;
use core::f64::consts::TAU;
use std::sync::OnceLock;

use stave_ui::photo::*;
use stave_ui::scene::Image;

use super::{
    knob_angle, CAPSTAN_X, CASE, CONTROLS_Y, FOOTS, FOOT_R, H, HEADS_STOPS, HEADS_X, HEAD_LAMP_Y, HEAD_X, KNOBS, PLATE, ROLL_L,
    ROLL_R, ROLL_R_PT, SCREWS, W, WINDOW,
};

const S: f64 = 2.0;
pub(super) const GLOW_SPRITE: (f64, f64) = (40.0, 40.0);
pub(super) const POINTER_SPRITE: (f64, f64) = (72.0, 72.0);
const WALL: f64 = 10.0;

pub(super) struct Art {
    pub face: Image,
    pub glass: Image,
    pub pointer: Vec<Image>,
    pub glow_red: Image,
    pub glow_amber: Image,
}

pub(super) fn art() -> &'static Art {
    static ART: OnceLock<Art> = OnceLock::new();
    ART.get_or_init(|| {
        let (_, _, hw, hh, _) = WINDOW;
        Art {
            face: bake(W, H, S, face_px),
            glass: bake_sprite((2.0 * hw, 2.0 * hh), S, glass_px),
            pointer: HEADS_STOPS.iter().map(|d| bake_sprite(POINTER_SPRITE, S, |x, y| pointer_px(x, y, d.to_radians()))).collect(),
            glow_red: bake_sprite(GLOW_SPRITE, S, |x, y| glow_px(x, y, hex(0xFF2A1A))),
            glow_amber: bake_sprite(GLOW_SPRITE, S, |x, y| glow_px(x, y, hex(0xFFAE32))),
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

fn window_sd(x: f64, y: f64) -> f64 {
    let (cx, cy, hw, hh, r) = WINDOW;
    sd_rrect(x, y, cx, cy, hw, hh, r)
}

fn plate_sd(x: f64, y: f64) -> f64 {
    let (cx, cy, hw, hh, r) = PLATE;
    sd_rrect(x, y, cx, cy, hw, hh, r)
}

fn dome(dx: f64, dy: f64, r: f64, flat: f64) -> V3 {
    let rr = dx.hypot(dy);
    let k = (rr / (r * flat)).min(0.99);
    let (ux, uy) = if rr > 1e-6 { (dx / rr, dy / rr) } else { (0.0, 0.0) };
    v3(ux * k, uy * k, (1.0 - k * k).sqrt())
}

fn desk(x: f64, y: f64) -> Rgb {
    let felt = 0.85 + 0.15 * vnoise(x, y, 0.9, 7) + 0.08 * (hash2((x * S) as i64, (y * S) as i64, 3) - 0.5);
    let pool = (-(((x - 180.0) / 460.0).powi(2) + ((y - 60.0) / 380.0).powi(2))).exp();
    let vign = 1.0 - 0.45 * smoothstep(300.0, 620.0, ((x - 380.0).powi(2) + (y - 260.0).powi(2)).sqrt());
    let base = mul(hex(0x252629), felt * (0.55 + 0.7 * pool) * vign);
    let shadow = 1.0 - 0.78 * (1.0 - smoothstep(-16.0, 50.0, case_sd(x - 11.0, y - 21.0)));
    let contact = 1.0 - 0.6 * (1.0 - smoothstep(0.0, 6.0, case_sd(x, y).min(case_sd(x, y - WALL))));
    mul(base, shadow * contact)
}

// ---------------------------------------------------------------- case

/// Oxblood crinkle paint: a deep red with a wrinkled, light-catching skin.
fn crinkle_normal(x: f64, y: f64) -> (f64, f64) {
    let e = 0.25;
    let h = |x: f64, y: f64| vnoise(x, y, 1.9, 201) + 0.5 * vnoise(x, y, 4.1, 202);
    ((h(x + e, y) - h(x - e, y)) * 0.9, (h(x, y + e) - h(x, y - e)) * 0.9)
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
    let (wx, wy) = crinkle_normal(x, y);
    let n = v3(n.x + wx, n.y + wy, n.z).norm();
    let albedo = mul(hex(0x5E1A1F), 0.95 + 0.1 * vnoise(x, y, 0.03, 203));
    let fall = 1.07 - 0.18 * (((x - cx) / w) * 0.45 + ((y - cy) / h) * 0.55);
    let mut c = mul(plastic(albedo, n, 0.22, 0.55), fall);
    // Shadows the raised parts cast: window bezel, plate, footswitches.
    let mut occ = 1.0;
    occ *= 1.0 - 0.45 * (1.0 - smoothstep(-2.0, 9.0, window_sd(x - 3.0, y - 5.0) - 5.0));
    occ *= 1.0 - 0.4 * (1.0 - smoothstep(-2.0, 8.0, plate_sd(x - 3.0, y - 5.0)));
    for &((fx, fy), _, _) in &FOOTS {
        occ *= 1.0 - 0.5 * (1.0 - smoothstep(-6.0, 12.0, (x - fx - 4.0).hypot(y - fy - 6.0) - FOOT_R));
    }
    c = mul(c, occ);
    c
}

fn case_wall(y: f64) -> Rgb {
    let (_, cy, _, h, _) = CASE;
    let t = ((y - (cy + h - 14.0)) / (WALL + 14.0)).clamp(0.0, 1.0);
    let n = v3(0.0, 0.85, 0.5).norm();
    mul(plastic(hex(0x4E1519), n, 0.2, 0.55), 0.78 - 0.38 * t)
}

// ---------------------------------------------------------------- deck

/// The window: a chrome bezel, then the recessed deck behind the glass.
fn window_px(x: f64, y: f64, dw: f64) -> Option<(Rgb, f64)> {
    const BEZEL: f64 = 5.0;
    if dw > BEZEL + 0.5 {
        return None;
    }
    let (cx, cy, hw, hh, r) = WINDOW;
    if dw > 0.0 {
        let (gx, gy) = grad_rrect(x, y, cx, cy, hw, hh, r);
        let t = (dw / BEZEL) * 2.0 - 1.0;
        let n = v3(gx * t * 0.9, gy * t * 0.9, (1.0 - (t * 0.9).powi(2)).sqrt());
        return Some((chrome(n, [0.86, 0.86, 0.88]), cov(dw - BEZEL)));
    }
    // The deck plate: dark brushed steel, the recess's shadows.
    let brushed = 0.9 + 0.1 * vnoise(x * 0.2, y * 6.0, 1.0, 211);
    let mut c = mul(hex(0x1C1D20), brushed);
    let n = v3(0.0, 0.0, 1.0);
    let (lit, sheen) = plastic_terms(n, 0.1, 0.4);
    c = add(mul(c, lit), [sheen * 0.4; 3]);
    c = mul(c, 1.0 - 0.55 * smoothstep(-5.0, 2.0, window_sd(x - 5.0, y - 8.0)));
    c = mul(c, 1.0 - 0.35 * (1.0 - smoothstep(0.0, 12.0, -dw)));

    // Rollers: machined aluminium discs, a hub, their shadow.
    for &(rx, ry) in &[ROLL_L, ROLL_R] {
        let (dx, dy) = (x - rx, y - ry);
        let rr = dx.hypot(dy);
        c = mul(c, 1.0 - 0.5 * (1.0 - smoothstep(-4.0, 10.0, (dx - 4.0).hypot(dy - 6.0) - ROLL_R_PT + 3.0)));
        if rr < ROLL_R_PT - 3.0 + 0.5 {
            let edge = smoothstep(ROLL_R_PT - 7.0, ROLL_R_PT - 3.0, rr);
            let (ux, uy) = (dx / rr.max(1e-6), dy / rr.max(1e-6));
            let n = v3(ux * edge * 0.8, uy * edge * 0.8, 1.0).norm();
            let kl = key();
            let th = dy.atan2(dx);
            let streak = (0.5 + 0.5 * (2.0 * (th - kl.y.atan2(kl.x))).cos()).powi(3);
            let spun = 0.3 + 0.5 * streak;
            let mut disc = mix([spun * 0.9, spun * 0.91, spun * 0.94], chrome(n, [0.85, 0.86, 0.9]), 0.3 + 0.4 * edge);
            if rr < 8.0 {
                disc = chrome(dome(dx, dy, 8.0, 1.2), [0.75, 0.75, 0.78]);
            }
            c = mix(c, disc, cov(rr - (ROLL_R_PT - 3.0)));
        }
    }
    // The head block: chrome heads with their gap, on a dark mount.
    let run_y = ROLL_L.1 + ROLL_R_PT;
    for (k, &(hx, _)) in HEAD_X.iter().enumerate() {
        let d = sd_rrect(x, y, hx, run_y + 9.0, 11.0, 13.0, 3.0);
        c = mul(c, 1.0 - 0.45 * (1.0 - smoothstep(-2.0, 6.0, sd_rrect(x - 3.0, y - 5.0, hx, run_y + 9.0, 11.0, 13.0, 3.0))));
        if d < 0.5 {
            let (gx, gy) = grad_rrect(x, y, hx, run_y + 9.0, 11.0, 13.0, 3.0);
            let b = smoothstep(-3.0, 0.0, d) * 0.85;
            let n = v3(gx * b, gy * b, 1.0).norm();
            // The erase head is a different alloy; the record/playback
            // heads are mu-metal shields with a polished face.
            let tint = if k == 0 { [0.72, 0.68, 0.62] } else { [0.88, 0.88, 0.9] };
            let mut head = chrome(n, tint);
            head = mix(head, [0.02; 3], cov((x - hx).abs() - 0.4) * cov((y - run_y - 5.0).abs() - 4.0));
            c = mix(c, head, cov(d));
        }
    }
    // Capstan (inside the loop) and the rubber pinch roller under it.
    let (dx, dy) = (x - CAPSTAN_X, y - (run_y - 7.0));
    if dx.hypot(dy) < 5.5 {
        c = mix(c, chrome(dome(dx, dy, 5.0, 1.3), [0.9, 0.9, 0.92]), cov(dx.hypot(dy) - 5.0));
    }
    let (dx, dy) = (x - CAPSTAN_X, y - (run_y + 13.0));
    let pr = dx.hypot(dy);
    c = mul(c, 1.0 - 0.45 * (1.0 - smoothstep(-2.0, 6.0, (dx - 3.0).hypot(dy - 4.0) - 11.0)));
    if pr < 11.5 {
        let rubber = plastic([0.025, 0.025, 0.028], dome(dx, dy, 11.0, 1.4), 0.25, 0.6);
        let hub = if pr < 4.0 { chrome(dome(dx, dy, 4.0, 1.2), [0.8, 0.8, 0.82]) } else { rubber };
        c = mix(c, hub, cov(pr - 11.0));
    }
    // Head lamps (unlit) under the playback heads.
    for &(hx, _) in &HEAD_X[2..] {
        if let Some((lc, a)) = lamp(x - hx - 16.0, y - HEAD_LAMP_Y, [0.09, 0.01, 0.01]) {
            c = mix(c, lc, a);
        }
    }
    Some((c, 1.0))
}

/// A small lamp lens, unlit, in a dark seat.
fn lamp(dx: f64, dy: f64, tint: Rgb) -> Option<(Rgb, f64)> {
    let rr = dx.hypot(dy);
    if rr > 5.0 {
        return None;
    }
    if rr > 3.6 {
        return Some((chrome(dome(dx, dy, 5.0, 1.0), [0.5, 0.5, 0.52]), cov(rr - 4.8)));
    }
    Some((plastic(tint, dome(dx, dy, 3.6, 1.1), 1.4, 0.04), 1.0))
}

// --------------------------------------------------------------- plate

fn plate_px(x: f64, y: f64, d: f64) -> Rgb {
    let (cx, cy, hw, hh, r) = PLATE;
    let brushed = 0.92 + 0.08 * vnoise(x * 0.15, y * 9.0, 1.0, 221) + 0.03 * (hash2((x * S) as i64, (y * S) as i64, 222) - 0.5);
    let mut n = v3(0.0, 0.0, 1.0);
    if d > -2.5 {
        let (gx, gy) = grad_rrect(x, y, cx, cy, hw, hh, r);
        let k = ((2.5 + d) / 2.5).clamp(0.0, 0.95);
        n = v3(gx * k, gy * k, (1.0 - k * k).sqrt());
    }
    let mut c = mul(chrome(n, [0.8, 0.81, 0.83]), 0.55 * brushed);
    c = add(c, [0.08; 3]);
    // An engraved border and the knob scales.
    c = mix(c, [0.06; 3], cov((d + 7.0).abs() - 0.5) * 0.8);
    for &(kx, kr, _) in &KNOBS {
        let sr = kr + 7.0;
        if (x - kx).abs() > sr + 3.0 || (y - CONTROLS_Y).abs() > sr + 3.0 {
            continue;
        }
        for k in 0..=10 {
            let a = knob_angle(f64::from(k) / 10.0);
            let dd = (x - kx - a.cos() * sr).hypot(y - CONTROLS_Y - a.sin() * sr) - if k % 5 == 0 { 1.5 } else { 0.9 };
            c = mix(c, [0.05; 3], cov(dd));
        }
    }
    for deg in HEADS_STOPS {
        let a = deg.to_radians();
        let dd = (x - HEADS_X - a.cos() * 30.0).hypot(y - CONTROLS_Y - a.sin() * 30.0) - 1.2;
        c = mix(c, [0.05; 3], cov(dd));
    }
    // Parts' contact shadows.
    let mut occ = 1.0;
    for &(kx, kr, _) in &KNOBS {
        occ *= 1.0 - 0.55 * (1.0 - smoothstep(-5.0, 12.0, (x - kx - 4.0).hypot(y - CONTROLS_Y - 7.0) - kr));
    }
    occ *= 1.0 - 0.45 * (1.0 - smoothstep(-4.0, 10.0, (x - HEADS_X - 4.0).hypot(y - CONTROLS_Y - 6.0) - 22.0));
    mul(c, occ)
}

/// A fluted black bakelite knob with a spun-aluminium cap.
fn knob(dx: f64, dy: f64, r: f64) -> Option<(Rgb, f64)> {
    let th = dy.atan2(dx);
    let flute = 1.0 + 0.045 * (th * 12.0).cos();
    let rr = dx.hypot(dy);
    let edge_r = r * flute;
    if rr > edge_r + 0.5 {
        return None;
    }
    let cap_r = r * 0.6;
    let (ux, uy) = if rr > 1e-6 { (dx / rr, dy / rr) } else { (0.0, 0.0) };
    if rr > cap_r {
        let t = ((rr - cap_r) / (edge_r - cap_r)).clamp(0.0, 1.0);
        let tilt = 0.3 + 1.0 * t * t;
        let flank = (th * 12.0).sin() * 0.35;
        let n = v3(ux * tilt.sin() - uy * flank, uy * tilt.sin() + ux * flank, tilt.cos()).norm();
        let mut c = plastic([0.018, 0.016, 0.016], n, 0.7, 0.2);
        c = mul(c, 1.0 - 0.6 * (1.0 - smoothstep(0.0, 1.2, rr - cap_r)));
        return Some((c, cov(rr - edge_r)));
    }
    let kl = key();
    let streak = (0.5 + 0.5 * (2.0 * (th - kl.y.atan2(kl.x))).cos()).powi(3);
    let rim = smoothstep(cap_r - 1.8, cap_r, rr);
    let n = v3(ux * rim * 0.8, uy * rim * 0.8, 1.0).norm();
    let spun = (0.35 + 0.55 * streak) * (0.93 + 0.07 * hash2((rr * 14.0) as i64, 2, 9));
    Some((mix([spun * 0.9, spun * 0.91, spun * 0.95], chrome(n, [0.88, 0.89, 0.92]), 0.3 + 0.4 * rim), 1.0))
}

/// The HEADS selector's aluminium skirt (the pointer is a sprite).
fn heads_base(dx: f64, dy: f64) -> Option<(Rgb, f64)> {
    let rr = dx.hypot(dy);
    if rr > 22.5 {
        return None;
    }
    let t = rr / 22.0;
    let ridge = (dy.atan2(dx) * 44.0).sin() * smoothstep(0.75, 0.85, t);
    let (ux, uy) = if rr > 1e-6 { (dx / rr, dy / rr) } else { (0.0, 0.0) };
    let tilt = 0.25 + 0.9 * t * t;
    let n = v3(ux * tilt.sin() - uy * 0.25 * ridge, uy * tilt.sin() + ux * 0.25 * ridge, tilt.cos()).norm();
    Some((chrome(n, [0.82, 0.83, 0.86]), cov(rr - 22.0)))
}

fn screw(dx: f64, dy: f64) -> Option<(Rgb, f64)> {
    const R: f64 = 5.5;
    let rr = dx.hypot(dy);
    if rr > R + 0.5 {
        return None;
    }
    let mut c = chrome(dome(dx, dy, R, 1.6), [0.85, 0.85, 0.87]);
    let a = 0.7f64;
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
    Some((mul(chrome(n, [0.82, 0.82, 0.85]), ao * (0.94 + 0.06 * (rr * 5.0).sin())), cov(dh)))
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
    let dp = plate_sd(x, y);
    if dp < 0.5 {
        c = mix(c, plate_px(x, y, dp), cov(dp));
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
    for &(kx, kr, _) in &KNOBS {
        lay(knob(x - kx, y - CONTROLS_Y, kr));
    }
    lay(heads_base(x - HEADS_X, y - CONTROLS_Y));
    for &((fx, fy), (lx, ly), _) in &FOOTS {
        lay(footswitch(x - fx, y - fy));
        lay(lamp(x - lx, y - ly, [0.1, 0.03, 0.01]));
    }
    c
}

// ------------------------------------------------------------- sprites

fn lay_px(col: &mut Rgb, alpha: &mut f64, c: Rgb, a: f64) {
    let na = a + *alpha * (1.0 - a);
    if na > 0.0 {
        for k in 0..3 {
            col[k] = (c[k] * a + col[k] * *alpha * (1.0 - a)) / na;
        }
    }
    *alpha = na;
}

/// Smoked glass: a faint tint, a broad soft reflection of the studio's
/// key light raking across, and a bright edge where the pane meets the
/// bezel. Mostly transparent — the machine shows through.
fn glass_px(dx: f64, dy: f64) -> (Rgb, f64) {
    let (_, _, hw, hh, r) = WINDOW;
    let d = sd_rrect(dx, dy, 0.0, 0.0, hw, hh, r);
    if d > 0.0 {
        return ([0.0; 3], 0.0);
    }
    // A diagonal band of reflection, upper left, and a fainter second.
    let u = (dx + hw) * 0.55 + (dy + hh) * 0.85;
    let band = (-((u - 150.0) / 60.0).powi(2)).exp() * 0.16 + (-((u - 260.0) / 18.0).powi(2)).exp() * 0.07;
    let rim = (1.0 - smoothstep(0.0, 3.0, -d)) * 0.22;
    let tint = 0.10;
    let a = (tint + band + rim).min(1.0);
    let c = mix([0.02, 0.02, 0.025], [0.95, 0.96, 1.0], ((band + rim) / a).min(1.0));
    (c, a)
}

/// The HEADS chicken-head: black bakelite, a white line.
fn chicken_sd(dx: f64, dy: f64, a: f64) -> f64 {
    let (u, v) = (dx * a.cos() + dy * a.sin(), -dx * a.sin() + dy * a.cos());
    let hub = u.hypot(v) - 10.0;
    let len = 23.0;
    let half = 7.5 * (1.0 - (u / len).clamp(0.0, 1.0)) + 2.0;
    let blade = (v.abs() - half).max(u - len).max(-u);
    let tail = (u + 4.0).hypot(v) - 8.5;
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
        let b = smoothstep(-4.0, 0.0, d) * 0.85;
        let n = v3(gx / gl * b, gy / gl * b, 1.0).norm();
        let mut c = plastic([0.02, 0.018, 0.018], n, 0.8, 0.18);
        let (u, v) = (dx * a.cos() + dy * a.sin(), -dx * a.sin() + dy * a.cos());
        if u > 5.0 && u < 21.0 {
            c = mix(c, [0.9, 0.88, 0.84], cov(v.abs() - 0.9));
        }
        lay_px(&mut col, &mut alpha, c, cov(d));
    }
    (col, alpha)
}

fn glow_px(dx: f64, dy: f64, color: Rgb) -> (Rgb, f64) {
    let rr = dx.hypot(dy);
    let halo = 0.5 * (-(rr / 6.5).powi(2)).exp() + 0.2 * (-(rr / 15.0).powi(2)).exp();
    let lens = cov(rr - 3.6);
    let hot = (1.0 - rr / 3.6).clamp(0.0, 1.0);
    let lens_c = mix(mul(color, 1.4), [2.1, 1.9, 1.7], hot * hot);
    let spec = cov((dx + 1.1).hypot(dy + 1.3) - 0.7);
    let c = mix(mix(mul(color, 1.2), lens_c, lens), [3.0; 3], spec * 0.6);
    let _ = TAU;
    (c, (halo + lens).min(1.0))
}

#[cfg(test)]
mod preview {
    /// Dump the bake to build/ribbon-face.ppm (ignored; for eyes).
    #[test]
    #[ignore]
    fn dump_face() {
        let img = &super::art().face;
        let mut out = format!("P6 {} {} 255\n", img.px_w, img.px_h).into_bytes();
        for px in img.rgba.chunks(4) {
            out.extend_from_slice(&px[..3]);
        }
        std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/../../build/ribbon-face.ppm"), out).unwrap();
    }
}
