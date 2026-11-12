//! `TransferCurve` — the per-band compressor transfer plot's bound
//! control indices + trail state (split from view.rs; the drawing
//! stays with the view tick that owns it).

extern crate alloc;

use crate::objc::Id;

/// Logical (point) width of the card transfer plot.
const PLOT_LOGICAL_W: f64 = 340.0;

pub(crate) struct TransferCurve {
    pub(crate) view: Id,
    /// Small NSTextField overlay in the corner of the curve showing
    /// the live GR value when active, hidden when idle.
    pub(crate) gr_text: Id,
    pub(crate) thresh_ci: usize,
    pub(crate) ratio_ci: usize,
    pub(crate) makeup_ci: usize,
    /// Band's style param (0=VCA, 1=FET, 2=OPTO, 3=VMU). Drives knee
    /// curvature in the visualisation so the curve shape mirrors
    /// what the audio path is actually doing.
    pub(crate) style_ci: Option<usize>,
    /// Explicit per-band knee width override (dB). > 0 wins over
    /// the style's bundled knee.
    pub(crate) knee_ci: Option<usize>,
    pub(crate) gr_pi: usize,
    pub(crate) accent_rgb: (f64, f64, f64),
    /// Last ~12 operating-point (px, py) positions, used to draw a
    /// fading trail behind the live dot. Reveals envelope character:
    /// fast (FET/VCA) styles smear less, slow (OPTO/VMU) styles
    /// leave a visible streak as compression eases in.
    pub(crate) op_trail: alloc::vec::Vec<(i32, i32)>,
}

use crate::cg::set_image_view_pixels;
use crate::theme;
use crate::view::PluginEditorView;
use crate::*;

/// Repaint each band's transfer-curve view with the current
/// threshold/ratio/makeup values + a live operating-point dot
/// derived from the band's GR probe.
pub(crate) fn update_transfer_curves(v: &mut PluginEditorView) {
    // Plot canvas at 2× backing. Logical size = card_w-28 × 52 pts;
    // the band cards are MB_CARD_W wide, so logical W = 340, H = 52.
    const W: usize = 680;
    const H: usize = 104;
    const DB_LO: f32 = -60.0;
    const DB_HI: f32 = 0.0;
    let db_range = DB_HI - DB_LO;
    let x_for_db =
        |db: f32| -> i32 { (((db - DB_LO) / db_range).clamp(0.0, 1.0) * (W as f32 - 1.0)) as i32 };
    let y_for_db = |db: f32| -> i32 {
        ((1.0 - ((db - DB_LO) / db_range).clamp(0.0, 1.0)) * (H as f32 - 1.0)) as i32
    };

    for tc in v.transfer_curves.iter_mut() {
        let thresh = v.binding.plain(tc.thresh_ci) as f32;
        let ratio = v.binding.plain(tc.ratio_ci) as f32;
        let makeup = v.binding.plain(tc.makeup_ci) as f32;
        let gr_db = f64::from_bits(
            v.bus.probes[tc.gr_pi]
                .latest
                .load(core::sync::atomic::Ordering::Relaxed),
        ) as f32;
        let ratio = ratio.max(1.0);
        // Knee width mirrors the DSP's per-style knee so a switch
        // to OPTO / VMU visibly softens the corner.
        let style = tc
            .style_ci
            .map(|ci| v.binding.plain(ci) as i32 & 0b11)
            .unwrap_or(0);
        // Style's bundled knee, then explicit override.
        let style_knee = match style {
            0 => 0.5_f32,
            1 => 2.0,
            2 => 6.0,
            _ => 10.0,
        };
        let knee_user = tc
            .knee_ci
            .map(|ci| v.binding.plain(ci) as f32)
            .unwrap_or(0.0);
        let knee_width = if knee_user > 0.0 {
            knee_user
        } else {
            style_knee
        };

        let mut pixels = alloc::vec![0_u8; W * H * 4];
        // Clear to transparent — the NSImageView's CALayer paints
        // the dark backdrop.
        let (ax_r, ax_g, ax_b, _) = theme::rgba_u8(theme::AXIS);
        // Faint unity diagonal — 2 px tall on 2× backing.
        for x in 0..(W as i32) {
            let frac = x as f32 / (W as f32 - 1.0);
            let db = DB_LO + frac * db_range;
            let y0 = y_for_db(db);
            for dy in 0..2 {
                let y = y0 + dy;
                if y >= 0 && (y as usize) < H {
                    let p = ((y as usize) * W + x as usize) * 4;
                    pixels[p] = ax_r;
                    pixels[p + 1] = ax_g;
                    pixels[p + 2] = ax_b;
                    pixels[p + 3] = 110;
                }
            }
        }
        // Threshold guide crosshair — 2 px on 2× backing. Slightly
        // dimmer than the unity diagonal so the curve still pops.
        let cross_alpha = 100_u8;
        let dim_r = (ax_r as u16 * 86 / 100) as u8;
        let dim_g = (ax_g as u16 * 86 / 100) as u8;
        let dim_b = (ax_b as u16 * 86 / 100) as u8;
        let tx = x_for_db(thresh);
        let ty = y_for_db(thresh);
        for dx in 0..2 {
            if tx + dx >= 0 && ((tx + dx) as usize) < W {
                for y in 0..H {
                    let p = (y * W + (tx + dx) as usize) * 4;
                    pixels[p] = pixels[p].max(dim_r);
                    pixels[p + 1] = pixels[p + 1].max(dim_g);
                    pixels[p + 2] = pixels[p + 2].max(dim_b);
                    pixels[p + 3] = pixels[p + 3].max(cross_alpha);
                }
            }
        }
        for dy in 0..2 {
            if ty + dy >= 0 && ((ty + dy) as usize) < H {
                for x in 0..W {
                    let p = ((ty + dy) as usize) * W + x;
                    let p = p * 4;
                    pixels[p] = pixels[p].max(dim_r);
                    pixels[p + 1] = pixels[p + 1].max(dim_g);
                    pixels[p + 2] = pixels[p + 2].max(dim_b);
                    pixels[p + 3] = pixels[p + 3].max(cross_alpha);
                }
            }
        }

        // Compressor curve in band accent with a soft knee around
        // the threshold. The plugin's actual response is a hard
        // knee, but a small visual softening (±3 dB) reads more
        // premium and is closer to what most "product" comps draw.
        let (ar, ag, ab) = (
            (tc.accent_rgb.0 * 255.0) as u8,
            (tc.accent_rgb.1 * 255.0) as u8,
            (tc.accent_rgb.2 * 255.0) as u8,
        );
        // Dim version for the row above the spine (gives the curve
        // a 2 px presence without the rasterised stairstep look).
        let (har, hag, hab) = (
            ((tc.accent_rgb.0 * 0.62) * 255.0) as u8,
            ((tc.accent_rgb.1 * 0.62) * 255.0) as u8,
            ((tc.accent_rgb.2 * 0.62) * 255.0) as u8,
        );
        // knee_width is the style-driven value computed above
        // (VCA hard, OPTO/VMU softer); a switch flips the curve
        // shape on-screen.
        let mut prev_y: Option<i32> = None;
        for x in 0..(W as i32) {
            let frac = x as f32 / (W as f32 - 1.0);
            let input = DB_LO + frac * db_range;
            // Soft-knee crossfade:
            //   t = 0 below the knee (input < thresh - knee/2)
            //   t = 1 above the knee (input > thresh + knee/2)
            //   smoothstep in between
            let t = (((input - (thresh - knee_width * 0.5)) / knee_width).clamp(0.0, 1.0)).powi(2)
                * (3.0
                    - 2.0 * (((input - (thresh - knee_width * 0.5)) / knee_width).clamp(0.0, 1.0)));
            let lin = input + makeup;
            let comp = thresh + (input - thresh) / ratio + makeup;
            let output = lin * (1.0 - t) + comp * t;
            let y = y_for_db(output.clamp(DB_LO, DB_HI));
            // Spine: 2 px bright band accent (= 1 logical pt at 2× backing).
            for dy in 0..2 {
                let yy = y + dy;
                if yy >= 0 && (yy as usize) < H {
                    let p = ((yy as usize) * W + x as usize) * 4;
                    pixels[p] = ar;
                    pixels[p + 1] = ag;
                    pixels[p + 2] = ab;
                    pixels[p + 3] = 245;
                }
            }
            // Halo above the spine — 2 px dimmer band accent.
            for dy in -2..0 {
                let yy = y + dy;
                if yy >= 0 && (yy as usize) < H {
                    let p = ((yy as usize) * W + x as usize) * 4;
                    if pixels[p + 3] < 160 {
                        pixels[p] = har;
                        pixels[p + 1] = hag;
                        pixels[p + 2] = hab;
                        pixels[p + 3] = 160;
                    }
                }
            }
            // Bridge gaps when the curve drops faster than 1 px/col.
            if let Some(py) = prev_y {
                let (a, b) = if py < y { (py + 1, y) } else { (y, py - 1) };
                for yy in a..=b {
                    if yy >= 0 && (yy as usize) < H {
                        let p = ((yy as usize) * W + x as usize) * 4;
                        pixels[p] = ar;
                        pixels[p + 1] = ag;
                        pixels[p + 2] = ab;
                        pixels[p + 3] = 210;
                    }
                }
            }
            prev_y = Some(y);
        }

        // Live operating-point dot — only shown when compression is
        // actually happening so the moving-dot affordance is reserved
        // for activity rather than always parked at the knee.
        let mag = (-gr_db).clamp(0.0, 30.0);
        let active = mag >= 0.1;
        if !active {
            tc.op_trail.clear();
        }
        if active {
            let op_input = (thresh + mag * ratio).clamp(DB_LO, DB_HI);
            let op_output = (thresh + (op_input - thresh) / ratio + makeup).clamp(DB_LO, DB_HI);
            let ox = x_for_db(op_input);
            let oy = y_for_db(op_output);

            // Roll the trail. Oldest entry drops, newest pushed.
            const TRAIL_LEN: usize = 12;
            if tc.op_trail.len() >= TRAIL_LEN {
                tc.op_trail.remove(0);
            }
            tc.op_trail.push((ox, oy));

            // Render trail behind the live dot — oldest fades out
            // toward transparent. Each step a small 3 px square.
            let trail_len = tc.op_trail.len();
            for (idx, &(tx, ty)) in tc.op_trail.iter().enumerate() {
                if idx + 1 == trail_len {
                    continue; // skip the latest (the dot draws it)
                }
                let age = trail_len - 1 - idx; // 0 = newest of trail
                let alpha = (180.0 * (1.0 - age as f32 / TRAIL_LEN as f32)) as u8;
                for dy in -2..=2 {
                    for dx in -2..=2 {
                        let x = tx + dx;
                        let y = ty + dy;
                        if x < 0 || y < 0 || (x as usize) >= W || (y as usize) >= H {
                            continue;
                        }
                        let dist2 = dx * dx + dy * dy;
                        if dist2 > 4 {
                            continue;
                        }
                        let p = ((y as usize) * W + x as usize) * 4;
                        if pixels[p + 3] < alpha {
                            pixels[p] = ar;
                            pixels[p + 1] = ag;
                            pixels[p + 2] = ab;
                            pixels[p + 3] = alpha;
                        }
                    }
                }
            }
            // Radii doubled for 2× backing so the visible halo
            // stays roughly the same logical size (~5 pt across).
            for dy in -10..=10 {
                for dx in -10..=10 {
                    let x = ox + dx;
                    let y = oy + dy;
                    if x < 0 || y < 0 || (x as usize) >= W || (y as usize) >= H {
                        continue;
                    }
                    let dist2 = dx * dx + dy * dy;
                    if dist2 > 100 {
                        continue;
                    }
                    let p = ((y as usize) * W + x as usize) * 4;
                    if dist2 <= 16 {
                        pixels[p] = 255;
                        pixels[p + 1] = 255;
                        pixels[p + 2] = 255;
                        pixels[p + 3] = 255;
                    } else if dist2 <= 64 {
                        pixels[p] = ar;
                        pixels[p + 1] = ag;
                        pixels[p + 2] = ab;
                        pixels[p + 3] = 200;
                    } else {
                        pixels[p] = ar;
                        pixels[p + 1] = ag;
                        pixels[p + 2] = ab;
                        pixels[p + 3] = 80;
                    }
                }
            }
        }

        unsafe { set_image_view_pixels(tc.view, &pixels, W, H, PLOT_LOGICAL_W, 52.0) };

        // GR readout text — only when active.
        let txt = if active {
            alloc::format!("−{:.1} dB", mag)
        } else {
            alloc::string::String::new()
        };
        unsafe {
            let ns = ns_string(&txt);
            msg_void_id(tc.gr_text, sel(c"setStringValue:"), ns);
        }
    }
}
