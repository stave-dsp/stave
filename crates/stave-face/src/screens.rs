//! Pictures for screens: the drawing a `Display`'s draw fn usually wants
//! — a graticule, a scope of the captured audio, its spectrum, a
//! transfer curve, an ADSR envelope. Each draws into a screen rect in the
//! theme's phosphor.

extern crate alloc;
use alloc::vec::Vec;

use stave_ui::layout::Rect;
use stave_ui::scene::{pt, Path, Pt, Scene, Stroke};
use stave_ui::style::Color;

use crate::part::Frame;

fn alpha(c: Color, a: f64) -> Color {
    Color::rgba(c.r, c.g, c.b, c.a * a)
}

/// A faint graticule, `cols × rows` divisions.
pub fn grid(f: &Frame<'_>, r: Rect, s: &mut Scene, cols: u32, rows: u32) {
    let c = alpha(f.theme.phosphor, 0.08);
    for i in 1..cols {
        let x = r.x + r.w * f64::from(i) / f64::from(cols);
        s.stroke(Path::new().move_to(pt(x, r.y + 3.0)).line_to(pt(x, r.y + r.h - 3.0)), Stroke::new(c, 1.0));
    }
    for i in 1..rows {
        let y = r.y + r.h * f64::from(i) / f64::from(rows);
        s.stroke(Path::new().move_to(pt(r.x + 3.0, y)).line_to(pt(r.x + r.w - 3.0, y)), Stroke::new(c, 1.0));
    }
}

/// A glowing phosphor line through `pts`.
pub fn glow_line(f: &Frame<'_>, s: &mut Scene, pts: &[Pt]) {
    if pts.len() < 2 {
        return;
    }
    let ph = f.theme.phosphor;
    s.stroke(Path::polyline(pts), Stroke::round(alpha(ph, 0.16), 5.0));
    s.stroke(Path::polyline(pts), Stroke::round(ph, 1.5));
}

/// The captured audio of `channel` as a scope trace.
pub fn scope(f: &Frame<'_>, r: Rect, s: &mut Scene, channel: usize) {
    grid(f, r, s, 8, 4);
    let mut buf = alloc::vec![0f32; 2048];
    let n = f.controller.capture(channel, &mut buf);
    if n < 2 {
        return;
    }
    let step = (n / 400).max(1);
    let pts: Vec<_> = (0..n)
        .step_by(step)
        .map(|i| {
            let x = r.x + 4.0 + (r.w - 8.0) * i as f64 / (n - 1) as f64;
            pt(x, r.y + r.h / 2.0 - f64::from(buf[i]).clamp(-1.0, 1.0) * (r.h / 2.0 - 6.0))
        })
        .collect();
    glow_line(f, s, &pts);
}

/// The magnitude spectrum of the captured audio of `channel`, log
/// frequency (20 Hz – 20 kHz at `sample_rate`), −90…0 dB.
pub fn spectrum(f: &Frame<'_>, r: Rect, s: &mut Scene, channel: usize, sample_rate: f64) {
    grid(f, r, s, 10, 6);
    let mut buf = alloc::vec![0f32; 2048];
    let n = f.controller.capture(channel, &mut buf).min(2048);
    let size = if n >= 2048 { 2048 } else if n >= 1024 { 1024 } else { return };
    let mut re: Vec<f32> = buf[..size]
        .iter()
        .enumerate()
        .map(|(i, &v)| v * (0.5 - 0.5 * (core::f32::consts::TAU * i as f32 / size as f32).cos()))
        .collect();
    let mut im = alloc::vec![0f32; size];
    fft(&mut re, &mut im);
    let mags: Vec<f64> = (0..size / 2)
        .map(|k| {
            let m = f64::from(re[k].hypot(im[k])) / (size as f64 / 4.0);
            20.0 * m.max(1e-9).log10()
        })
        .collect();
    let bins = (r.w / 2.0) as usize;
    let (lo, hi) = (20f64.ln(), 20_000f64.ln());
    let pts: Vec<_> = (0..bins)
        .map(|i| {
            let fr = (lo + (hi - lo) * i as f64 / (bins - 1) as f64).exp();
            let k = ((fr / sample_rate * size as f64) as usize).clamp(1, size / 2 - 1);
            let db = mags[k].max(mags[(k + 1).min(size / 2 - 1)]);
            let t = ((db + 90.0) / 90.0).clamp(0.0, 1.0);
            pt(r.x + r.w * i as f64 / (bins - 1) as f64, r.y + r.h - 3.0 - t * (r.h - 8.0))
        })
        .collect();
    let mut fill = Path::new().move_to(pt(r.x, r.y + r.h));
    for p in &pts {
        fill = fill.line_to(*p);
    }
    s.solid(fill.line_to(pt(r.x + r.w, r.y + r.h)).close(), alpha(f.theme.phosphor, 0.12));
    glow_line(f, s, &pts);
}

/// The plugin's published transfer curve: input across, output up,
/// both over ±1, with the identity line faint behind it.
pub fn curve(f: &Frame<'_>, r: Rect, s: &mut Scene) {
    grid(f, r, s, 4, 4);
    let faint = alpha(f.theme.phosphor, 0.2);
    s.stroke(Path::new().move_to(pt(r.x, r.y + r.h)).line_to(pt(r.x + r.w, r.y)), Stroke::new(faint, 1.0));
    let ys = f.controller.curve();
    if ys.len() < 2 {
        return;
    }
    let n = ys.len();
    let pts: Vec<_> = ys
        .iter()
        .enumerate()
        .map(|(i, &y)| {
            let x = r.x + r.w * i as f64 / (n - 1) as f64;
            pt(x, r.y + r.h / 2.0 - f64::from(y).clamp(-1.2, 1.2) / 1.2 * (r.h / 2.0 - 4.0))
        })
        .collect();
    glow_line(f, s, &pts);
}

/// An ADSR envelope: attack, decay and release as fractions of the
/// screen's width (0–1 each), sustain as a level (0–1).
pub fn envelope(f: &Frame<'_>, r: Rect, s: &mut Scene, attack: f64, decay: f64, sustain: f64, release: f64) {
    grid(f, r, s, 8, 4);
    let (x0, y0, w, h) = (r.x + 6.0, r.y + r.h - 6.0, r.w - 12.0, r.h - 12.0);
    let seg = w / 3.6;
    let a = x0 + attack.clamp(0.0, 1.0) * seg;
    let d = a + decay.clamp(0.0, 1.0) * seg;
    let hold = d + seg * 0.6;
    let rel = hold + release.clamp(0.0, 1.0) * seg;
    let top = y0 - h;
    let sus = y0 - sustain.clamp(0.0, 1.0) * h;
    let pts = [pt(x0, y0), pt(a, top), pt(d, sus), pt(hold, sus), pt(rel, y0)];
    let mut fill = Path::new().move_to(pts[0]);
    for p in &pts[1..] {
        fill = fill.line_to(*p);
    }
    s.solid(fill.close(), alpha(f.theme.phosphor, 0.12));
    glow_line(f, s, &pts);
    for p in &pts[1..4] {
        s.solid(Path::circle(*p, 2.4), f.theme.phosphor);
    }
}

/// In-place radix-2 FFT (length a power of two).
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -core::f32::consts::TAU / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * cr - im[b] * ci, re[b] * ci + im[b] * cr);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let nc = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = nc;
            }
        }
        len <<= 1;
    }
}
