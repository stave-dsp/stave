//! The bake — the pedal photographed, in software: every pixel of the
//! enclosure, knobs, footswitches and LED bezels is SHADED (height
//! profiles → normals → the studio in shade.rs), at 2× so Retina is
//! sharp. Moving parts are separate sprites (toggle levers per
//! position, LED glows) or live vectors (mod.rs), so the expensive
//! picture is baked exactly once per process (`art()` memoizes).

extern crate alloc;
use alloc::sync::Arc;
use alloc::vec::Vec;
use std::sync::OnceLock;

use stave_ui::scene::Image;

use super::shade::*;
use super::{knob_angle, CAP_R, FOOT, FOOT_R, H, KNOBS, KNOB_R, PEDAL, RECORD, SCALE_R, TOGGLES, W};

/// Pixels per point.
const S: f64 = 2.0;
/// Sprite sizes in points.
pub(super) const TOGGLE_SPRITE: (f64, f64) = (48.0, 76.0);
pub(super) const GLOW_SPRITE: (f64, f64) = (64.0, 64.0);

/// The baked pictures.
pub(super) struct Art {
    pub face: Image,
    /// The lever at up / centre / down.
    pub toggle: [Image; 3],
    /// Lit LED glows: bypass (red), hold (amber).
    pub glow: [Image; 2],
}

pub(super) fn art() -> &'static Art {
    static ART: OnceLock<Art> = OnceLock::new();
    ART.get_or_init(|| Art {
        face: bake(W, H, face_px),
        toggle: [bake_sprite(TOGGLE_SPRITE, |x, y| toggle_px(x, y, 0)),
                 bake_sprite(TOGGLE_SPRITE, |x, y| toggle_px(x, y, 1)),
                 bake_sprite(TOGGLE_SPRITE, |x, y| toggle_px(x, y, 2))],
        glow: [bake_sprite(GLOW_SPRITE, |x, y| glow_px(x, y, hex(0xFF3A22))),
               bake_sprite(GLOW_SPRITE, |x, y| glow_px(x, y, hex(0xFFB03A)))],
    })
}

fn bake(w: f64, h: f64, f: fn(f64, f64) -> Rgb) -> Image {
    stave_ui::photo::bake(w, h, S, f)
}

fn bake_sprite(size: (f64, f64), f: impl Fn(f64, f64) -> (Rgb, f64)) -> Image {
    stave_ui::photo::bake_sprite(size, S, f)
}

/// Edge coverage from a signed distance (negative inside), AA'd over
/// one pixel.
fn cover(d: f64) -> f64 {
    stave_ui::photo::cover(d, S)
}

// ------------------------------------------------------------ the face

fn pedal_sd(x: f64, y: f64) -> f64 {
    let (px, py, pw, ph, r) = PEDAL;
    sd_rrect(x, y, px + pw / 2.0, py + ph / 2.0, pw / 2.0, ph / 2.0, r)
}

/// The desk the pedal sits on: charcoal felt under a warm key pool,
/// with the enclosure's soft shadow and its contact darkening.
fn desk(x: f64, y: f64) -> Rgb {
    let felt = 0.85 + 0.15 * vnoise(x, y, 0.9, 7) + 0.08 * (hash2((x * S) as i64, (y * S) as i64, 3) - 0.5);
    let pool = (-(((x - 120.0) / 380.0).powi(2) + ((y - 60.0) / 420.0).powi(2))).exp();
    let vign = 1.0 - 0.45 * smoothstep(250.0, 560.0, ((x - 260.0).powi(2) + (y - 330.0).powi(2)).sqrt());
    let base = mul(hex(0x2A2A2E), felt * (0.55 + 0.7 * pool) * vign);
    let shadow = 1.0 - 0.75 * (1.0 - smoothstep(-16.0, 46.0, pedal_sd(x - 10.0, y - 20.0)));
    let contact = 1.0 - 0.6 * (1.0 - smoothstep(0.0, 6.0, pedal_sd(x, y).min(pedal_sd(x, y - WALL))));
    mul(base, shadow * contact)
}

/// The record graphic's warped edge radius at angle `th`.
fn record_edge(th: f64) -> f64 {
    RECORD.2 + 2.6 * (3.0 * th + 0.7).sin() + 1.1 * (5.0 * th - 1.1).sin()
}

/// The printed face: cream powder-coat, the rust pinstripe, the
/// warped record, the knob scales. (Paint only — lighting is later.)
/// `print(r, θ, paper)` paints the label (see `label_print`).
fn albedo(x: f64, y: f64, d: f64, print: &dyn Fn(f64, f64, f64) -> Rgb) -> Rgb {
    let mottle = 1.0 + 0.05 * (vnoise(x, y, 0.06, 11) - 0.5) + 0.04 * (vnoise(x, y, 0.6, 12) - 0.5);
    let mut c = mul(hex(0xEADFC6), mottle);

    // Rust pinstripe, inset.
    let stripe = cover((d + 12.0).abs() - 0.8);
    c = mix(c, hex(0xA9542E), stripe * 0.9);

    // The warped record, rising like a sun from the bottom edge.
    let (rx, ry, _, lab) = RECORD;
    let (dx, dy) = (x - rx, y - ry);
    let r = dx.hypot(dy);
    let th = dy.atan2(dx);
    let edge = record_edge(th);
    let on_record = cover(r - edge);
    if on_record > 0.0 {
        let kl = key();
        let az = kl.y.atan2(kl.x);
        let aniso = (0.5 + 0.5 * (2.0 * (th - az)).cos()).powi(5);
        let grooves = 0.65 + 0.35 * (r * 3.3).sin() * (0.6 + 0.4 * hash2((r * 6.0) as i64, 0, 5));
        let gap = [118.0, 146.0, 172.0].iter().fold(1.0f64, |g, &gr| g.min(0.35 + 0.65 * smoothstep(0.6, 1.8, (r - gr).abs())));
        let lead = smoothstep(lab + 6.0, lab + 14.0, r);
        let sheen = 0.012 + (0.05 * aniso * grooves * lead) * gap + 0.01 * (1.0 - lead);
        let mut rec = [sheen * 0.95, sheen * 0.97, sheen * 1.05];
        // A teal glint band along the grooves (the print's one cool note).
        let glint = (-((r - 160.0) / 7.0).powi(2)).exp() * aniso;
        rec = add(rec, mul(hex(0x2F7F78), 0.25 * glint));
        // The label (printed, so it turns — see `label_print`).
        let on_label = cover(r - lab);
        let paper = 1.0 + 0.06 * (vnoise(x, y, 0.4, 21) - 0.5);
        let label = print(r, th, paper);
        rec = mix(rec, label, on_label);
        rec = mix(rec, [0.01, 0.01, 0.01], cover(r - 3.5));
        c = mix(c, rec, on_record);
    }

    // Knob scales: eleven dots, the ends a touch larger.
    for &(kx, ky) in &KNOBS {
        if (x - kx).abs() > SCALE_R + 4.0 || (y - ky).abs() > SCALE_R + 4.0 {
            continue;
        }
        for k in 0..=10 {
            let a = knob_angle(f64::from(k) / 10.0);
            let (tx, ty) = (kx + a.cos() * SCALE_R, ky + a.sin() * SCALE_R);
            let rad = if k == 0 || k == 10 { 1.9 } else { 1.25 };
            c = mix(c, hex(0x2B2622), cover((x - tx).hypot(y - ty) - rad));
        }
    }
    c
}

/// The label's print at radius `r`, angle `a` (already turned by the
/// spin): two halves split on a diameter, a cream rim, a ring of
/// dots, a spoke. The groove sheen is the LIGHT, so it never turns.
fn label_print(r: f64, a: f64, paper: f64) -> Rgb {
    use core::f64::consts::TAU;
    let lab = RECORD.3;
    let split = (0.5 + 0.5 * (a.sin() * r / 0.6).clamp(-1.0, 1.0)).clamp(0.0, 1.0);
    let mut c = mul(mix(hex(0xB4532A), hex(0xD9A23A), split), paper);
    c = mix(c, hex(0xEADFC6), cover((r - (lab - 6.0)).abs() - 1.1));
    let dot_a = (a / (TAU / 24.0)).round() * (TAU / 24.0);
    let (dx, dy) = (r * a.cos() - (lab - 15.0) * dot_a.cos(), r * a.sin() - (lab - 15.0) * dot_a.sin());
    c = mix(c, hex(0x3A1C10), cover(dx.hypot(dy) - 1.6));
    if (a - 0.9).cos() > 0.0 && r > 18.0 && r < lab - 22.0 {
        c = mix(c, hex(0xEADFC6), cover((r * (a - 0.9).sin()).abs() - 1.4));
    }
    c
}

/// The label at rest (the bake's own print).
fn print_at_rest(r: f64, a: f64, paper: f64) -> Rgb {
    label_print(r, a, paper)
}

/// The enclosure's top: printed albedo under the bevel's normals.
fn enclosure(x: f64, y: f64, d: f64, print: &dyn Fn(f64, f64, f64) -> Rgb) -> Rgb {
    let (lit, sheen, k) = face_light(x, y, d);
    mul(add(mul(albedo(x, y, d, print), lit), [sheen; 3]), k)
}

/// The face's lighting at a point, independent of what is printed
/// there: (albedo gain, sheen, falloff × contact shadows).
fn face_light(x: f64, y: f64, d: f64) -> (f64, f64, f64) {
    const BEVEL: f64 = 9.0;
    let (px, py, pw, ph, r) = PEDAL;
    let n = if d > -BEVEL {
        let (gx, gy) = grad_rrect(x, y, px + pw / 2.0, py + ph / 2.0, pw / 2.0, ph / 2.0, r);
        let k = ((BEVEL + d) / BEVEL).clamp(0.0, 0.999);
        let hz = (1.0 - k * k).sqrt();
        v3(gx * k, gy * k, hz)
    } else {
        v3(0.0, 0.0, 1.0)
    };
    // Powder-coat orange peel: a whisper of normal grain.
    let gx = (vnoise(x, y, 1.6, 31) - 0.5) * 0.05;
    let gy = (vnoise(x, y, 1.6, 32) - 0.5) * 0.05;
    let n = v3(n.x + gx, n.y + gy, n.z).norm();
    let fall = 1.08 - 0.2 * (((x - px) / pw) * 0.4 + ((y - py) / ph) * 0.6);
    let (lit, sheen) = plastic_terms(n, 0.10, 0.5);

    // Contact shadows the parts cast on the face.
    let mut occ = 1.0;
    for &(kx, ky) in &KNOBS {
        let ds = (x - kx - 4.5).hypot(y - ky - 7.0) - KNOB_R;
        occ *= 1.0 - 0.55 * (1.0 - smoothstep(-6.0, 15.0, ds));
        let dc = (x - kx).hypot(y - ky) - KNOB_R;
        occ *= 1.0 - 0.35 * (1.0 - smoothstep(0.0, 3.5, dc));
    }
    for &((fx, fy), (lx, ly)) in &FOOT {
        let ds = (x - fx - 4.0).hypot(y - fy - 6.0) - (FOOT_R - 2.0);
        occ *= 1.0 - 0.5 * (1.0 - smoothstep(-6.0, 14.0, ds));
        let dl = (x - lx - 1.5).hypot(y - ly - 2.5) - 7.5;
        occ *= 1.0 - 0.4 * (1.0 - smoothstep(-2.0, 4.0, dl));
    }
    (lit, sheen, fall * occ)
}

/// A knob at local (dx, dy): black knurled skirt, spun-aluminium cap.
fn knob(dx: f64, dy: f64) -> Rgb {
    let r = dx.hypot(dy).max(1e-6);
    let th = dy.atan2(dx);
    let (ux, uy) = (dx / r, dy / r);
    if r > CAP_R {
        // The skirt: a rounded chamfer falling away, knurled.
        let t = ((r - CAP_R) / (KNOB_R - CAP_R)).clamp(0.0, 1.0);
        let tilt = 0.35 + 1.05 * t * t;
        let ridge = (th * 44.0).sin();
        let (tx, ty) = (-uy, ux);
        let n = v3(ux * tilt.sin() + tx * 0.32 * ridge, uy * tilt.sin() + ty * 0.32 * ridge, tilt.cos()).norm();
        let mut c = plastic([0.022, 0.022, 0.024], n, 0.55, 0.35);
        c = add(c, mul(chrome(n, [1.0, 1.0, 1.0]), 0.05));
        // The groove where cap meets skirt.
        let groove = 1.0 - 0.8 * (1.0 - smoothstep(0.0, 1.4, r - CAP_R));
        mul(c, groove * (1.0 - 0.35 * smoothstep(0.8, 1.0, t)))
    } else {
        // The cap: spun aluminium, faintly domed, with a bright
        // chamfered rim.
        let rim = smoothstep(CAP_R - 2.2, CAP_R - 0.3, r);
        let dome = 0.12;
        let n = v3(ux * (dome * r / CAP_R + rim * 0.9), uy * (dome * r / CAP_R + rim * 0.9), 1.0).norm();
        let kl = key();
        let az = kl.y.atan2(kl.x);
        let streak = (0.5 + 0.5 * (2.0 * (th - az)).cos()).powi(3);
        let rings = 0.92 + 0.08 * hash2((r * 14.0) as i64, 1, 9);
        let spun = (0.32 + 0.62 * streak) * rings;
        let metal = mix([spun * 0.86, spun * 0.88, spun * 0.92], chrome(n, [0.9, 0.91, 0.95]), 0.35 + 0.4 * rim);
        let centre = 1.0 - 0.25 * (1.0 - smoothstep(0.0, 2.2, r));
        mul(metal, centre)
    }
}

/// A footswitch at local (dx, dy): chrome hex nut + domed button.
fn footswitch(dx: f64, dy: f64) -> Option<(Rgb, f64)> {
    let r = dx.hypot(dy);
    const BTN: f64 = 14.5;
    if r < BTN + 1.0 {
        let k = (r / (BTN * 1.7)).min(0.99);
        let (ux, uy) = if r > 1e-6 { (dx / r, dy / r) } else { (0.0, 0.0) };
        let edge = smoothstep(BTN - 1.6, BTN, r);
        let n = v3(ux * (k + edge * 0.8), uy * (k + edge * 0.8), (1.0 - k * k).sqrt()).norm();
        let c = mul(chrome(n, [0.93, 0.93, 0.96]), 1.0 - 0.5 * edge);
        return Some((c, cover(r - BTN)));
    }
    let dh = sd_hex(dx, dy, FOOT_R - 3.0);
    if dh < 0.5 {
        let e = 0.3;
        let gx = sd_hex(dx + e, dy, FOOT_R - 3.0) - sd_hex(dx - e, dy, FOOT_R - 3.0);
        let gy = sd_hex(dx, dy + e, FOOT_R - 3.0) - sd_hex(dx, dy - e, FOOT_R - 3.0);
        let gl = gx.hypot(gy).max(1e-9);
        let b = smoothstep(-2.4, 0.0, dh) * 0.85;
        let n = v3(gx / gl * b, gy / gl * b, 1.0).norm();
        // Machined top: faint concentric tooling marks.
        let tool = 0.94 + 0.06 * (r * 5.0).sin();
        let c = mul(chrome(n, [0.82, 0.82, 0.85]), tool);
        // The button's shadow ring on the nut.
        let ao = 1.0 - 0.5 * (1.0 - smoothstep(0.0, 3.0, r - BTN));
        return Some((mul(c, ao), cover(dh)));
    }
    None
}

/// An LED bezel (unlit): chrome ring around a dark glossy lens.
fn led(dx: f64, dy: f64, lens_tint: Rgb) -> Option<(Rgb, f64)> {
    let r = dx.hypot(dy);
    if r > 7.6 {
        return None;
    }
    let (ux, uy) = if r > 1e-6 { (dx / r, dy / r) } else { (0.0, 0.0) };
    if r > 5.0 {
        let t = ((r - 6.3) / 1.3).clamp(-1.0, 1.0);
        let n = v3(ux * t, uy * t, (1.0 - t * t).sqrt()).norm();
        return Some((chrome(n, [0.88, 0.88, 0.9]), cover(r - 7.5)));
    }
    let k = r / 7.5;
    let n = v3(ux * k, uy * k, (1.0 - k * k).sqrt());
    let c = plastic(lens_tint, n, 1.6, 0.03);
    Some((c, 1.0))
}

/// How much of the enclosure's front wall the camera sees, points.
const WALL: f64 = 7.0;

/// The front side wall: the same powder-coat, turned away from the
/// key light, darkening toward the desk.
fn side_wall(y: f64) -> Rgb {
    let (_, py, _, ph, _) = PEDAL;
    let t = ((y - (py + ph - 12.0)) / (WALL + 12.0)).clamp(0.0, 1.0);
    let n = v3(0.0, 0.85, 0.5).norm();
    let c = plastic(mul(hex(0xEADFC6), 0.9), n, 0.08, 0.6);
    mul(c, 0.75 - 0.35 * t)
}

fn face_px(x: f64, y: f64) -> Rgb {
    let d = pedal_sd(x, y);
    let mut c = desk(x, y);
    let dw = pedal_sd(x, y - WALL);
    if dw < 1.0 && d > -1.0 {
        c = mix(c, side_wall(y), cover(dw));
    }
    if d >= 1.0 {
        return c;
    }
    c = mix(c, enclosure(x, y, d, &print_at_rest), cover(d));
    for &(kx, ky) in &KNOBS {
        let (dx, dy) = (x - kx, y - ky);
        let r = dx.hypot(dy);
        if r < KNOB_R + 1.0 {
            c = mix(c, knob(dx, dy), cover(r - KNOB_R));
        }
    }
    for (i, &((fx, fy), (lx, ly))) in FOOT.iter().enumerate() {
        if let Some((fc, a)) = footswitch(x - fx, y - fy) {
            c = mix(c, fc, a);
        }
        let tint = if i == 0 { [0.10, 0.012, 0.01] } else { [0.11, 0.05, 0.008] };
        if let Some((lc, a)) = led(x - lx, y - ly, tint) {
            c = mix(c, lc, a);
        }
    }
    // Toggle bushings' footprint: a faint machined washer under the
    // sprite so the hardware reads as mounted, not floating.
    for &(tx, ty) in &TOGGLES {
        let r = (x - tx).hypot(y - ty);
        if r < 14.0 {
            let n = v3(0.0, 0.0, 1.0);
            let w = mul(chrome(n, [0.7, 0.7, 0.72]), 0.9 + 0.1 * (r * 6.0).sin());
            c = mix(c, w, cover(r - 13.0) * cover(9.0 - r).max(0.0));
        }
    }
    c
}

// -------------------------------------------------------------- toggles

/// Distance from `p` to segment a→b, and the position t along it.
fn seg_dist(px: f64, py: f64, ax: f64, ay: f64, bx: f64, by: f64) -> (f64, f64) {
    let (vx, vy) = (bx - ax, by - ay);
    let t = (((px - ax) * vx + (py - ay) * vy) / (vx * vx + vy * vy).max(1e-9)).clamp(0.0, 1.0);
    ((px - ax - vx * t).hypot(py - ay - vy * t), t)
}

/// One toggle at (dx, dy) from its centre; `pos` 0 up, 1 centre, 2 down.
fn toggle_px(dx: f64, dy: f64, pos: usize) -> (Rgb, f64) {
    let dir = [-1.0, 0.0, 1.0][pos];
    let len = 21.0;
    let tip = (0.0, dir * len);
    // Shadows first (black, soft): the nut, then the lever, which
    // casts longer as it rises.
    let mut col: Rgb = [0.0; 3];
    let mut alpha = 0.5 * (1.0 - smoothstep(-2.0, 5.0, sd_hex(dx - 2.5, dy - 4.0, 9.5)));
    let (sx, sy) = if pos == 1 { (10.0, 14.0) } else { (tip.0 + 8.0, tip.1 + 12.0) };
    let (sd, _) = seg_dist(dx, dy, 3.0, 5.0, sx, sy);
    alpha = alpha.max(0.42 * (1.0 - smoothstep(0.0, 6.0, sd - 2.5)));


    // Paint helper: lay `c` at coverage `a` over the accumulated pixel.
    fn lay(col: &mut Rgb, alpha: &mut f64, c: Rgb, a: f64) {
        let na = a + *alpha * (1.0 - a);
        if na > 0.0 {
            for k in 0..3 {
                col[k] = (c[k] * a + col[k] * *alpha * (1.0 - a)) / na;
            }
        }
        *alpha = na;
    }

    // Hex nut.
    let dh = sd_hex(dx, dy, 9.0);
    if dh < 0.5 {
        let e = 0.3;
        let gx = sd_hex(dx + e, dy, 9.0) - sd_hex(dx - e, dy, 9.0);
        let gy = sd_hex(dx, dy + e, 9.0) - sd_hex(dx, dy - e, 9.0);
        let gl = gx.hypot(gy).max(1e-9);
        let b = smoothstep(-1.8, 0.0, dh) * 0.85;
        let n = v3(gx / gl * b, gy / gl * b, 1.0).norm();
        lay(&mut col, &mut alpha, chrome(n, [0.8, 0.8, 0.83]), cover(dh));
    }
    // Threaded bushing.
    let r = dx.hypot(dy);
    if r < 5.6 {
        let (ux, uy) = if r > 1e-6 { (dx / r, dy / r) } else { (0.0, 0.0) };
        let k = r / 6.5;
        let n = v3(ux * k, uy * k, (1.0 - k * k).sqrt());
        let thread = 0.75 + 0.25 * (r * 9.0).sin();
        lay(&mut col, &mut alpha, mul(chrome(n, [0.7, 0.7, 0.73]), thread), cover(r - 5.5));
    }
    // The bat lever: a tapered chrome rod to a ball end.
    if pos != 1 {
        let (dist, t) = seg_dist(dx, dy, 0.0, 0.0, tip.0, tip.1);
        let rad = 2.5 + 1.0 * t;
        if dist < rad + 0.5 {
            let s = (dx.signum() * dist / rad).clamp(-0.999, 0.999);
            // Across the rod: a cylinder normal; along it the rod
            // rises toward the viewer as it leaves the bushing.
            let n = v3(s, -dir * 0.35, (1.0 - s * s).sqrt()).norm();
            lay(&mut col, &mut alpha, chrome(n, [0.93, 0.93, 0.96]), cover(dist - rad));
        }
    }
    let ball_r = if pos == 1 { 4.6 } else { 4.1 };
    let (bx, by) = (dx - tip.0, dy - tip.1);
    let br = bx.hypot(by);
    if br < ball_r + 0.5 {
        let k = (br / ball_r).min(0.999);
        let (ux, uy) = if br > 1e-6 { (bx / br, by / br) } else { (0.0, 0.0) };
        let n = v3(ux * k, uy * k, (1.0 - k * k).sqrt());
        lay(&mut col, &mut alpha, chrome(n, [0.95, 0.95, 0.98]), cover(br - ball_r));
    }
    (col, alpha)
}

// ----------------------------------------------------------------- LEDs

/// A lit LED: a hot lens and its bloom on the face.
fn glow_px(dx: f64, dy: f64, color: Rgb) -> (Rgb, f64) {
    let r = dx.hypot(dy);
    let halo = 0.55 * (-(r / 8.0).powi(2)).exp() + 0.22 * (-(r / 19.0).powi(2)).exp();
    let lens = cover(r - 5.0);
    let hot = (1.0 - r / 5.0).clamp(0.0, 1.0).powf(1.5);
    let lens_c = mix(mul(color, 1.6), [2.2, 2.1, 1.9], hot);
    let spec = cover((dx + 1.7).hypot(dy + 1.9) - 1.1);
    let c = mix(mix(mul(color, 1.2), lens_c, lens), [3.0, 3.0, 3.0], spec * 0.8);
    (c, (halo + lens).min(1.0))
}

// ---------------------------------------------------------- the label

/// The label's on-screen box (points): every pixel the turning print
/// can touch, clipped to the face.
pub(super) fn label_rect() -> (f64, f64, f64, f64) {
    let (rx, ry, _, lab) = RECORD;
    let (_, py, _, ph, _) = PEDAL;
    let top = (ry - lab - 1.0).floor();
    (rx - lab - 1.0, top, 2.0 * lab + 2.0, py + ph - top)
}

/// One label pixel, pre-lit: colour = `base` + `gain` · print.
struct LabelPx {
    idx: usize,
    base: Rgb,
    gain: f64,
    r: f64,
    th: f64,
    paper: f64,
}

/// The label's static half, computed once: which pixels it covers
/// and, for each, everything but the print (the face is linear in
/// its albedo, so print-black and print-white pin it exactly).
fn label_cache() -> &'static (usize, usize, Vec<LabelPx>) {
    static CACHE: OnceLock<(usize, usize, Vec<LabelPx>)> = OnceLock::new();
    CACHE.get_or_init(|| {
        let (x0, y0, w, h) = label_rect();
        let (rx, ry, _, lab) = RECORD;
        let (pw, ph) = ((w * S) as usize, (h * S) as usize);
        let mut px = Vec::new();
        for j in 0..ph {
            for i in 0..pw {
                let (x, y) = (x0 + (i as f64 + 0.5) / S, y0 + (j as f64 + 0.5) / S);
                let d = pedal_sd(x, y);
                let (dx, dy) = (x - rx, y - ry);
                let r = dx.hypot(dy);
                if r >= lab + 1.0 || d >= -0.5 {
                    continue;
                }
                let (lit, sheen, k) = face_light(x, y, d);
                let black = albedo(x, y, d, &|_, _, _| [0.0; 3]);
                let white = albedo(x, y, d, &|_, _, _| [1.0; 3]);
                let paper = 1.0 + 0.06 * (vnoise(x, y, 0.4, 21) - 0.5);
                px.push(LabelPx {
                    idx: j * pw + i,
                    base: mul(add(mul(black, lit), [sheen; 3]), k),
                    gain: (white[0] - black[0]) * lit * k,
                    r,
                    th: dy.atan2(dx),
                    paper,
                });
            }
        }
        (pw, ph, px)
    })
}

/// The label at `spin` radians — only the print is re-shaded each
/// paint; pixels off the label or the face stay transparent, so the
/// bake shows through.
pub(super) fn label(spin: f64) -> Image {
    let (pw, ph, cache) = label_cache();
    let mut px = alloc::vec![0u8; pw * ph * 4];
    for p in cache {
        let c = add(p.base, mul(label_print(p.r, p.th - spin, p.paper), p.gain));
        px[p.idx * 4..p.idx * 4 + 4].copy_from_slice(&[to_byte(c[0]), to_byte(c[1]), to_byte(c[2]), 255]);
    }
    Image { px_w: *pw as u32, px_h: *ph as u32, rgba: Arc::from(px) }
}
