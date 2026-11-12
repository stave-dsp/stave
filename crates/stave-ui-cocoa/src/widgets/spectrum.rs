use core::ffi::c_void;

extern crate alloc;

use crate::theme;

use crate::cg::set_image_view_pixels;
use crate::view::PluginEditorView;
use crate::*;

/// Repaint the shared spectrum strip from the bus capture (moved
/// verbatim from view.rs; becomes the Spectrum component in 7.3).
pub(crate) fn update_spectrum(v: &mut PluginEditorView) {
    const N: usize = 512;
    const HALF: usize = N / 2;

    // Pull the most recent capture frame from the bus (output side).
    let chans = v
        .bus
        .capture
        .channels
        .load(core::sync::atomic::Ordering::Acquire) as usize;
    let frames = v
        .bus
        .capture
        .frames
        .load(core::sync::atomic::Ordering::Acquire) as usize;
    if chans == 0 || frames < 8 {
        return;
    }
    let take = N.min(frames);
    let cap_len = stave_core::debug::CAPTURE_LEN;
    let mut buf = alloc::vec![0.0_f32; N];
    for i in 0..take {
        let mut s = 0.0;
        for c in 0..chans.min(stave_core::debug::CAPTURE_CHANNELS) {
            let bits = v.bus.capture.samples[c * cap_len + i]
                .load(core::sync::atomic::Ordering::Relaxed) as u32;
            s += f32::from_bits(bits);
        }
        buf[i] = s / chans.max(1) as f32;
    }
    // Hann window.
    for i in 0..take {
        let w = 0.5 - 0.5 * (2.0 * core::f32::consts::PI * i as f32 / (take as f32 - 1.0)).cos();
        buf[i] *= w;
    }

    // In-place radix-2 FFT (split into real + imag arrays).
    let mut re = buf;
    let mut im = alloc::vec![0.0_f32; N];
    fft_radix2(&mut re, &mut im);

    // Magnitude → dB.
    let inv_n = 2.0 / N as f32;
    let mut db_bins = alloc::vec![-90.0_f32; HALF];
    for k in 0..HALF {
        let m = (re[k] * re[k] + im[k] * im[k]).sqrt() * inv_n;
        db_bins[k] = if m <= 1e-7 { -90.0 } else { 20.0 * m.log10() };
    }
    // Exponential smoothing across frames so the bars don't strobe.
    let alpha = 0.45;
    for k in 0..HALF {
        v.spectrum_smooth[k] += alpha * (db_bins[k] - v.spectrum_smooth[k]);
    }

    // Render at 2× backing resolution so the bars + threshold lines
    // stay crisp on Retina (2× backing presented at MB_SPEC_W×64 pts).
    let w: usize = (v.spectrum_strip_w.max(60.0) * 2.0) as usize;
    let h: usize = 128;
    let mut pixels = alloc::vec![0_u8; w * h * 4];
    for i in (0..pixels.len()).step_by(4) {
        // background already dark via the layer; leave transparent
        // and let the layer show through.
        pixels[i] = 0;
        pixels[i + 1] = 0;
        pixels[i + 2] = 0;
        pixels[i + 3] = 0;
    }

    // dB range.
    let db_min = -78.0_f32;
    let db_max = -6.0_f32;
    let db_range = db_max - db_min;

    // Faint horizontal gridlines at -12 / -24 / -36 / -48 / -60 dB.
    // Two-pixel thick (= 1 logical pt at 2× backing) so they stay
    // visible on Retina without going blurry.
    let (gr_r, gr_g, gr_b, _) = theme::rgba_u8(theme::AXIS);
    let grid_alpha = 90_u8;
    for &grid_db in &[-12.0_f32, -24.0, -36.0, -48.0, -60.0] {
        let frac = ((grid_db - db_min) / db_range).clamp(0.0, 1.0);
        let y0 = ((1.0 - frac) * (h as f32 - 1.0)) as usize;
        for dy in 0..2 {
            let y = (y0 + dy).min(h - 1);
            for x in 0..w {
                let p = (y * w + x) * 4;
                pixels[p] = gr_r;
                pixels[p + 1] = gr_g;
                pixels[p + 2] = gr_b;
                pixels[p + 3] = grid_alpha;
            }
        }
    }
    // Crossover frequencies for region tinting.
    let low_xover = v
        .spectrum_low_xover_ctrl
        .map(|ci| v.plain(ci) as f32)
        .unwrap_or(250.0);
    let high_xover = v
        .spectrum_high_xover_ctrl
        .map(|ci| v.plain(ci) as f32)
        .unwrap_or(2500.0);
    // Per-band compressor thresholds (dB). Default mid-range so the
    // overlay is visible even if controls weren't found.
    let read_db = |ci: Option<usize>, default: f32| -> f32 {
        ci.map(|i| v.plain(i) as f32).unwrap_or(default)
    };
    let lo_thresh = read_db(v.spectrum_lo_thresh_ctrl, -18.0);
    let mid_thresh = read_db(v.spectrum_mid_thresh_ctrl, -18.0);
    let hi_thresh = read_db(v.spectrum_hi_thresh_ctrl, -18.0);
    let thresh_y = |t: f32| -> usize {
        let frac = ((t - db_min) / db_range).clamp(0.0, 1.0);
        ((1.0 - frac) * (h as f32 - 1.0)) as usize
    };
    let sr = v.spectrum_sample_rate.max(1000.0);
    let nyq = sr * 0.5;
    let log_min = 20.0_f32.log10();
    let log_max = nyq.log10();

    // Cross-highlight: when any band is soloed, the non-soloed
    // bands' spectrum regions render at a fraction of their normal
    // alpha so the soloed band visually pops.
    let solos = v.read_solo_state();
    let any_solo = solos[0] || solos[1] || solos[2];

    // Compression ghost — read per-band GR so we can sketch where
    // each bar WOULD have peaked without compression. Pixels at
    // that "ghost" y are painted as a thin outline so the user
    // can see "you took X dB off here".
    let read_gr = |pi: Option<usize>| -> f32 {
        pi.map(|i| {
            f64::from_bits(
                v.bus.probes[i]
                    .latest
                    .load(core::sync::atomic::Ordering::Relaxed),
            ) as f32
        })
        .map(|v| (-v).clamp(0.0, 30.0))
        .unwrap_or(0.0)
    };
    let lo_gr = read_gr(v.spectrum_lo_gr_probe);
    let mid_gr = read_gr(v.spectrum_mid_gr_probe);
    let hi_gr = read_gr(v.spectrum_hi_gr_probe);

    for x in 0..w {
        let t = x as f32 / (w - 1) as f32;
        let hz = 10f32.powf(log_min + t * (log_max - log_min));
        let bin_f = hz * N as f32 / sr;
        let bin = (bin_f as usize).min(HALF - 1);
        let db = v.spectrum_smooth[bin];
        let mag = ((db - db_min) / db_range).clamp(0.0, 1.0);
        let bar_top = ((1.0 - mag) * h as f32) as usize;
        // Which band does this column belong to → accent colour,
        // threshold, and SOLO-state for dimming.
        let (band_idx, band_thresh) = if hz < low_xover {
            (0_usize, lo_thresh)
        } else if hz < high_xover {
            (1, mid_thresh)
        } else {
            (2, hi_thresh)
        };
        let base = theme::band_u8(band_idx);
        let hot = theme::band_hot_u8(band_idx);
        let dim = any_solo && !solos[band_idx];
        let alpha_scale: f32 = if dim { 0.30 } else { 1.0 };
        let ty = thresh_y(band_thresh);
        for y in bar_top..h {
            let p = (y * w + x) * 4;
            let (r, g, b, a) = if y < ty {
                (hot.0, hot.1, hot.2, 240_u8)
            } else {
                (base.0, base.1, base.2, 200_u8)
            };
            pixels[p] = r;
            pixels[p + 1] = g;
            pixels[p + 2] = b;
            pixels[p + 3] = ((a as f32) * alpha_scale) as u8;
        }
        // Top edge highlight (also dimmed when off-band).
        if bar_top < h {
            let p = (bar_top * w + x) * 4;
            pixels[p] = 255;
            pixels[p + 1] = 255;
            pixels[p + 2] = 255;
            pixels[p + 3] = (240.0 * alpha_scale) as u8;
        }
        // Compression ghost — a single white dot at the position
        // the bar WOULD have peaked at without the band's gain
        // reduction. Only drawn when GR > 0.5 dB so it doesn't
        // clutter when idle.
        let band_gr = match band_idx {
            0 => lo_gr,
            1 => mid_gr,
            _ => hi_gr,
        };
        if band_gr > 0.5 {
            let pre_db = db + band_gr;
            let pre_mag = ((pre_db - db_min) / db_range).clamp(0.0, 1.0);
            let ghost_y = ((1.0 - pre_mag) * h as f32) as usize;
            if ghost_y < bar_top && ghost_y < h {
                let p = (ghost_y * w + x) * 4;
                pixels[p] = 240;
                pixels[p + 1] = 240;
                pixels[p + 2] = 250;
                pixels[p + 3] = (170.0 * alpha_scale) as u8;
            }
        }
    }

    // Compressor threshold lines per band region — dashed, drawn in
    // the band accent so they read as "this band's threshold".
    let band_x_ranges = [
        (20.0f32, low_xover, (160, 200, 240), lo_thresh),
        (low_xover, high_xover, (170, 220, 190), mid_thresh),
        (high_xover, nyq, (240, 195, 145), hi_thresh),
    ];
    // 2× backing: dashes scaled (12 px on, 8 px off ≈ 6/4 pt) and
    // threshold lines drawn 2 px tall with a 2 px glow below.
    for &(lo_hz, hi_hz, col, t_db) in &band_x_ranges {
        let x_lo =
            ((lo_hz.log10() - log_min) / (log_max - log_min)).clamp(0.0, 1.0) * (w - 1) as f32;
        let x_hi =
            ((hi_hz.log10() - log_min) / (log_max - log_min)).clamp(0.0, 1.0) * (w - 1) as f32;
        let y = thresh_y(t_db);
        for x in (x_lo as usize)..=(x_hi as usize).min(w - 1) {
            if (x / 12) % 2 == 0 {
                continue;
            }
            for dy in 0..2 {
                let yy = (y + dy).min(h - 1);
                let p = (yy * w + x) * 4;
                pixels[p] = col.0;
                pixels[p + 1] = col.1;
                pixels[p + 2] = col.2;
                pixels[p + 3] = 235;
            }
            for dy in 2..4 {
                if y + dy >= h {
                    break;
                }
                let p2 = ((y + dy) * w + x) * 4;
                pixels[p2] = pixels[p2].max(col.0 / 3);
                pixels[p2 + 1] = pixels[p2 + 1].max(col.1 / 3);
                pixels[p2 + 2] = pixels[p2 + 2].max(col.2 / 3);
                pixels[p2 + 3] = pixels[p2 + 3].max(120);
            }
        }
    }

    // Sidechain HPF cutoff markers per band — short dashed
    // vertical line at the cutoff freq, drawn in the band accent.
    // Only painted when cutoff > 10 Hz so off-state stays clean.
    let sc_hpf_marker = |ci: Option<usize>| -> Option<f32> {
        let v = ci.map(|i| v.plain(i) as f32)?;
        if v > 10.0 {
            Some(v)
        } else {
            None
        }
    };
    let bands_sc: [(Option<f32>, (u8, u8, u8)); 3] = [
        (sc_hpf_marker(v.spectrum_lo_sc_hpf_ctrl), (160, 200, 240)),
        (sc_hpf_marker(v.spectrum_mid_sc_hpf_ctrl), (170, 220, 190)),
        (sc_hpf_marker(v.spectrum_hi_sc_hpf_ctrl), (240, 195, 145)),
    ];
    for &(hpf_opt, col) in &bands_sc {
        if let Some(hpf_hz) = hpf_opt {
            let t = (hpf_hz.log10() - log_min) / (log_max - log_min);
            if !t.is_finite() {
                continue;
            }
            let x0 = ((t.clamp(0.0, 1.0)) * (w - 1) as f32) as usize;
            for dx in 0..2 {
                let x = (x0 + dx).min(w - 1);
                // Dashed: 6 px on, 6 px off, only in the bottom
                // 1/3 of the spectrum so it doesn't fight bars.
                for y in (2 * h / 3)..h {
                    if (y / 6) % 2 == 0 {
                        continue;
                    }
                    let p = (y * w + x) * 4;
                    pixels[p] = col.0;
                    pixels[p + 1] = col.1;
                    pixels[p + 2] = col.2;
                    pixels[p + 3] = 200;
                }
            }
        }
    }

    // Crossover marker lines — 2 px wide on Retina.
    for &xover_hz in &[low_xover, high_xover] {
        let t = (xover_hz.log10() - log_min) / (log_max - log_min);
        if !t.is_finite() {
            continue;
        }
        let x0 = ((t.clamp(0.0, 1.0)) * (w - 1) as f32) as usize;
        for dx in 0..2 {
            let x = (x0 + dx).min(w - 1);
            for y in 0..h {
                let p = (y * w + x) * 4;
                pixels[p] = 230;
                pixels[p + 1] = 230;
                pixels[p + 2] = 240;
                pixels[p + 3] = 220;
            }
        }
    }

    // Present at logical MB_SPEC_W×64 points; backing pixels are 2×.
    unsafe {
        set_image_view_pixels(
            v.spectrum_view,
            &pixels,
            w,
            h,
            v.spectrum_strip_w.max(60.0),
            64.0,
        )
    };
}

/// The spectrum analyser strip: draggable band-threshold canvas +
/// per-band GR overlays + right-edge dB scale. `r` includes the
/// 24 pt label gutter on the right; overlays key off SpectrumKeys.
pub(crate) unsafe fn create_spectrum(
    v: &mut PluginEditorView,
    parent: Id,
    r: CGRect,
    k: &stave_core::SpectrumKeys,
    ci_of: &dyn Fn(stave_core::params::ParamId) -> Option<usize>,
) {
    let label_class = class(c"NSTextField");
    let strip_w = (r.size.width - 24.0).max(60.0);
    v.spectrum_strip_w = strip_w;
    let strip_h = r.size.height;
    // Stash the crossover and per-band threshold control indices.
    // The threshold values are overlaid on the analyser as
    // horizontal lines (one per band region) so the user can see
    // at a glance which bins are above/below their compressor
    // threshold — the "compression in context" view.
    let g = |ids: &[stave_core::params::ParamId], i: usize| ids.get(i).and_then(|&id| ci_of(id));
    v.spectrum_low_xover_ctrl = g(&k.splits, 0);
    v.spectrum_high_xover_ctrl = g(&k.splits, 1);
    v.spectrum_lo_thresh_ctrl = g(&k.thresholds, 0);
    v.spectrum_mid_thresh_ctrl = g(&k.thresholds, 1);
    v.spectrum_hi_thresh_ctrl = g(&k.thresholds, 2);
    v.spectrum_lo_sc_hpf_ctrl = g(&k.sc_hpfs, 0);
    v.spectrum_mid_sc_hpf_ctrl = g(&k.sc_hpfs, 1);
    v.spectrum_hi_sc_hpf_ctrl = g(&k.sc_hpfs, 2);
    v.spectrum_lo_gr_probe = k.gr_probes.first().and_then(|n| find_probe(&v.bus, n));
    v.spectrum_mid_gr_probe = k.gr_probes.get(1).and_then(|n| find_probe(&v.bus, n));
    v.spectrum_hi_gr_probe = k.gr_probes.get(2).and_then(|n| find_probe(&v.bus, n));
    // Custom NSImageView subclass so mouse drags on the analyser
    // map to per-band threshold writes.
    let spectrum_class = unsafe { spectrum_view_class() };
    let spec_raw = unsafe { msg_id(spectrum_class, sel(c"alloc")) };
    let spec_rect = CGRect {
        origin: CGPoint {
            x: r.origin.x,
            y: r.origin.y,
        },
        size: CGSize {
            width: strip_w,
            height: strip_h,
        },
    };
    let spec_view = unsafe { msg_id_rect(spec_raw, sel(c"initWithFrame:"), spec_rect) };
    unsafe {
        // Don't scale image — pixel-for-pixel.
        msg_void_int(spec_view, sel(c"setImageScaling:"), 0);
        // Wants layer for dark backdrop.
        msg_void_int(spec_view, sel(c"setWantsLayer:"), 1);
        let layer = msg_id(spec_view, sel(c"layer"));
        if !layer.is_null() {
            let bg = make_cg_color(theme::BG_0.0, theme::BG_0.1, theme::BG_0.2, theme::BG_0.3);
            let f: unsafe extern "C" fn(Id, Sel, *mut c_void) =
                core::mem::transmute(objc_msgSend as *const ());
            f(layer, sel(c"setBackgroundColor:"), bg);
            let f2: unsafe extern "C" fn(Id, Sel, f64) =
                core::mem::transmute(objc_msgSend as *const ());
            f2(layer, sel(c"setCornerRadius:"), theme::RADIUS_SM);
            // Hairline border so the analyser reads as an inset
            // panel rather than a void in the chrome.
            let fw: unsafe extern "C" fn(Id, Sel, f64) =
                core::mem::transmute(objc_msgSend as *const ());
            fw(layer, sel(c"setBorderWidth:"), 1.0);
            let border = make_cg_color(theme::LINE.0, theme::LINE.1, theme::LINE.2, theme::LINE.3);
            let fb: unsafe extern "C" fn(Id, Sel, *mut c_void) =
                core::mem::transmute(objc_msgSend as *const ());
            fb(layer, sel(c"setBorderColor:"), border);
        }
        msg_void_id(parent, sel(c"addSubview:"), spec_view);
        // Stash our self_ptr in the view's editor_ptr ivar so the
        // mouse handlers can locate the bus.
        let ivar = class_getInstanceVariable(spectrum_class, c"editor_ptr".as_ptr());
        if !ivar.is_null() {
            object_setIvar(spec_view, ivar, v.self_ptr as Id);
        }
    }
    v.spectrum_view = spec_view;

    // Per-band live GR readout overlays sitting along the top of
    // the spectrum in each band's region. Empty when GR < 0.1 dB.
    let gr_low = unsafe { msg_id(label_class, sel(c"alloc")) };
    let gr_mid = unsafe { msg_id(label_class, sel(c"alloc")) };
    let gr_hi = unsafe { msg_id(label_class, sel(c"alloc")) };
    // Approximate band-region x centers at default crossovers
    // (250 / 2500). Re-laid out per band; initial placement is
    // good enough since the bands' x-range only shifts when the
    // user moves the crossovers, and the labels stay within
    // their bands anyway.
    let gr_y = r.origin.y + 2.0;
    let gr_w = 70.0;
    let gr_h = 12.0;
    let lbls = [
        (
            gr_low,
            (0.45_f64, 0.68_f64, 0.92_f64),
            r.origin.x + strip_w * 0.15,
        ),
        (
            gr_mid,
            (0.55_f64, 0.82_f64, 0.66_f64),
            r.origin.x + strip_w * 0.49,
        ),
        (
            gr_hi,
            (0.92_f64, 0.68_f64, 0.42_f64),
            r.origin.x + strip_w * 0.81,
        ),
    ];
    for (lbl, rgb, x) in &lbls {
        unsafe {
            let rect = CGRect {
                origin: CGPoint { x: *x, y: gr_y },
                size: CGSize {
                    width: gr_w,
                    height: gr_h,
                },
            };
            let l = msg_id_rect(*lbl, sel(c"initWithFrame:"), rect);
            msg_void_int(l, sel(c"setBezeled:"), 0);
            msg_void_int(l, sel(c"setDrawsBackground:"), 0);
            msg_void_int(l, sel(c"setEditable:"), 0);
            msg_void_int(l, sel(c"setSelectable:"), 0);
            msg_void_int(l, sel(c"setAlignment:"), 1); // center
            let font = ns_font_mono_digit(9.0, NS_FONT_WEIGHT_HEAVY);
            msg_void_id(l, sel(c"setFont:"), font);
            msg_void_id(l, sel(c"setTextColor:"), ns_color(rgb.0, rgb.1, rgb.2, 1.0));
            msg_void_id(l, sel(c"setStringValue:"), ns_string(""));
            msg_void_id(parent, sel(c"addSubview:"), l);
        }
    }
    v.spectrum_lo_gr_label = gr_low;
    v.spectrum_mid_gr_label = gr_mid;
    v.spectrum_hi_gr_label = gr_hi;

    // Frequency tick labels under the spectrum — SF Mono digits,
    // tabular, so the columns don't shift between values.
    unsafe {
        let tick_font = ns_font_mono_digit(9.0, NS_FONT_WEIGHT_REGULAR);
        let spec_x0 = 20.0_f64;
        let spec_w = strip_w;
        let log_min = 20f64.log10();
        let log_max = 24000f64.log10();
        let tick_y = 36.0 + 64.0 + 2.0;
        for (hz, txt) in &[
            (50.0f64, "50"),
            (200.0, "200"),
            (1000.0, "1k"),
            (5000.0, "5k"),
            (15000.0, "15k"),
        ] {
            let t = ((*hz).log10() - log_min) / (log_max - log_min);
            let x = spec_x0 + t * spec_w - 15.0;
            let tk_raw = msg_id(label_class, sel(c"alloc"));
            let tk_rect = CGRect {
                origin: CGPoint { x, y: tick_y },
                size: CGSize {
                    width: 30.0,
                    height: 12.0,
                },
            };
            let tk = msg_id_rect(tk_raw, sel(c"initWithFrame:"), tk_rect);
            msg_void_id(tk, sel(c"setStringValue:"), ns_string(txt));
            msg_void_int(tk, sel(c"setBezeled:"), 0);
            msg_void_int(tk, sel(c"setDrawsBackground:"), 0);
            msg_void_int(tk, sel(c"setEditable:"), 0);
            msg_void_int(tk, sel(c"setSelectable:"), 0);
            msg_void_int(tk, sel(c"setAlignment:"), 1);
            msg_void_id(tk, sel(c"setFont:"), tick_font);
            msg_void_id(tk, sel(c"setTextColor:"), ns_color_token(theme::INK_MUTED));
            msg_void_id(parent, sel(c"addSubview:"), tk);
        }

        // dB scale labels on the right edge of the spectrum
        // (matches the gridline positions in update_spectrum).
        let db_min = -78.0_f32;
        let db_max = -6.0_f32;
        let db_range = db_max - db_min;
        let spec_y0 = r.origin.y;
        let strip_h = 64.0_f64;
        let db_font = ns_font_mono_digit(8.0, NS_FONT_WEIGHT_REGULAR);
        for &db in &[-12.0_f32, -36.0, -60.0] {
            let frac = ((db - db_min) / db_range).clamp(0.0, 1.0) as f64;
            let cy = spec_y0 + (1.0 - frac) * strip_h;
            let r_raw = msg_id(label_class, sel(c"alloc"));
            let r_rect = CGRect {
                origin: CGPoint {
                    x: r.origin.x + strip_w + 2.0,
                    y: cy - 5.0,
                },
                size: CGSize {
                    width: 20.0,
                    height: 10.0,
                },
            };
            let r_lbl = msg_id_rect(r_raw, sel(c"initWithFrame:"), r_rect);
            msg_void_int(r_lbl, sel(c"setBezeled:"), 0);
            msg_void_int(r_lbl, sel(c"setDrawsBackground:"), 0);
            msg_void_int(r_lbl, sel(c"setEditable:"), 0);
            msg_void_int(r_lbl, sel(c"setSelectable:"), 0);
            msg_void_int(r_lbl, sel(c"setAlignment:"), 2); // right
            msg_void_id(r_lbl, sel(c"setFont:"), db_font);
            msg_void_id(
                r_lbl,
                sel(c"setTextColor:"),
                ns_color_token(theme::INK_MUTED),
            );
            msg_void_id(
                r_lbl,
                sel(c"setStringValue:"),
                ns_string(&alloc::format!("{}", db as i32)),
            );
            msg_void_id(parent, sel(c"addSubview:"), r_lbl);
        }
    }

    // Section caption above the cards.

    // === Section: 3 band cards ===
    // Each card is now a v-contained per-band channel strip — its
    // full chain (compression curve + detection + timing + saturation)
    // lives inside the card, so the band is the unit of focus and the
    // old standalone DETECTION row is gone. The wide canvas lets the
    // ten per-band knobs sit in a 5×2 grid instead of a tall 2×4
    // stack: the biggest height saving over the old layout, and more
    // logical (related controls share a row).
    let _cards_y = 130.0;

    // === Global control footer ===
    // The crossover points are the ONLY global controls — everything
    // else (envelope, mix, drive, …) now lives per-band inside the
    // cards. A compact centred CROSSOVER panel holds the two split
    // frequencies; they also map to the draggable lines on the
    // spectrum above.
}
