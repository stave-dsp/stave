//! `Wave` — the periodic-waveform display (the LFO view): one cycle
//! of the bus-published curve as a polyline, with a live phase
//! cursor riding it. Repainted from the tick loop like the transfer
//! plot; reads only published state, never engine internals.

extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;

use core::sync::atomic::Ordering;

use crate::cg::set_image_view_pixels;
use crate::objc::Id;
use crate::view::PluginEditorView;
use crate::*;

pub(crate) struct WaveView {
    pub(crate) view: Id,
    /// Probe index of the 0..1 phase wire.
    pub(crate) phase_pi: usize,
    /// Logical size (points); backing is painted at 2×.
    pub(crate) w: f64,
    pub(crate) h: f64,
}

/// Create the wave image view and register it for tick repaints.
pub(crate) unsafe fn add_wave(v: &mut PluginEditorView, parent: Id, phase_pi: usize, rect: CGRect) {
    unsafe {
        let cls = class(c"NSImageView");
        let raw = msg_id(cls, sel(c"alloc"));
        let view = msg_id_rect(raw, sel(c"initWithFrame:"), rect);
        msg_void_int(view, sel(c"setImageScaling:"), 0);
        msg_void_int(view, sel(c"setWantsLayer:"), 1);
        let l = msg_id(view, sel(c"layer"));
        if !l.is_null() {
            let bg = make_cg_color(0.07, 0.07, 0.09, 1.0);
            let f: unsafe extern "C" fn(Id, Sel, *mut core::ffi::c_void) =
                core::mem::transmute(objc_msgSend as *const ());
            f(l, sel(c"setBackgroundColor:"), bg);
            let f2: unsafe extern "C" fn(Id, Sel, f64) =
                core::mem::transmute(objc_msgSend as *const ());
            f2(l, sel(c"setCornerRadius:"), 3.0);
        }
        msg_void_id(parent, sel(c"addSubview:"), view);
        v.waves.push(WaveView {
            view,
            phase_pi,
            w: rect.size.width,
            h: rect.size.height,
        });
    }
}

/// Repaint every wave view: the published cycle as a polyline, the
/// phase cursor as a dot ON the line.
pub(crate) fn update_waves(v: &mut PluginEditorView) {
    let curve_n = v.bus.curve_used.load(Ordering::Acquire) as usize;
    if v.waves.is_empty() || curve_n < 2 {
        return;
    }
    // Snapshot the cycle once for all views.
    let pts: Vec<f32> = (0..curve_n)
        .map(|i| f32::from_bits(v.bus.curve[i].load(Ordering::Relaxed)))
        .collect();
    let a = theme::ACCENT;
    let accent = (
        (a.0 * 255.0) as u8,
        (a.1 * 255.0) as u8,
        (a.2 * 255.0) as u8,
        255u8,
    );

    for wv in v.waves.iter() {
        let w = (wv.w * 2.0) as usize;
        let h = (wv.h * 2.0) as usize;
        if w == 0 || h == 0 {
            continue;
        }
        let mut px = vec![0u8; w * h * 4];
        let y_of = |val: f32| -> i32 {
            let t = ((val.clamp(-1.0, 1.0) + 1.0) * 0.5) as f64;
            ((1.0 - t) * (h as f64 - 5.0) + 2.0) as i32
        };
        // Midline.
        let mid_y = y_of(0.0);
        for x in 0..w {
            put(&mut px, w, h, x as i32, mid_y, (60, 60, 70, 255));
        }
        // The cycle polyline, linearly interpolated between curve
        // points — nearest-sample lookup drew a staircase at any
        // pixel density above the published resolution.
        let sample = |t: f32| -> f32 {
            let f = t.clamp(0.0, 1.0) * (curve_n - 1) as f32;
            let i = (f as usize).min(curve_n - 2);
            let frac = f - i as f32;
            pts[i] * (1.0 - frac) + pts[i + 1] * frac
        };
        let mut last: Option<(i32, i32)> = None;
        for x in 0..w {
            let t = x as f32 / (w - 1) as f32;
            let p = (x as i32, y_of(sample(t)));
            if let Some(q) = last {
                line(&mut px, w, h, q, p, accent);
            }
            last = Some(p);
        }
        // The live cursor: phase 0..1 along the cycle.
        let phase = f64::from_bits(v.bus.probes[wv.phase_pi].latest.load(Ordering::Relaxed))
            .clamp(0.0, 1.0) as f32;
        let cx = (phase * (w as f32 - 1.0)) as i32;
        let cy = y_of(sample(phase));
        dot(&mut px, w, h, cx, cy, 4, (255, 255, 255, 255));
        unsafe { set_image_view_pixels(wv.view, &px, w, h, wv.w, wv.h) };
    }
}

#[inline]
fn put(px: &mut [u8], w: usize, h: usize, x: i32, y: i32, c: (u8, u8, u8, u8)) {
    if x < 0 || y < 0 || x as usize >= w || y as usize >= h {
        return;
    }
    let i = (y as usize * w + x as usize) * 4;
    px[i] = c.0;
    px[i + 1] = c.1;
    px[i + 2] = c.2;
    px[i + 3] = c.3;
}

fn line(px: &mut [u8], w: usize, h: usize, a: (i32, i32), b: (i32, i32), c: (u8, u8, u8, u8)) {
    // Vertical fill between adjacent columns keeps steep edges solid.
    let (x0, y0) = a;
    let (x1, y1) = b;
    let steps = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
    for s in 0..=steps {
        let x = x0 + (x1 - x0) * s / steps;
        let y = y0 + (y1 - y0) * s / steps;
        put(px, w, h, x, y, c);
        put(px, w, h, x, y + 1, c);
    }
}

fn dot(px: &mut [u8], w: usize, h: usize, cx: i32, cy: i32, r: i32, c: (u8, u8, u8, u8)) {
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy <= r * r {
                put(px, w, h, cx + dx, cy + dy, c);
            }
        }
    }
}
