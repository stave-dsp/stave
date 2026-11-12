//! The photo kit — what a PHOTOREAL picture is made of, under the
//! materials (material.rs) and the parts: a tiny vector type, the studio (one soft key light from the upper left, a softbox
//! environment for chrome), materials, deterministic grain, signed
//! distances, and the bakers that turn a per-pixel shader into a
//! scene [`Image`]. Screen space is y-DOWN, z toward the viewer.
//!
//! The method: give every surface a height profile, take its normal,
//! light it with the same studio — consistency is what reads as real.
//! Bake once (memoize the result), animate with sprites and vectors.

extern crate alloc;
use alloc::sync::Arc;
use alloc::vec::Vec;

use stave_ui::scene::Image;

#[derive(Copy, Clone, Debug)]
pub struct V3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

pub const fn v3(x: f64, y: f64, z: f64) -> V3 {
    V3 { x, y, z }
}

impl V3 {
    pub fn dot(self, o: V3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn norm(self) -> V3 {
        let l = self.dot(self).sqrt().max(1e-12);
        v3(self.x / l, self.y / l, self.z / l)
    }
    pub fn scale(self, k: f64) -> V3 {
        v3(self.x * k, self.y * k, self.z * k)
    }
}

impl core::ops::Add for V3 {
    type Output = V3;
    fn add(self, o: V3) -> V3 {
        v3(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

/// Linear RGB.
pub type Rgb = [f64; 3];

pub fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

pub fn mul(a: Rgb, k: f64) -> Rgb {
    [a[0] * k, a[1] * k, a[2] * k]
}

pub fn add(a: Rgb, b: Rgb) -> Rgb {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// sRGB hex → linear.
pub fn hex(h: u32) -> Rgb {
    let ch = |s: u32| {
        let c = f64::from((h >> s) & 0xFF) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    [ch(16), ch(8), ch(0)]
}

/// Linear → sRGB byte, with a soft filmic shoulder so highlights roll
/// off instead of clipping (photographs never clip hard).
pub fn to_byte(c: f64) -> u8 {
    let c = c.max(0.0);
    let c = if c > 0.8 { 0.8 + 0.2 * (1.0 - (-(c - 0.8) / 0.2).exp()) } else { c };
    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

pub fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The key light: upper left, raised — and the view, straight down.
pub fn key() -> V3 {
    v3(-0.45, -0.65, 1.0).norm()
}
pub const VIEW: V3 = v3(0.0, 0.0, 1.0);

/// A diffuse surface with a soft sheen: albedo under ambient + key,
/// plus a broad specular lobe (`spec` strength, `rough` 0 = glossy).
pub fn plastic(albedo: Rgb, n: V3, spec: f64, rough: f64) -> Rgb {
    let (lit, s) = plastic_terms(n, spec, rough);
    add(mul(albedo, lit), [s, s, s])
}

/// `plastic` split at its linearity: colour = albedo · lit + sheen.
/// Lets a re-paint keep the lighting and swap only the albedo.
pub fn plastic_terms(n: V3, spec: f64, rough: f64) -> (f64, f64) {
    let l = key();
    let diff = n.dot(l).max(0.0);
    let h = (l + VIEW).norm();
    let pow = 8.0 + (1.0 - rough) * 120.0;
    let s = spec * n.dot(h).max(0.0).powf(pow);
    // Sky fill from above-front keeps the shadow side from going dead.
    let fill = 0.28 + 0.12 * (-n.y).max(0.0) + 0.1 * n.z;
    (fill + 0.78 * diff, s)
}

/// The studio as chrome sees it: a big softbox up-left, a bright
/// ceiling, a dark floor, a hard-ish horizon. `r` = reflection dir.
pub fn env(r: V3) -> Rgb {
    let up = (-r.y * 0.8 + r.z * 0.6).clamp(-1.0, 1.0);
    let base = if up > 0.0 { 0.22 + 0.55 * up.powf(0.7) } else { 0.05 + 0.17 * (1.0 + up).powf(3.0) };
    let k = key();
    let d = v3(r.x - k.x, r.y - k.y, r.z - k.z);
    let soft = 2.6 * (-(d.dot(d)) * 5.0).exp();
    // A second, dimmer strip light to the right for an edge kicker.
    let k2 = v3(0.7, -0.2, 0.68).norm();
    let d2 = v3(r.x - k2.x, r.y - k2.y, r.z - k2.z);
    let strip = 0.7 * (-(d2.dot(d2)) * 14.0).exp();
    let v = base + soft + strip;
    [v * 0.97, v * 0.99, v]
}

/// Polished metal: the environment along the reflection, tinted.
pub fn chrome(n: V3, tint: Rgb) -> Rgb {
    let r = n.scale(2.0 * n.dot(VIEW)) + VIEW.scale(-1.0);
    let e = env(r);
    [e[0] * tint[0], e[1] * tint[1], e[2] * tint[2]]
}

/// Integer hash → [0, 1).
pub fn hash2(x: i64, y: i64, seed: u32) -> f64 {
    let mut h = (x as u32).wrapping_mul(0x8DA6_B343) ^ (y as u32).wrapping_mul(0xD816_3841) ^ seed.wrapping_mul(0xCB1A_B31F);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    f64::from(h) / 4_294_967_296.0
}

/// Smooth value noise at `scale` (cells per unit), [0, 1).
pub fn vnoise(x: f64, y: f64, scale: f64, seed: u32) -> f64 {
    let (fx, fy) = (x * scale, y * scale);
    let (ix, iy) = (fx.floor() as i64, fy.floor() as i64);
    let (tx, ty) = (fx - fx.floor(), fy - fy.floor());
    let (sx, sy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let a = hash2(ix, iy, seed);
    let b = hash2(ix + 1, iy, seed);
    let c = hash2(ix, iy + 1, seed);
    let d = hash2(ix + 1, iy + 1, seed);
    let top = a + (b - a) * sx;
    let bot = c + (d - c) * sx;
    top + (bot - top) * sy
}

/// Signed distance to a rounded rect centred at (cx, cy), half-size
/// (hx, hy), corner radius r. Negative inside.
pub fn sd_rrect(x: f64, y: f64, cx: f64, cy: f64, hx: f64, hy: f64, r: f64) -> f64 {
    let qx = (x - cx).abs() - (hx - r);
    let qy = (y - cy).abs() - (hy - r);
    let ox = qx.max(0.0);
    let oy = qy.max(0.0);
    (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - r
}

/// The outward unit gradient of `sd_rrect` (central differences).
pub fn grad_rrect(x: f64, y: f64, cx: f64, cy: f64, hx: f64, hy: f64, r: f64) -> (f64, f64) {
    let e = 0.25;
    let gx = sd_rrect(x + e, y, cx, cy, hx, hy, r) - sd_rrect(x - e, y, cx, cy, hx, hy, r);
    let gy = sd_rrect(x, y + e, cx, cy, hx, hy, r) - sd_rrect(x, y - e, cx, cy, hx, hy, r);
    let l = (gx * gx + gy * gy).sqrt().max(1e-9);
    (gx / l, gy / l)
}

/// Signed distance to a regular hexagon (flat top/bottom), inradius r.
pub fn sd_hex(x: f64, y: f64, r: f64) -> f64 {
    let (kx, ky, kz) = (-0.866_025_404, 0.5, 0.577_350_269);
    let (mut px, mut py) = (x.abs(), y.abs());
    let d = 2.0 * (kx * px + ky * py).min(0.0);
    px -= d * kx;
    py -= d * ky;
    let cx = px.clamp(-kz * r, kz * r);
    let dx = px - cx;
    let dy = py - r;
    (dx * dx + dy * dy).sqrt() * dy.signum()
}

/// Edge coverage from a signed distance in points (negative inside),
/// anti-aliased over one pixel at `scale` pixels per point.
pub fn cover(d: f64, scale: f64) -> f64 {
    (0.5 - d * scale).clamp(0.0, 1.0)
}

/// Opaque bake: `f(x, y)` in points → linear RGB, at `scale` px/pt.
pub fn bake(w: f64, h: f64, scale: f64, f: impl Fn(f64, f64) -> Rgb) -> Image {
    let (pw, ph) = ((w * scale) as usize, (h * scale) as usize);
    let mut px = Vec::with_capacity(pw * ph * 4);
    for j in 0..ph {
        for i in 0..pw {
            let c = f((i as f64 + 0.5) / scale, (j as f64 + 0.5) / scale);
            px.extend_from_slice(&[to_byte(c[0]), to_byte(c[1]), to_byte(c[2]), 255]);
        }
    }
    Image { px_w: pw as u32, px_h: ph as u32, rgba: Arc::from(px) }
}

/// Sprite bake: `f(dx, dy)` relative to the sprite centre → (linear
/// RGB, alpha), stored premultiplied, at `scale` px/pt.
pub fn bake_sprite(size: (f64, f64), scale: f64, f: impl Fn(f64, f64) -> (Rgb, f64)) -> Image {
    let (pw, ph) = ((size.0 * scale) as usize, (size.1 * scale) as usize);
    let mut px = Vec::with_capacity(pw * ph * 4);
    for j in 0..ph {
        for i in 0..pw {
            let (x, y) = ((i as f64 + 0.5) / scale - size.0 / 2.0, (j as f64 + 0.5) / scale - size.1 / 2.0);
            let (c, a) = f(x, y);
            let a = a.clamp(0.0, 1.0);
            // Premultiply in display space: encode the colour, scale by alpha.
            let enc = |v: f64| (f64::from(to_byte(v)) * a + 0.5) as u8;
            px.extend_from_slice(&[enc(c[0]), enc(c[1]), enc(c[2]), (a * 255.0 + 0.5) as u8]);
        }
    }
    Image { px_w: pw as u32, px_h: ph as u32, rgba: Arc::from(px) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plastic_is_linear_in_albedo() {
        let n = v3(0.2, -0.3, 1.0).norm();
        let (lit, s) = plastic_terms(n, 0.1, 0.5);
        let c = plastic([0.5, 0.25, 0.0], n, 0.1, 0.5);
        assert!((c[0] - (0.5 * lit + s)).abs() < 1e-12);
        assert!((c[2] - s).abs() < 1e-12);
    }

    #[test]
    fn sprites_are_premultiplied() {
        let img = bake_sprite((2.0, 2.0), 1.0, |_, _| ([1.0, 1.0, 1.0], 0.5));
        assert_eq!(img.rgba[3], 128);
        assert!(img.rgba[0] <= img.rgba[3]);
    }

    #[test]
    fn rrect_distance_signs() {
        assert!(sd_rrect(0.0, 0.0, 0.0, 0.0, 10.0, 5.0, 2.0) < 0.0);
        assert!(sd_rrect(20.0, 0.0, 0.0, 0.0, 10.0, 5.0, 2.0) > 0.0);
    }
}
