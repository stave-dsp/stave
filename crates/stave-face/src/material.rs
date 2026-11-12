//! Materials and hardware, lit by the one studio in `crate::photo`
//! (key light upper-left, a softbox environment). Everything here is a
//! pure per-pixel function: surfaces take a position and a normal,
//! hardware takes a position relative to its centre and returns colour
//! plus coverage. [`bake`] and [`sprite`] run them over pixels in
//! parallel.
//!
//! These graduated from the first five photoreal faces (heirloom,
//! revenant, ribbon, aeolian, engram), where each had its own copy.

use std::vec::Vec;

use crate::photo::*;
use stave_ui::scene::Image;

use crate::theme::{CapStyle, Coat};

// ------------------------------------------------------------------ bake

fn workers() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 16)
}

/// Run `f` over every pixel of a `w × h` point area at `scale` px/pt,
/// rows split across threads. `f` returns linear RGB and coverage
/// (alpha); the result is premultiplied sRGB8.
pub fn bake_rgba(w: f64, h: f64, scale: f64, f: impl Fn(f64, f64) -> (Rgb, f64) + Sync) -> Image {
    let (pw, ph) = ((w * scale).round().max(1.0) as usize, (h * scale).round().max(1.0) as usize);
    let mut px = std::vec![0u8; pw * ph * 4];
    let rows_per = ph.div_ceil(workers());
    std::thread::scope(|s| {
        for (band, chunk) in px.chunks_mut(rows_per * pw * 4).enumerate() {
            let f = &f;
            s.spawn(move || {
                for (i, out) in chunk.chunks_mut(4).enumerate() {
                    let (xi, yi) = (i % pw, band * rows_per + i / pw);
                    let (x, y) = ((xi as f64 + 0.5) / scale, (yi as f64 + 0.5) / scale);
                    let (c, a) = f(x, y);
                    let a = a.clamp(0.0, 1.0);
                    out[0] = (f64::from(to_byte(c[0])) * a).round() as u8;
                    out[1] = (f64::from(to_byte(c[1])) * a).round() as u8;
                    out[2] = (f64::from(to_byte(c[2])) * a).round() as u8;
                    out[3] = (a * 255.0).round() as u8;
                }
            });
        }
    });
    Image {
        px_w: pw as u32,
        px_h: ph as u32,
        rgba: px.into(),
    }
}

/// An opaque bake.
pub fn bake(w: f64, h: f64, scale: f64, f: impl Fn(f64, f64) -> Rgb + Sync) -> Image {
    bake_rgba(w, h, scale, |x, y| (f(x, y), 1.0))
}

/// A sprite of `size` points centred on its middle: `f` gets the offset
/// from the centre.
pub fn sprite(size: f64, scale: f64, f: impl Fn(f64, f64) -> Option<(Rgb, f64)> + Sync) -> Image {
    let h = size / 2.0;
    bake_rgba(size, size, scale, |x, y| f(x - h, y - h).unwrap_or(([0.0; 3], 0.0)))
}

/// Anti-aliased coverage of a signed distance at `scale` px/pt.
pub fn cov(d: f64, scale: f64) -> f64 {
    cover(d, scale)
}

// --------------------------------------------------------------- normals

/// A spherical cap of radius `r`: the normal at offset (dx, dy). `flat`
/// > 1 flattens the top.
pub fn dome(dx: f64, dy: f64, r: f64, flat: f64) -> V3 {
    let rr = dx.hypot(dy);
    let k = (rr / (r * flat)).min(0.99);
    let (ux, uy) = if rr > 1e-6 { (dx / rr, dy / rr) } else { (0.0, 0.0) };
    v3(ux * k, uy * k, (1.0 - k * k).sqrt())
}

/// A rounded rectangle given by centre, half-extents and corner radius.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct RRect {
    pub cx: f64,
    pub cy: f64,
    pub hw: f64,
    pub hh: f64,
    pub r: f64,
}

impl RRect {
    pub fn new(x: f64, y: f64, w: f64, h: f64, r: f64) -> Self {
        RRect {
            cx: x + w / 2.0,
            cy: y + h / 2.0,
            hw: w / 2.0,
            hh: h / 2.0,
            r: r.min(w / 2.0).min(h / 2.0),
        }
    }
    pub fn sd(&self, x: f64, y: f64) -> f64 {
        sd_rrect(x, y, self.cx, self.cy, self.hw, self.hh, self.r)
    }
    /// The normal of a rounded bevel `width` wide just inside the edge.
    pub fn bevel(&self, x: f64, y: f64, width: f64) -> V3 {
        let d = self.sd(x, y);
        if d <= -width {
            return v3(0.0, 0.0, 1.0);
        }
        let (gx, gy) = grad_rrect(x, y, self.cx, self.cy, self.hw, self.hh, self.r);
        let k = ((width + d) / width).clamp(0.0, 0.999);
        v3(gx * k, gy * k, (1.0 - k * k).sqrt())
    }
    pub fn shifted(&self, dx: f64, dy: f64) -> Self {
        RRect {
            cx: self.cx + dx,
            cy: self.cy + dy,
            ..*self
        }
    }
}

// -------------------------------------------------------------- surfaces

/// Hammer dents: cellular noise shaped into shallow bowls → slope.
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
                (best, bx, by) = (d, px, py);
            }
        }
    }
    let r = best / CELL;
    let k = 0.22 * r * (1.0 - smoothstep(0.55, 0.85, r));
    let (ux, uy) = if best > 1e-6 { ((x - bx) / best, (y - by) / best) } else { (0.0, 0.0) };
    (ux * k, uy * k)
}

/// Slope of a noise height field by central differences.
fn noise_slope(x: f64, y: f64, freq: f64, seed: u32, amp: f64) -> (f64, f64) {
    let e = 0.35;
    let gx = vnoise(x + e, y, freq, seed) - vnoise(x - e, y, freq, seed);
    let gy = vnoise(x, y + e, freq, seed) - vnoise(x, y - e, freq, seed);
    (gx * amp, gy * amp)
}

/// A finished surface of base colour `base` at (x, y) with geometric
/// normal `n` (bevels, domes): the finish perturbs the normal and picks
/// the reflectance.
pub fn surface(finish: Coat, base: Rgb, x: f64, y: f64, n: V3) -> Rgb {
    match finish {
        Coat::Anodized => {
            // Brushed grain: fine horizontal streaks in both the albedo
            // and the normal, a satin sheen, a little metal in the spec.
            let streak = vnoise(x * 0.04, y * 6.0, 1.0, 11) * 0.6 + vnoise(x * 0.11, y * 2.2, 1.0, 12) * 0.4;
            let n = v3(n.x, n.y + (streak - 0.5) * 0.06, n.z).norm();
            let albedo = mul(base, 0.92 + 0.16 * streak);
            let paint = plastic(albedo, n, 0.28, 0.32);
            add(mul(paint, 0.9), mul(chrome(n, mix(base, [1.0; 3], 0.35)), 0.08))
        }
        Coat::Powder => {
            let (sx, sy) = noise_slope(x, y, 0.9, 21, 0.05);
            let n = v3(n.x - sx, n.y - sy, n.z).norm();
            let albedo = mul(base, 0.97 + 0.05 * vnoise(x, y, 0.07, 22));
            plastic(albedo, n, 0.12, 0.55)
        }
        Coat::Hammertone => {
            let (hx, hy) = hammer_slope(x, y);
            let n = v3(n.x - hx, n.y - hy, n.z).norm();
            let paint = plastic(base, n, 0.18, 0.45);
            add(mul(paint, 0.85), mul(chrome(n, mix(base, [1.0; 3], 0.4)), 0.22))
        }
        Coat::Crinkle => {
            let (ax, ay) = noise_slope(x, y, 0.55, 31, 0.35);
            let (bx, by) = noise_slope(x, y, 1.4, 32, 0.18);
            let n = v3(n.x - ax - bx, n.y - ay - by, n.z).norm();
            plastic(base, n, 0.1, 0.6)
        }
        Coat::Enamel => {
            let mottle = vnoise(x, y, 0.025, 41) * 0.6 + vnoise(x, y, 0.09, 42) * 0.4;
            let albedo = mix(mul(base, 0.82), mul(base, 1.12), mottle);
            plastic(albedo, n, 0.55, 0.1)
        }
    }
}

// ------------------------------------------------------------------ desk

/// The bench under the enclosure: felt, a pool of key light, a vignette,
/// and the enclosure's own soft shadow and contact line.
pub fn desk(base: Rgb, x: f64, y: f64, w: f64, h: f64, case: &RRect) -> Rgb {
    let felt = 0.85 + 0.15 * vnoise(x, y, 0.9, 7) + 0.06 * (hash2((x * 3.0) as i64, (y * 3.0) as i64, 3) - 0.5);
    let pool = (-(((x - w * 0.3) / (w * 0.7)).powi(2) + ((y - h * 0.15) / (h * 0.75)).powi(2))).exp();
    let vign = 1.0 - 0.45 * smoothstep(0.42, 0.9, ((x / w - 0.5).powi(2) + (y / h - 0.5).powi(2)).sqrt());
    let c = mul(base, felt * (0.55 + 0.7 * pool) * vign);
    let shadow = 1.0 - 0.75 * (1.0 - smoothstep(-16.0, 46.0, case.shifted(10.0, 18.0).sd(x, y)));
    let contact = 1.0 - 0.6 * (1.0 - smoothstep(0.0, 6.0, case.sd(x, y)));
    mul(c, shadow * contact)
}

// --------------------------------------------------------------- shadows

/// A raised object on the face that shades the surface around it: the
/// key light is upper-left, so its shadow falls down-right.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Occluder {
    /// A round object (knob, footswitch) of radius `r`, standing `height`.
    Disc { cx: f64, cy: f64, r: f64, height: f64 },
    /// A rectangular object (bezel, plate lip).
    Rect { rect: RRect, height: f64 },
}

impl Occluder {
    /// Light reaching the surface at (x, y): 1 = fully lit.
    pub fn light(&self, x: f64, y: f64) -> f64 {
        match *self {
            Occluder::Disc { cx, cy, r, height } => {
                let (ox, oy) = (0.45 * height, 0.75 * height);
                let d = (x - cx - ox).hypot(y - cy - oy) - r;
                let soft = 1.0 - 0.55 * (1.0 - smoothstep(-0.4 * height, 1.3 * height, d));
                let contact = 1.0 - 0.35 * (1.0 - smoothstep(0.0, 2.5, (x - cx).hypot(y - cy) - r));
                soft * contact
            }
            Occluder::Rect { rect, height } => {
                let d = rect.shifted(0.4 * height, 0.65 * height).sd(x, y);
                1.0 - 0.45 * (1.0 - smoothstep(-0.3 * height, 1.2 * height, d))
            }
        }
    }
    fn bounds(&self) -> (f64, f64, f64, f64) {
        match *self {
            Occluder::Disc { cx, cy, r, height } => (cx - r - 2.0, cy - r - 2.0, cx + r + 2.0 * height + 4.0, cy + r + 2.0 * height + 4.0),
            Occluder::Rect { rect, height } => (
                rect.cx - rect.hw - 2.0,
                rect.cy - rect.hh - 2.0,
                rect.cx + rect.hw + 2.0 * height + 4.0,
                rect.cy + rect.hh + 2.0 * height + 4.0,
            ),
        }
    }
}

/// Occluders bucketed on a coarse grid so a pixel only tests its
/// neighbours.
pub struct Shadows {
    cell: f64,
    cols: usize,
    rows: usize,
    buckets: Vec<Vec<Occluder>>,
}

impl Shadows {
    pub fn new(w: f64, h: f64, occluders: &[Occluder]) -> Self {
        let cell = 48.0;
        let (cols, rows) = ((w / cell).ceil().max(1.0) as usize, (h / cell).ceil().max(1.0) as usize);
        let mut buckets = std::vec![Vec::new(); cols * rows];
        for o in occluders {
            let (x0, y0, x1, y1) = o.bounds();
            let (c0, r0) = ((x0 / cell).floor().max(0.0) as usize, (y0 / cell).floor().max(0.0) as usize);
            let (c1, r1) = (((x1 / cell).floor() as usize).min(cols - 1), ((y1 / cell).floor() as usize).min(rows - 1));
            for r in r0..=r1 {
                for c in c0..=c1 {
                    buckets[r * cols + c].push(*o);
                }
            }
        }
        Shadows { cell, cols, rows, buckets }
    }
    pub fn light(&self, x: f64, y: f64) -> f64 {
        let (c, r) = ((x / self.cell).floor(), (y / self.cell).floor());
        if c < 0.0 || r < 0.0 || c as usize >= self.cols || r as usize >= self.rows {
            return 1.0;
        }
        self.buckets[r as usize * self.cols + c as usize].iter().map(|o| o.light(x, y)).product()
    }
}

// --------------------------------------------------------------- hardware

/// A knob cap of outer radius `r` at offset (dx, dy): the skirt and cap
/// of `style`, `tint` for [`CapStyle::Soft`].
pub fn knob(style: CapStyle, tint: Rgb, dx: f64, dy: f64, r: f64, scale: f64) -> Option<(Rgb, f64)> {
    let rr = dx.hypot(dy);
    if rr > r + 0.5 {
        return None;
    }
    let th = dy.atan2(dx);
    let (ux, uy) = if rr > 1e-6 { (dx / rr, dy / rr) } else { (0.0, 0.0) };
    let a = cov(rr - r, scale);
    match style {
        CapStyle::Aluminium => {
            let cap_r = r * 0.72;
            if rr > cap_r {
                let t = ((rr - cap_r) / (r - cap_r)).clamp(0.0, 1.0);
                let tilt = 0.35 + 1.05 * t * t;
                let ridge = (th * (r * 1.4).round()).sin();
                let n = v3(ux * tilt.sin() - uy * 0.32 * ridge, uy * tilt.sin() + ux * 0.32 * ridge, tilt.cos()).norm();
                let mut c = plastic([0.022, 0.022, 0.024], n, 0.55, 0.35);
                c = add(c, mul(chrome(n, [1.0; 3]), 0.05));
                let groove = 1.0 - 0.8 * (1.0 - smoothstep(0.0, 1.4, rr - cap_r));
                Some((mul(c, groove * (1.0 - 0.35 * smoothstep(0.8, 1.0, t))), a))
            } else {
                let rim = smoothstep(cap_r - 2.2, cap_r - 0.3, rr);
                let n = v3(ux * (0.12 * rr / cap_r + rim * 0.9), uy * (0.12 * rr / cap_r + rim * 0.9), 1.0).norm();
                let kl = key();
                let az = kl.y.atan2(kl.x);
                let streak = (0.5 + 0.5 * (2.0 * (th - az)).cos()).powi(3);
                let rings = 0.92 + 0.08 * hash2((rr * 14.0) as i64, 1, 9);
                let spun = (0.32 + 0.62 * streak) * rings;
                let metal = mix([spun * 0.86, spun * 0.88, spun * 0.92], chrome(n, [0.9, 0.91, 0.95]), 0.35 + 0.4 * rim);
                let centre = 1.0 - 0.25 * (1.0 - smoothstep(0.0, 2.2, rr));
                Some((mul(metal, centre), 1.0))
            }
        }
        CapStyle::Dome => {
            let dome_r = r * 0.74;
            if rr > dome_r {
                let t = ((rr - dome_r) / (r - dome_r)).clamp(0.0, 1.0);
                let tilt = 0.4 + 0.9 * t * t;
                let ridge = (th * (r * 1.6).round()).sin() * 0.3;
                let n = v3(ux * tilt.sin() - uy * ridge, uy * tilt.sin() + ux * ridge, tilt.cos()).norm();
                let c = mul(chrome(n, [0.85, 0.86, 0.9]), 1.0 - 0.5 * (1.0 - smoothstep(0.0, 1.2, rr - dome_r)));
                return Some((c, a));
            }
            Some((plastic(tint, dome(dx, dy, dome_r, 1.25), 1.1, 0.12), 1.0))
        }
        CapStyle::Chicken => {
            // The skirt: a black bakelite disc, gently domed, with a
            // fine knurl at the rim. The head is drawn live (it turns).
            let n = dome(dx, dy, r, 2.2);
            let ridge = smoothstep(r - 3.0, r - 0.5, rr) * (th * (r * 2.2).round()).sin() * 0.25;
            let n = v3(n.x - uy * ridge, n.y + ux * ridge, n.z).norm();
            Some((plastic([0.018, 0.018, 0.02], n, 0.7, 0.18), a))
        }
        CapStyle::Soft => {
            // A soft-touch cylinder: a gentle crown, a darker chamfer,
            // a faint knurl on the side wall.
            let wall = r * 0.86;
            if rr > wall {
                let t = ((rr - wall) / (r - wall)).clamp(0.0, 1.0);
                let tilt = 0.5 + 0.9 * t;
                let ridge = (th * (r * 1.8).round()).sin() * 0.18;
                let n = v3(ux * tilt.sin() - uy * ridge, uy * tilt.sin() + ux * ridge, tilt.cos()).norm();
                return Some((mul(plastic(tint, n, 0.25, 0.4), 0.78), a));
            }
            let n = dome(dx, dy, wall, 2.4);
            Some((plastic(tint, n, 0.3, 0.3), 1.0))
        }
    }
}

/// A chrome footswitch: hex nut and domed button, `r` the nut's radius.
pub fn footswitch(dx: f64, dy: f64, r: f64, scale: f64) -> Option<(Rgb, f64)> {
    let rr = dx.hypot(dy);
    let btn = r * 0.62;
    if rr < btn + 1.0 {
        let edge = smoothstep(btn - 1.6, btn, rr);
        let n = dome(dx, dy, btn, 1.7);
        let n = v3(n.x * (1.0 + edge), n.y * (1.0 + edge), n.z).norm();
        return Some((mul(chrome(n, [0.93, 0.93, 0.96]), 1.0 - 0.45 * edge), cov(rr - btn, scale)));
    }
    let hr = r - 1.0;
    let dh = sd_hex(dx, dy, hr);
    if dh >= 0.5 {
        return None;
    }
    let e = 0.3;
    let gx = sd_hex(dx + e, dy, hr) - sd_hex(dx - e, dy, hr);
    let gy = sd_hex(dx, dy + e, hr) - sd_hex(dx, dy - e, hr);
    let gl = gx.hypot(gy).max(1e-9);
    let b = smoothstep(-2.0, 0.0, dh) * 0.85;
    let n = v3(gx / gl * b, gy / gl * b, 1.0).norm();
    let ao = 1.0 - 0.5 * (1.0 - smoothstep(0.0, 3.0, rr - btn));
    Some((mul(chrome(n, [0.82, 0.82, 0.85]), ao), cov(dh, scale)))
}

/// A cross-head screw of radius `r`.
pub fn screw(dx: f64, dy: f64, r: f64, scale: f64) -> Option<(Rgb, f64)> {
    let rr = dx.hypot(dy);
    if rr > r + 0.5 {
        return None;
    }
    let mut c = chrome(dome(dx, dy, r, 1.6), [0.85, 0.85, 0.87]);
    let a = 0.3f64;
    let (u, v) = (dx * a.cos() + dy * a.sin(), -dx * a.sin() + dy * a.cos());
    c = mix(c, [0.02; 3], cov((u.abs().min(v.abs()) - r * 0.15).max(rr - r * 0.6), scale));
    Some((c, cov(rr - r, scale)))
}

/// An LED's chrome bezel and dark lens (the unlit part), radius `r`.
pub fn lamp_seat(dx: f64, dy: f64, r: f64, scale: f64) -> Option<(Rgb, f64)> {
    let rr = dx.hypot(dy);
    if rr > r + 0.5 {
        return None;
    }
    let lens = r * 0.7;
    if rr > lens {
        return Some((chrome(dome(dx, dy, r, 1.0), [0.6, 0.6, 0.62]), cov(rr - r, scale)));
    }
    Some((plastic([0.05, 0.05, 0.06], dome(dx, dy, lens, 1.1), 1.3, 0.04), 1.0))
}

/// A lit LED: hot lens and halo in `color` (linear, may exceed 1),
/// lens radius `r`. Additive-looking, so draw it over the seat.
pub fn lamp_glow(dx: f64, dy: f64, color: Rgb, r: f64, scale: f64) -> Option<(Rgb, f64)> {
    let rr = dx.hypot(dy);
    let halo = 0.5 * (-(rr / (r * 1.6)).powi(2)).exp() + 0.2 * (-(rr / (r * 3.6)).powi(2)).exp();
    let lens = cov(rr - r, scale);
    let hot = (1.0 - rr / r).clamp(0.0, 1.0);
    let lens_c = mix(mul(color, 1.4), [2.1, 2.0, 1.9], hot * hot);
    let c = mix(mul(color, 1.2), lens_c, lens);
    let a = (halo + lens).min(1.0);
    if a < 1e-3 {
        None
    } else {
        Some((c, a))
    }
}

/// A bat-handle toggle lever, `len` long, seen from above and tipped by
/// `tilt` (−1 up, 0 centre, 1 down): a chrome shaft to a ball end.
pub fn toggle_lever(dx: f64, dy: f64, len: f64, tilt: f64, scale: f64) -> Option<(Rgb, f64)> {
    let tip = tilt * len;
    let (bx, by) = (0.0, tip);
    let t = if tip.abs() > 1e-6 { (dy / tip).clamp(0.0, 1.0) } else { 0.0 };
    let shaft_w = 2.0 + 1.6 * t;
    let ds = dx.abs().max(if tip >= 0.0 { -dy.min(dy - tip) } else { dy.max(tip - dy) }) - shaft_w;
    let db = (dx - bx).hypot(dy - by) - 4.2;
    let d = ds.min(db);
    if d > 0.5 {
        return None;
    }
    let n = if db < ds {
        dome(dx - bx, dy - by, 4.2, 1.0)
    } else {
        v3((dx / shaft_w).clamp(-0.95, 0.95), 0.0, 1.0).norm()
    };
    Some((chrome(n, [0.9, 0.9, 0.93]), cov(d, scale)))
}

/// A toggle's threaded bushing and hex nut, radius `r`.
pub fn toggle_nut(dx: f64, dy: f64, r: f64, scale: f64) -> Option<(Rgb, f64)> {
    let rr = dx.hypot(dy);
    let dh = sd_hex(dx, dy, r);
    if dh > 0.5 {
        return None;
    }
    let bush = r * 0.55;
    if rr < bush {
        return Some((chrome(dome(dx, dy, bush, 1.4), [0.7, 0.7, 0.72]), 1.0));
    }
    let e = 0.3;
    let gx = sd_hex(dx + e, dy, r) - sd_hex(dx - e, dy, r);
    let gy = sd_hex(dx, dy + e, r) - sd_hex(dx, dy - e, r);
    let gl = gx.hypot(gy).max(1e-9);
    let b = smoothstep(-1.5, 0.0, dh) * 0.8;
    let n = v3(gx / gl * b, gy / gl * b, 1.0).norm();
    Some((mul(chrome(n, [0.8, 0.8, 0.83]), 0.9), cov(dh, scale)))
}

/// A recessed screen bezel's lip (the frame around a display window),
/// `lip` wide, for a window `rect`.
pub fn bezel(x: f64, y: f64, rect: &RRect, lip: f64, scale: f64) -> Option<(Rgb, f64)> {
    let outer = RRect {
        hw: rect.hw + lip,
        hh: rect.hh + lip,
        r: rect.r + lip,
        ..*rect
    };
    let d = outer.sd(x, y);
    if d > 0.5 || rect.sd(x, y) < -0.5 {
        return None;
    }
    let n = outer.bevel(x, y, lip);
    let inner = 1.0 - smoothstep(-1.2, 0.4, rect.sd(x, y));
    let c = mul(chrome(v3(-n.x, -n.y, n.z).norm(), [0.32, 0.32, 0.34]), 0.6 + 0.4 * inner);
    Some((c, cov(d, scale)))
}

/// Glass over a screen: a soft diagonal sheen and a rim glint.
pub fn glass(x: f64, y: f64, rect: &RRect, scale: f64) -> Option<(Rgb, f64)> {
    let d = rect.sd(x, y);
    if d > 0.0 {
        return None;
    }
    let (u, w) = ((x - rect.cx + rect.hw) * 0.6 + (y - rect.cy + rect.hh) * 0.8, rect.hw.max(rect.hh));
    let band = (-((u - w * 0.55) / (w * 0.35)).powi(2)).exp() * 0.12 + (-((u - w * 0.95) / (w * 0.08)).powi(2)).exp() * 0.05;
    let rim = (1.0 - smoothstep(0.0, 2.5, -d)) * 0.1;
    let a = (0.02 + band + rim).min(1.0);
    let _ = scale;
    Some((mix([0.0; 3], [1.0; 3], ((band + rim) / a).min(1.0)), a))
}

// ------------------------------------------------------------ amp hardware

/// Signed distance to a chicken-head pointer of reach `r` pointing along
/// `angle` (radians, y-down): a round hub tapering to a beak.
fn chicken_sd(dx: f64, dy: f64, r: f64, angle: f64) -> f64 {
    let (c, sn) = (angle.cos(), angle.sin());
    // Into the pointer's frame: u along the beak, v across it.
    let (u, v) = (dx * c + dy * sn, -dx * sn + dy * c);
    let hub = dx.hypot(dy) - r * 0.42;
    // A tapered beak from the hub to the tip, half-width w(u).
    let t = (u / r).clamp(0.0, 1.0);
    let w = r * (0.34 * (1.0 - t) + 0.06 * t);
    let beak = (v.abs() - w).max(u - r).max(-u - r * 0.62);
    hub.min(beak) - 0.6
}

/// The chicken-head pointer of a [`CapStyle::Chicken`] knob, reach `r`,
/// turned to `angle`, moulded in `tint` with a white index line.
pub fn chicken_head(dx: f64, dy: f64, r: f64, angle: f64, tint: Rgb, scale: f64) -> Option<(Rgb, f64)> {
    let d = chicken_sd(dx, dy, r, angle);
    if d > 0.5 {
        return None;
    }
    // A rounded moulding: the normal tilts toward the outline.
    let e = 0.35;
    let gx = chicken_sd(dx + e, dy, r, angle) - chicken_sd(dx - e, dy, r, angle);
    let gy = chicken_sd(dx, dy + e, r, angle) - chicken_sd(dx, dy - e, r, angle);
    let gl = gx.hypot(gy).max(1e-9);
    let k = smoothstep(-3.5, 0.0, d) * 0.85;
    let n = v3(gx / gl * k, gy / gl * k, 1.0).norm();
    let mut c = plastic(tint, n, 0.9, 0.16);
    // The index line along the beak.
    let (cs, sn) = (angle.cos(), angle.sin());
    let (u, v) = (dx * cs + dy * sn, -dx * sn + dy * cs);
    if u > r * 0.25 {
        c = mix(c, [0.95, 0.94, 0.9], cov((v.abs() - 0.7).max(u - r + 1.5), scale));
    }
    Some((c, cov(d, scale)))
}

/// A faceted jewel pilot lamp of radius `r` in a chrome bezel; `lit`
/// glows it in `color` (linear, may exceed 1).
pub fn jewel(dx: f64, dy: f64, r: f64, color: Rgb, lit: bool, scale: f64) -> Option<(Rgb, f64)> {
    let rr = dx.hypot(dy);
    let halo = if lit { 0.45 * (-(rr / (r * 1.4)).powi(2)).exp() } else { 0.0 };
    if rr > r + 0.5 {
        return if halo > 0.01 { Some((mul(color, 1.1), halo)) } else { None };
    }
    let lens = r * 0.72;
    if rr > lens {
        return Some((chrome(dome(dx, dy, r, 1.0), [0.85, 0.85, 0.88]), cov(rr - r, scale)));
    }
    // Facets: the lens is cut into wedges and rings, each catching the
    // light at its own angle.
    let th = dy.atan2(dx);
    let wedge = (th / (core::f64::consts::TAU / 12.0)).floor() * (core::f64::consts::TAU / 12.0) + 0.26;
    let ring = (rr / lens * 3.0).floor() / 3.0;
    let n = v3(wedge.cos() * (0.25 + 0.4 * ring), wedge.sin() * (0.25 + 0.4 * ring), 1.0).norm();
    let base = if lit { mul(color, 0.9) } else { mul(color, 0.08) };
    let glint = chrome(n, if lit { [1.0, 0.9, 0.85] } else { [0.5, 0.5, 0.5] });
    let mut c = add(base, mul(glint, 0.35));
    if lit {
        c = mix(c, [2.0, 1.9, 1.8], (1.0 - rr / lens).clamp(0.0, 1.0).powi(3));
    }
    Some((c, 1.0))
}

/// Tolex: vinyl with a leathery pebble grain over base `base`, normal `n`.
pub fn tolex(base: Rgb, x: f64, y: f64, n: V3) -> Rgb {
    let (ax, ay) = noise_slope(x, y, 1.2, 51, 0.32);
    let (bx, by) = noise_slope(x, y, 0.45, 52, 0.18);
    let n = v3(n.x - ax - bx, n.y - ay - by, n.z).norm();
    let albedo = mul(base, 0.93 + 0.1 * vnoise(x, y, 0.06, 53));
    plastic(albedo, n, 0.22, 0.42)
}

/// Grille cloth: a basket weave of threads in `base`, with the odd
/// lighter fleck (salt-and-pepper).
pub fn grille(base: Rgb, x: f64, y: f64) -> Rgb {
    const PITCH: f64 = 1.6;
    let (cx, cy) = ((x / PITCH).floor(), (y / PITCH).floor());
    let (fx, fy) = (x / PITCH - cx, y / PITCH - cy);
    let over = (cx as i64 + cy as i64).rem_euclid(2) == 0;
    // Each cell is one thread crossing: lit across its width, dark in
    // the gaps between threads.
    let along = if over { fy } else { fx };
    let across = if over { fx } else { fy };
    let thread = (core::f64::consts::PI * across).sin().powf(0.5) * (0.85 + 0.15 * (core::f64::consts::PI * along).sin());
    // Sparse light threads woven in (salt-and-pepper), and a slow
    // variation in the dye.
    let fleck = smoothstep(0.955, 0.985, hash2(cx as i64, cy as i64, 61)) * 0.6;
    let dye = 0.92 + 0.12 * vnoise(x, y, 0.04, 62);
    let tone = mix(mul(base, dye), [0.62, 0.58, 0.52], fleck);
    mul(tone, 0.55 + 0.55 * thread)
}

/// A piping cord (welt) of half-width `w`, at signed distance `d` from
/// its centre line: colour and coverage.
pub fn piping(base: Rgb, d: f64, w: f64, side: (f64, f64), scale: f64) -> Option<(Rgb, f64)> {
    if d.abs() > w + 0.5 {
        return None;
    }
    let t = (d / w).clamp(-0.99, 0.99);
    let n = v3(side.0 * t, side.1 * t, (1.0 - t * t).sqrt()).norm();
    Some((plastic(base, n, 0.6, 0.2), cov(d.abs() - w, scale)))
}

/// A black corner protector over a box corner: a rounded cap of size
/// `s` at offset (dx, dy) from the corner, pointing into the box along
/// (sx, sy) (±1 each).
pub fn corner(dx: f64, dy: f64, s: f64, sx: f64, sy: f64, scale: f64) -> Option<(Rgb, f64)> {
    let (u, v) = (dx * sx, dy * sy);
    if u < -1.0 || v < -1.0 {
        return None;
    }
    let d = (u + v - s).max(-u).max(-v) - 1.5;
    let d = d.min((u - 2.0).hypot(v - 2.0) - s * 0.45);
    if d > 0.5 {
        return None;
    }
    let k = smoothstep(-4.0, 0.0, d) * 0.6;
    let n = v3(sx * k * 0.5, sy * k * 0.5, 1.0).norm();
    let mut c = plastic([0.03, 0.03, 0.032], n, 0.25, 0.55);
    // A rivet.
    let rv = (u - s * 0.3).hypot(v - s * 0.3) - 2.2;
    if rv < 0.5 {
        c = mix(c, chrome(dome(u - s * 0.3, v - s * 0.3, 2.2, 1.0), [0.8, 0.8, 0.82]), cov(rv, scale));
    }
    Some((c, cov(d, scale)))
}

/// A rack ear's mounting slot (an oval cut, the rack rail behind it, a
/// screw in it), slot half-length `hl` and radius `r`.
pub fn rack_slot(dx: f64, dy: f64, hl: f64, r: f64, scale: f64) -> Option<(Rgb, f64)> {
    let d = (dx.abs() - hl).max(0.0).hypot(dy) - r;
    if d > 0.5 {
        return None;
    }
    let depth = smoothstep(-r, 0.0, d);
    let mut c = mix([0.01; 3], [0.08; 3], depth);
    let screw_r = r * 1.15;
    if let Some((sc, a)) = screw(dx - hl * 0.4, dy, screw_r, scale) {
        c = mix(c, sc, a);
    }
    Some((c, cov(d, scale)))
}

/// A rack handle seen face-on: a chrome bar `len` tall, radius `r`.
pub fn rack_handle(dx: f64, dy: f64, len: f64, r: f64, scale: f64) -> Option<(Rgb, f64)> {
    let d = dx.hypot((dy.abs() - len / 2.0).max(0.0)) - r;
    if d > 0.5 {
        return None;
    }
    let t = (dx / r).clamp(-0.99, 0.99);
    let ty = if dy.abs() > len / 2.0 { (dy - dy.signum() * len / 2.0) / r } else { 0.0 };
    let n = v3(t, ty.clamp(-0.99, 0.99), (1.0 - t * t - ty * ty).max(0.02).sqrt()).norm();
    Some((chrome(n, [0.9, 0.9, 0.93]), cov(d, scale)))
}
