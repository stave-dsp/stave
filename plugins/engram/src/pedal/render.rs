//! The bake — engram photographed, in software, with the shared studio
//! (`stave_ui::photo`): an indigo ANODIZED case (dyed aluminium — a
//! fine bead-blast grain under a coloured, faintly metallic sheen, the
//! bevel catching bright where the dye is thinnest); a vacuum-
//! fluorescent display behind smoked glass, its fine control-grid mesh
//! and unlit segments faintly visible as on a real VFD; machined
//! aluminium knobs; chrome footswitches. A second, square VFD holds the
//! SPACE map — every learned room a faint unlit dot. Sprites: the two
//! glasses, the lamps.

use std::sync::OnceLock;

use stave_ui::photo::*;
use stave_ui::scene::Image;

use super::{cell, hear_slot, knob_angle, map_screen, strip_segment, to_screen, CASE, DISPLAY, FOOTS, FOOT_R, FORGET, FORGET_R, H, KNOBS, KNOB_R, KNOB_Y, MAP, MAP_WINDOW, SCREWS, SLOTS, STRIP_SEGS, W};
use crate::engine::space_model::{MAP_XY, SPACES};

const S: f64 = 2.0;
pub(super) const LAMP_SPRITE: (f64, f64) = (40.0, 40.0);
const WALL: f64 = 10.0;

pub(super) struct Art {
    pub face: Image,
    pub glass: Image,
    pub map_glass: Image,
    pub lamp_red: Image,
    pub lamp_cyan: Image,
}

pub(super) fn art() -> &'static Art {
    static ART: OnceLock<Art> = OnceLock::new();
    ART.get_or_init(|| {
        let (_, _, hw, hh, _) = DISPLAY;
        let (_, _, mhw, mhh, _) = MAP_WINDOW;
        Art {
            face: bake(W, H, S, face_px),
            glass: bake_sprite((2.0 * hw, 2.0 * hh), S, |x, y| glass_px(x, y, DISPLAY)),
            map_glass: bake_sprite((2.0 * mhw, 2.0 * mhh), S, |x, y| glass_px(x, y, MAP_WINDOW)),
            lamp_red: bake_sprite(LAMP_SPRITE, S, |x, y| lamp_px(x, y, hex(0xFF2A1A))),
            lamp_cyan: bake_sprite(LAMP_SPRITE, S, |x, y| lamp_px(x, y, hex(0x5CFFE0))),
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

fn display_sd(x: f64, y: f64) -> f64 {
    let (cx, cy, hw, hh, r) = DISPLAY;
    sd_rrect(x, y, cx, cy, hw, hh, r)
}

fn map_sd(x: f64, y: f64) -> f64 {
    let (cx, cy, hw, hh, r) = MAP_WINDOW;
    sd_rrect(x, y, cx, cy, hw, hh, r)
}

/// The map's dots, bucketed on a 16 × 16 grid (for the bake).
fn map_buckets() -> &'static Vec<Vec<(f64, f64)>> {
    static B: OnceLock<Vec<Vec<(f64, f64)>>> = OnceLock::new();
    B.get_or_init(|| {
        let mut b = vec![Vec::new(); 256];
        for i in 0..SPACES {
            let (u, v) = (f64::from(MAP_XY[2 * i]), f64::from(MAP_XY[2 * i + 1]));
            let p = map_screen(u, v);
            b[((v * 16.0) as usize).min(15) * 16 + ((u * 16.0) as usize).min(15)].push((p.x, p.y));
        }
        b
    })
}

/// The SPACE window: chrome bezel, black well, mesh, a faint frame, and
/// every learned room as an unlit phosphor dot.
fn map_display(x: f64, y: f64) -> Option<(Rgb, f64)> {
    let d = map_sd(x, y);
    if d > 5.5 {
        return None;
    }
    if d > 0.0 {
        let (cx, cy, hw, hh, r) = MAP_WINDOW;
        let (gx, gy) = grad_rrect(x, y, cx, cy, hw, hh, r);
        let t = (d / 5.0) * 2.0 - 1.0;
        let n = v3(gx * t * 0.9, gy * t * 0.9, (1.0 - (t * 0.9).powi(2)).sqrt());
        return Some((chrome(n, [0.8, 0.8, 0.86]), cov(d - 5.0)));
    }
    let mut c = [0.006, 0.014, 0.014];
    let (u, v) = (x * 1.2, y * 1.2 * 1.1547);
    let mesh = ((u + 0.5 * v.floor()).fract() - 0.5).abs().min(((v).fract() - 0.5).abs());
    c = add(c, mul([0.01, 0.025, 0.022], 1.0 - smoothstep(0.0, 0.12, mesh)));
    let (x0, y0, s) = MAP;
    // A faint frame round the map.
    let frame = sd_rrect(x, y, x0 + s / 2.0, y0 + s / 2.0, s / 2.0 + 4.0, s / 2.0 + 4.0, 3.0).abs() - 0.35;
    c = add(c, mul([0.02, 0.05, 0.042], cov(frame)));
    // The rooms.
    let (bu, bv) = (((x - x0) / s * 16.0).floor() as i64, ((y - y0) / s * 16.0).floor() as i64);
    let mut dot = f64::MAX;
    for jv in bv - 1..=bv + 1 {
        for ju in bu - 1..=bu + 1 {
            if (0..16).contains(&ju) && (0..16).contains(&jv) {
                for &(px, py) in &map_buckets()[(jv * 16 + ju) as usize] {
                    dot = dot.min((x - px).hypot(y - py) - 0.9);
                }
            }
        }
    }
    c = add(c, mul([0.05, 0.12, 0.1], cov(dot - 0.15)));
    c = mul(c, 1.0 - 0.6 * smoothstep(-6.0, 1.0, map_sd(x - 3.0, y - 5.0)));
    Some((c, 1.0))
}

fn dome(dx: f64, dy: f64, r: f64, flat: f64) -> V3 {
    let rr = dx.hypot(dy);
    let k = (rr / (r * flat)).min(0.99);
    let (ux, uy) = if rr > 1e-6 { (dx / rr, dy / rr) } else { (0.0, 0.0) };
    v3(ux * k, uy * k, (1.0 - k * k).sqrt())
}

fn desk(x: f64, y: f64) -> Rgb {
    let felt = 0.85 + 0.15 * vnoise(x, y, 0.9, 7) + 0.08 * (hash2((x * S) as i64, (y * S) as i64, 3) - 0.5);
    let pool = (-(((x - 170.0) / 440.0).powi(2) + ((y - 60.0) / 360.0).powi(2))).exp();
    let vign = 1.0 - 0.45 * smoothstep(380.0, 760.0, ((x - W / 2.0).powi(2) + (y - 240.0).powi(2)).sqrt());
    let base = mul(hex(0x232427), felt * (0.55 + 0.7 * pool) * vign);
    let shadow = 1.0 - 0.78 * (1.0 - smoothstep(-16.0, 50.0, case_sd(x - 11.0, y - 21.0)));
    let contact = 1.0 - 0.6 * (1.0 - smoothstep(0.0, 6.0, case_sd(x, y).min(case_sd(x, y - WALL))));
    mul(base, shadow * contact)
}

/// Indigo anodizing: dye over bead-blasted aluminium.
fn anodized(n: V3, x: f64, y: f64) -> Rgb {
    let grain = 0.92 + 0.08 * hash2((x * S) as i64, (y * S) as i64, 401) + 0.04 * (vnoise(x, y, 0.08, 402) - 0.5);
    let dye = mul(hex(0x1C1752), grain);
    let paint = plastic(dye, n, 0.05, 0.4);
    // The metal under the dye shows as a coloured sheen.
    let metal = chrome(n, [0.16, 0.14, 0.42]);
    add(mul(paint, 0.9), mul(metal, 0.35))
}

fn case_px(x: f64, y: f64, d: f64) -> Rgb {
    const BEVEL: f64 = 9.0;
    let (cx, cy, w, h, r) = CASE;
    let mut n = v3(0.0, 0.0, 1.0);
    if d > -BEVEL {
        let (gx, gy) = grad_rrect(x, y, cx + w / 2.0, cy + h / 2.0, w / 2.0, h / 2.0, r);
        let k = ((BEVEL + d) / BEVEL).clamp(0.0, 0.999);
        n = v3(gx * k, gy * k, (1.0 - k * k).sqrt());
    }
    let fall = 1.07 - 0.18 * (((x - cx) / w) * 0.45 + ((y - cy) / h) * 0.55);
    let mut c = mul(anodized(n, x, y), fall);
    let mut occ = 1.0;
    occ *= 1.0 - 0.45 * (1.0 - smoothstep(-2.0, 9.0, display_sd(x - 4.0, y - 6.0) - 5.0));
    occ *= 1.0 - 0.45 * (1.0 - smoothstep(-2.0, 9.0, map_sd(x - 4.0, y - 6.0) - 5.0));
    for &(kx, _) in &KNOBS {
        occ *= 1.0 - 0.55 * (1.0 - smoothstep(-5.0, 13.0, (x - kx - 4.5).hypot(y - KNOB_Y - 7.5) - KNOB_R));
    }
    for &((fx, fy), _, _) in &FOOTS {
        occ *= 1.0 - 0.5 * (1.0 - smoothstep(-6.0, 12.0, (x - fx - 4.0).hypot(y - fy - 6.0) - FOOT_R));
    }
    c = mul(c, occ);
    // Laser-etched knob scales (anodizing etches to bright metal).
    for &(kx, _) in &KNOBS {
        let sr = KNOB_R + 8.0;
        if (x - kx).abs() > sr + 3.0 || (y - KNOB_Y).abs() > sr + 3.0 {
            continue;
        }
        for k in 0..=10 {
            let a = knob_angle(f64::from(k) / 10.0);
            let dd = (x - kx - a.cos() * sr).hypot(y - KNOB_Y - a.sin() * sr) - if k % 5 == 0 { 1.5 } else { 0.9 };
            c = mix(c, [0.75, 0.76, 0.85], cov(dd) * 0.9);
        }
    }
    c
}

fn case_wall(y: f64) -> Rgb {
    let (_, cy, _, h, _) = CASE;
    let t = ((y - (cy + h - 14.0)) / (WALL + 14.0)).clamp(0.0, 1.0);
    let n = v3(0.0, 0.85, 0.5).norm();
    mul(anodized(n, 0.0, y), 0.8 - 0.4 * t)
}

/// The VFD: a black-glass well with a chrome bezel, the fine control
/// grid, and every segment faintly visible unlit.
fn display(x: f64, y: f64) -> Option<(Rgb, f64)> {
    let d = display_sd(x, y);
    if d > 5.5 {
        return None;
    }
    if d > 0.0 {
        let (cx, cy, hw, hh, r) = DISPLAY;
        let (gx, gy) = grad_rrect(x, y, cx, cy, hw, hh, r);
        let t = (d / 5.0) * 2.0 - 1.0;
        let n = v3(gx * t * 0.9, gy * t * 0.9, (1.0 - (t * 0.9).powi(2)).sqrt());
        return Some((chrome(n, [0.8, 0.8, 0.86]), cov(d - 5.0)));
    }
    let mut c = [0.006, 0.014, 0.014];
    // The control grid: a fine hex mesh over everything.
    let (u, v) = (x * 1.2, y * 1.2 * 1.1547);
    let mesh = ((u + 0.5 * v.floor()).fract() - 0.5).abs().min(((v).fract() - 0.5).abs());
    c = add(c, mul([0.01, 0.025, 0.022], 1.0 - smoothstep(0.0, 0.12, mesh)));
    // The field's guides, unlit: rings round the centre (equal line
    // length) and a crosshair — phosphor that is merely there.
    let centre = to_screen(0.5, 0.5);
    let edge = to_screen(1.0, 1.0);
    let (rx, ry) = (edge.x - centre.x, edge.y - centre.y);
    let er = ((x - centre.x) / rx).hypot((y - centre.y) / ry);
    let mut guide: f64 = 0.0;
    for ring in [0.33, 0.66, 1.0] {
        guide = guide.max(1.0 - smoothstep(0.0, 0.006, (er - ring).abs()));
    }
    if (x - centre.x).abs() < 0.6 || (y - centre.y).abs() < 0.6 {
        guide = guide.max(0.6 * (1.0 - smoothstep(0.9, 1.0, er)));
    }
    c = add(c, mul([0.02, 0.05, 0.042], guide));
    let mut seg_d = f64::MAX;
    // The memory cells and hearing slots, unlit.
    for k in 0..SLOTS {
        let (sx, sy, sw, sh) = cell(k);
        seg_d = seg_d.min(sd_rrect(x, y, sx + sw / 2.0, sy + sh / 2.0, sw / 2.0, sh / 2.0, 1.2));
    }
    for k in 0..12 {
        let (sx, sy, sw, sh) = hear_slot(k);
        seg_d = seg_d.min(sd_rrect(x, y, sx + sw / 2.0, sy + sh / 2.0, sw / 2.0, sh / 2.0, 0.8));
    }
    for k in 0..2 {
        for s in 0..STRIP_SEGS {
            let (sx, sy, sw, sh) = strip_segment(k, s);
            seg_d = seg_d.min(sd_rrect(x, y, sx + sw / 2.0, sy + sh / 2.0, sw / 2.0, sh / 2.0, 0.8));
        }
    }
    c = add(c, mul([0.007, 0.018, 0.015], cov(seg_d)));
    // The PULSE lamp's unlit dot.
    c = add(c, mul([0.03, 0.02, 0.008], cov((x - super::PULSE_AT.0).hypot(y - super::PULSE_AT.1) - 2.2)));
    // The bezel's shadow inside the well.
    c = mul(c, 1.0 - 0.6 * smoothstep(-6.0, 1.0, display_sd(x - 3.0, y - 5.0)));
    Some((c, 1.0))
}

/// A machined aluminium knob: a knurled barrel, a polished top with
/// lathe rings.
fn knob(dx: f64, dy: f64) -> Option<(Rgb, f64)> {
    let r = dx.hypot(dy);
    if r > KNOB_R + 0.5 {
        return None;
    }
    let top = KNOB_R * 0.8;
    let (ux, uy) = if r > 1e-6 { (dx / r, dy / r) } else { (0.0, 0.0) };
    if r > top {
        let t = ((r - top) / (KNOB_R - top)).clamp(0.0, 1.0);
        let tilt = 0.5 + 0.9 * t * t;
        let ridge = (dy.atan2(dx) * 60.0).sin() * 0.3;
        let n = v3(ux * tilt.sin() - uy * ridge, uy * tilt.sin() + ux * ridge, tilt.cos()).norm();
        return Some((mul(chrome(n, [0.84, 0.85, 0.9]), 0.9), cov(r - KNOB_R)));
    }
    let kl = key();
    let th = dy.atan2(dx);
    let streak = (0.5 + 0.5 * (2.0 * (th - kl.y.atan2(kl.x))).cos()).powi(3);
    let rings = 0.93 + 0.07 * hash2((r * 12.0) as i64, 3, 9);
    let spun = (0.38 + 0.55 * streak) * rings;
    let rim = smoothstep(top - 2.0, top, r);
    let n = v3(ux * rim * 0.8, uy * rim * 0.8, 1.0).norm();
    Some((mix([spun * 0.9, spun * 0.91, spun * 0.95], chrome(n, [0.88, 0.89, 0.93]), 0.3 + 0.4 * rim), 1.0))
}

fn screw(dx: f64, dy: f64) -> Option<(Rgb, f64)> {
    const R: f64 = 5.0;
    let rr = dx.hypot(dy);
    if rr > R + 0.5 {
        return None;
    }
    // A hex socket cap screw, black.
    let mut c = chrome(dome(dx, dy, R, 1.6), [0.25, 0.25, 0.3]);
    c = mix(c, [0.01; 3], cov(sd_hex(dx, dy, 1.8)));
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
        return Some((chrome(dome(dx, dy, 6.0, 1.0), [0.6, 0.6, 0.66]), cov(rr - 5.8)));
    }
    Some((plastic([0.04, 0.04, 0.06], dome(dx, dy, 4.2, 1.1), 1.3, 0.04), 1.0))
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
    lay(display(x, y));
    lay(map_display(x, y));
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
    lay(forget_button(x - FORGET.0, y - FORGET.1));
    c
}

/// The FORGET button: a small recessed chrome push in a black collar.
fn forget_button(dx: f64, dy: f64) -> Option<(Rgb, f64)> {
    let rr = dx.hypot(dy);
    if rr > FORGET_R + 2.5 {
        return None;
    }
    if rr > FORGET_R {
        return Some(([0.01, 0.01, 0.015], cov(rr - FORGET_R - 2.0)));
    }
    Some((chrome(dome(dx, dy, FORGET_R, 1.6), [0.85, 0.86, 0.9]), 1.0))
}

/// Smoked glass over the VFD: tint, a raking reflection, a rim glint.
fn glass_px(dx: f64, dy: f64, window: (f64, f64, f64, f64, f64)) -> (Rgb, f64) {
    let (_, _, hw, hh, r) = window;
    let d = sd_rrect(dx, dy, 0.0, 0.0, hw, hh, r);
    if d > 0.0 {
        return ([0.0; 3], 0.0);
    }
    let u = (dx + hw) * 0.5 + (dy + hh) * 0.86;
    let band = (-((u - 120.0) / 50.0).powi(2)).exp() * 0.12 + (-((u - 210.0) / 14.0).powi(2)).exp() * 0.06;
    let rim = (1.0 - smoothstep(0.0, 3.0, -d)) * 0.2;
    let a = (0.06 + band + rim).min(1.0);
    (mix([0.0; 3], [0.92, 0.95, 1.0], ((band + rim) / a).min(1.0)), a)
}

fn lamp_px(dx: f64, dy: f64, color: Rgb) -> (Rgb, f64) {
    let rr = dx.hypot(dy);
    let halo = 0.5 * (-(rr / 6.5).powi(2)).exp() + 0.2 * (-(rr / 15.0).powi(2)).exp();
    let lens = cov(rr - 4.2);
    let hot = (1.0 - rr / 4.2).clamp(0.0, 1.0);
    let c = mix(mul(color, 1.2), mix(mul(color, 1.4), [2.1, 2.0, 1.9], hot * hot), lens);
    (c, (halo + lens).min(1.0))
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
        std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/../../build/engram-face.ppm"), out).unwrap();
    }
}
