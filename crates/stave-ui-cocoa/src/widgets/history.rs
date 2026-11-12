//! `History` — the scrolling recent-history strip of a probe (the
//! echo-tail view). The engine publishes ONE 0..1 number per block;
//! the widget owns the ring and repaints it each tick, newest at the
//! right edge. Reads only published state, like every widget here.

extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;

use core::sync::atomic::Ordering;

use crate::cg::set_image_view_pixels;
use crate::objc::Id;
use crate::view::PluginEditorView;
use crate::*;

/// Ring capacity: at the ~30 Hz tick this is a ~8.5 s window —
/// enough to watch a long echo tail breathe and die.
const RING: usize = 256;

pub(crate) struct HistoryView {
    pub(crate) view: Id,
    pub(crate) pi: usize,
    /// Logical size (points); backing is painted at 2×.
    pub(crate) w: f64,
    pub(crate) h: f64,
    ring: Vec<f32>,
    head: usize,
}

/// Create the history image view and register it for tick repaints.
pub(crate) unsafe fn add_history(v: &mut PluginEditorView, parent: Id, pi: usize, rect: CGRect) {
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
        v.histories.push(HistoryView {
            view,
            pi,
            w: rect.size.width,
            h: rect.size.height,
            ring: vec![0.0; RING],
            head: 0,
        });
    }
}

/// Sample every registered probe into its ring, then repaint the
/// strip: filled columns, newest at the right, accent-tinted.
pub(crate) fn update_histories(v: &mut PluginEditorView) {
    let a = theme::ACCENT;
    for hv in v.histories.iter_mut() {
        let latest = f64::from_bits(v.bus.probes[hv.pi].latest.load(Ordering::Relaxed)) as f32;
        hv.ring[hv.head] = latest.clamp(0.0, 1.0);
        hv.head = (hv.head + 1) % RING;

        let w = (hv.w * 2.0) as usize;
        let h = (hv.h * 2.0) as usize;
        if w == 0 || h == 0 {
            continue;
        }
        let mut px = vec![0u8; w * h * 4];
        for x in 0..w {
            // x = w-1 is the newest sample; walk back through the
            // ring with LINEAR interpolation between samples — the
            // integer step drew a chunky column outline once the
            // pixel density passed the ring resolution.
            let back_f = (w - 1 - x) as f64 * RING as f64 / w as f64;
            let b0 = back_f as usize;
            let frac = (back_f - b0 as f64) as f32;
            let at = |b: usize| hv.ring[(hv.head + RING - 1 - b.min(RING - 1)) % RING];
            let val = at(b0) * (1.0 - frac) + at(b0 + 1) * frac;
            let bar = ((val as f64) * (h as f64 - 4.0)) as usize;
            let x_t = x as f64 / w as f64;
            for y in 0..bar.min(h) {
                // Fade older columns and dim the fill toward the base.
                let y_t = y as f64 / h as f64;
                let alpha = (60.0 + 195.0 * y_t) * (0.35 + 0.65 * x_t);
                let i = ((h - 1 - y) * w + x) * 4;
                px[i] = (a.0 * 255.0) as u8;
                px[i + 1] = (a.1 * 255.0) as u8;
                px[i + 2] = (a.2 * 255.0) as u8;
                px[i + 3] = alpha as u8;
            }
        }
        unsafe { set_image_view_pixels(hv.view, &px, w, h, hv.w, hv.h) };
    }
}
