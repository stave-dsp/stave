//! The 30 Hz bus→widget sync (values, styling, dimming,
//! repaints) + preset application + power styling.

use core::ffi::c_void;
use core::ptr;

extern crate alloc;

use crate::theme;
use crate::view::PluginEditorView;
use crate::*;

impl PluginEditorView {
    /// Reflect the host's bypass state in the brand mark. Wordmark fades
    /// to ~38% opacity; tagline switches between the spec caption and a
    /// warn-tinted "BYPASSED" tag. Called only on edge transitions so
    /// it's safe to rebuild the kerned attributed strings.
    /// Apply a factory preset: each value, clamped to its param's range,
    /// as a bracketed gesture so the host records it; the tick loop
    /// refreshes every widget from the table.
    pub(crate) fn apply_preset(&self, idx: usize) {
        let Some((_, pairs)) = self.presets.get(idx) else {
            return;
        };
        for &(ci, value) in pairs {
            let p = self.param(ci);
            let (lo, hi) = (p.normalized_to_plain(0.0), p.normalized_to_plain(1.0));
            self.write_control_plain(ci, value.clamp(lo.min(hi), lo.max(hi)), true);
        }
    }

    /// Restyle the top-bar power button for the current bypass state:
    /// ACTIVE = accent ring + soft glow; BYPASSED = muted, glow off.
    pub(crate) unsafe fn style_power_button(&self, bypassed: bool) {
        if self.bypass_button.is_null() {
            return;
        }
        let btn = self.bypass_button;
        let lyr = unsafe { msg_id(btn, sel(c"layer")) };
        let (ring, title, glow) = if bypassed {
            (
                theme::with_alpha(theme::LINE, 0.8),
                theme::INK_MUTED,
                0.0f32,
            )
        } else {
            (
                theme::with_alpha(theme::ACCENT, 0.95),
                theme::ACCENT,
                0.65f32,
            )
        };
        unsafe {
            if !lyr.is_null() {
                msg_void_ptr(
                    lyr,
                    sel(c"setBorderColor:"),
                    make_cg_color(ring.0, ring.1, ring.2, ring.3),
                );
                msg_void_ptr(
                    lyr,
                    sel(c"setShadowColor:"),
                    make_cg_color(theme::ACCENT.0, theme::ACCENT.1, theme::ACCENT.2, 1.0),
                );
                msg_void_f32(lyr, sel(c"setShadowOpacity:"), glow);
                msg_void_double(lyr, sel(c"setShadowRadius:"), 7.0);
                let f: unsafe extern "C" fn(Id, Sel, CGSize) =
                    core::mem::transmute(objc_msgSend as *const ());
                f(
                    lyr,
                    sel(c"setShadowOffset:"),
                    CGSize {
                        width: 0.0,
                        height: 0.0,
                    },
                );
            }
            button_set_attributed_title(
                btn,
                "\u{23FB}",
                ns_font_role((11.0, theme::W_SEMIBOLD)),
                ns_color_token(title),
                0.0,
            );
        }
    }

    unsafe fn apply_bypass_visual(&self, on: bool) {
        unsafe {
            self.style_power_button(on);
            // Wordmark opacity is the cheap signal — animatable on CALayer,
            // doesn't disturb glyph metrics.
            if !self.bypass_wordmark.is_null() {
                let layer = msg_id(self.bypass_wordmark, sel(c"layer"));
                if !layer.is_null() {
                    let f: unsafe extern "C" fn(Id, Sel, f32) =
                        core::mem::transmute(objc_msgSend as *const ());
                    f(layer, sel(c"setOpacity:"), if on { 0.38 } else { 1.0 });
                }
            }
            // Tagline carries the explicit "BYPASSED" affordance so it's not
            // just a brightness change someone could miss.
            if !self.bypass_tagline.is_null() {
                let (text, color, kern) = if on {
                    ("BYPASSED", ns_color_token(theme::WARN), 2.4)
                } else {
                    ("3-BAND  DYNAMICS", ns_color_token(theme::INK_MUTED), 1.6)
                };
                label_set_kerned(
                    self.bypass_tagline,
                    text,
                    ns_font_role(theme::role_label()),
                    color,
                    kern,
                );
            }
        }
    }

    /// Snapshot the three bands' SOLO control values (true = on). Used
    /// by tick() and update_spectrum() for cross-highlighting.
    pub(crate) fn read_solo_state(&self) -> [bool; 3] {
        let mut out = [false; 3];
        for (i, slot) in self.band_cards.iter().enumerate() {
            if let Some((_, ci)) = slot {
                let v = self.plain(*ci);
                out[i] = v > 0.5;
            }
        }
        out
    }

    pub fn tick(&mut self) {
        // Bypass-reactive brand mark: dim the wordmark + retitle the
        // tagline when the host has bypassed the plug-in. Only rebuild
        // the attributed string on edge changes to keep the tick cheap.
        if let Some(bp_ci) = self.bypass_ci {
            let on = self.plain(bp_ci) > 0.5;
            if on != self.bypass_last_on {
                unsafe { self.apply_bypass_visual(on) };
                self.bypass_last_on = on;
            }
        }
        // Update spectrum analyser if present.
        if !self.spectrum_view.is_null() {
            crate::widgets::spectrum::update_spectrum(self);
        }
        // Repaint the I/O meters every tick (their probe values change
        // continuously while audio runs).
        for &(_pi, view) in &self.meters {
            if !view.is_null() {
                unsafe {
                    let f: unsafe extern "C" fn(Id, Sel, bool) =
                        core::mem::transmute(objc_msgSend as *const ());
                    f(view, sel(c"setNeedsDisplay:"), true);
                }
            }
        }
        // Refresh knobs whose bus value changed since the last tick.
        for slot in self.knobs.iter_mut() {
            let ci = slot.0;
            let knob_view = slot.1;
            let value_lbl = slot.2;
            let cur_bits = self.binding.plain(ci).to_bits();
            if cur_bits != slot.3 {
                slot.3 = cur_bits;
                unsafe {
                    let f: unsafe extern "C" fn(Id, Sel, bool) =
                        core::mem::transmute(objc_msgSend as *const ());
                    f(knob_view, sel(c"setNeedsDisplay:"), true);
                    if !value_lbl.is_null() {
                        let txt = format_value(self.binding.param(ci));
                        let ns = ns_string(&txt);
                        msg_void_id(value_lbl, sel(c"setStringValue:"), ns);
                    }
                }
            }
        }
        // Repaint per-band transfer curves with current params + GR.
        crate::widgets::transfer::update_transfer_curves(self);
        crate::widgets::wave::update_waves(self);
        crate::widgets::history::update_histories(self);
        // Tab bars + show_when pages follow the bus like everything
        // else — automation and preset loads turn pages too.
        crate::widgets::tabs::update_tabs(self);
        crate::widgets::scene_widget::update_scene_widgets(self);
        crate::widgets::rings::update_rings(self);
        crate::widgets::dropdown::update_dropdowns(self);
        crate::widgets::cables::update_cables(self);
        crate::widgets::inspector::update_inspector(self);
        // Live GR badges on the spectrum per band — empty when idle.
        let band_gr_pairs: [(Option<usize>, Id); 3] = [
            (self.spectrum_lo_gr_probe, self.spectrum_lo_gr_label),
            (self.spectrum_mid_gr_probe, self.spectrum_mid_gr_label),
            (self.spectrum_hi_gr_probe, self.spectrum_hi_gr_label),
        ];
        for (pi_opt, lbl) in band_gr_pairs {
            if lbl.is_null() {
                continue;
            }
            let gr = pi_opt
                .map(|pi| {
                    f64::from_bits(
                        self.bus.probes[pi]
                            .latest
                            .load(core::sync::atomic::Ordering::Relaxed),
                    ) as f32
                })
                .unwrap_or(0.0);
            let mag = (-gr).clamp(0.0, 30.0);
            let txt = if mag < 0.1 {
                alloc::string::String::new()
            } else {
                alloc::format!("−{:.1} dB", mag)
            };
            unsafe {
                let ns = ns_string(&txt);
                msg_void_id(lbl, sel(c"setStringValue:"), ns);
            }
        }
        // Cross-highlight: dim non-soloed band cards (and spectrum
        // regions, handled inside update_spectrum) so SOLO becomes a
        // visual focus tool, not just an audio gate.
        // Push bus → dropdown selection for style and sat type. The
        // dropdowns are NSPopUpButtons; selectItemAtIndex: keeps the
        // visible selection in sync with the bus.
        for &(ci, btn) in self
            .style_buttons
            .iter()
            .chain(self.sat_type_buttons.iter())
        {
            let v = self.plain(ci) as i64 & 0b11;
            unsafe {
                let cur: i64 = {
                    let f: unsafe extern "C" fn(Id, Sel) -> i64 =
                        core::mem::transmute(objc_msgSend as *const ());
                    f(btn, sel(c"indexOfSelectedItem"))
                };
                if cur != v {
                    msg_void_int(btn, sel(c"selectItemAtIndex:"), v);
                }
            }
        }
        // Push bus → selection for generic stepped dropdowns (preset
        // loads and host automation move the bus underneath the UI).
        for &(ci, btn) in &self.stepped_dropdowns {
            let steps = self.param(ci).step_count() as usize;
            if steps < 2 {
                continue;
            }
            let v = (self.plain(ci) as i64).clamp(0, steps as i64 - 1);
            unsafe {
                let cur: i64 = {
                    let f: unsafe extern "C" fn(Id, Sel) -> i64 =
                        core::mem::transmute(objc_msgSend as *const ());
                    f(btn, sel(c"indexOfSelectedItem"))
                };
                if cur != v {
                    msg_void_int(btn, sel(c"selectItemAtIndex:"), v);
                }
            }
        }
        // Push bus → button.state for every SOLO/MUTE toggle. Bus is
        // authoritative; user clicks (TODO via target/action) will be
        // the only way state changes.
        for &(ci, btn) in &self.toggle_buttons {
            let v = self.plain(ci);
            unsafe {
                msg_void_int(btn, sel(c"setState:"), if v > 0.5 { 1 } else { 0 });
            }
        }
        let solos = self.read_solo_state();
        let any_solo = solos[0] || solos[1] || solos[2];
        for (i, slot) in self.band_cards.iter().enumerate() {
            if let Some((card, _)) = slot {
                let dim = any_solo && !solos[i];
                let target: f64 = if dim { 0.35 } else { 1.0 };
                unsafe {
                    // setAlphaValue on the view rather than layer opacity
                    // — the view-level alpha drives layer composition
                    // reliably even when implicit animations are in flight.
                    msg_void_double(*card, sel(c"setAlphaValue:"), target);
                }
            }
        }
        // Table-authoritative: drags route through the gesture path;
        // here the table's value is pushed to each slider and readout.
        for &(idx, slider, value_lbl) in &self.sliders {
            let p = self.param(idx);
            unsafe { msg_void_double(slider, sel(c"setDoubleValue:"), p.normalized()) };
            let txt = format_value(p);
            unsafe {
                let ns = ns_string(&txt);
                msg_void_id(value_lbl, sel(c"setStringValue:"), ns);
            }
        }
    }
}

// The 30 Hz driver — lives HERE because the timer exists to fire
// the tick.
impl crate::view::PluginEditorView {
    pub(crate) unsafe fn start_timer(&mut self) {
        // We use NSTimer's scheduledTimerWithTimeInterval:repeats:block:
        // form … but that requires a Block. Instead, run a CFRunLoop
        // periodic timer via NSTimer.scheduledTimerWithTimeInterval:
        // target:selector:userInfo:repeats:. Selector needs a method on
        // an NSObject. Since we don't have one, we lean on a small trick:
        // use NSTimer's block API via objc_msgSend with a stack-allocated
        // block descriptor.
        //
        // Simpler: poll via CFRunLoopAddTimer + CFRunLoopTimerCreate. Pass
        // our self pointer as userInfo. ObjC-free.
        let context = CFRunLoopTimerContext {
            version: 0,
            info: self.self_ptr as *mut c_void,
            retain: None,
            release: None,
            copy_description: None,
        };
        let interval = 1.0 / 30.0;
        let timer = unsafe {
            CFRunLoopTimerCreate(
                ptr::null(),
                CFAbsoluteTimeGetCurrent() + interval,
                interval,
                0,
                0,
                tick_callback,
                // CFRunLoopTimerCreate copies the context struct; the
                // *mut in the C signature is never written through.
                ptr::from_ref(&context).cast_mut().cast(),
            )
        };
        let runloop = unsafe { CFRunLoopGetCurrent() };
        unsafe { CFRunLoopAddTimer(runloop, timer, kCFRunLoopCommonModes) };
        self.timer = timer as Id;
    }
}
