//! The macOS widget host — the platform side of the two
//! stave-ui seams: `CocoaMetrics` measures text with the real theme
//! fonts; `mount` walks a solved `Node` tree and realises it with the
//! existing NSView/CG design-system widgets. A Windows/Linux (or wgpu
//! paint-everything) backend implements the same shape against the
//! same solved rects; nothing above this file changes.

use core::ffi::c_void;

use stave_core::editor::SlotShaping;
use stave_core::ui::{Kind, Layout, MeasureText, TextStyle, TypeRole};
use stave_core::{UiNode, Widget};

use crate::theme;

use super::view::PluginEditorView;
use super::*;

fn role_font(role: TypeRole) -> (f64, f64) {
    match role {
        TypeRole::Display => theme::role_wordmark(),
        TypeRole::Label => theme::role_label(),
        TypeRole::Tick => theme::role_tick(),
        TypeRole::Value => theme::role_value(),
        TypeRole::Caption => theme::role_label(),
        TypeRole::Overlay => theme::role_overlay(),
    }
}

/// Text measurement through a throwaway NSTextField carrying the SAME
/// kerned attributed string the painter mounts — metrics can't drift
/// from rendering. Build-time only (layout solve), never per-frame.
pub(crate) struct CocoaMetrics;

impl MeasureText for CocoaMetrics {
    fn measure(&self, text: &str, style: &TextStyle) -> (f64, f64) {
        unsafe {
            let label_class = class(c"NSTextField");
            let raw = msg_id(label_class, sel(c"alloc"));
            let rect = CGRect {
                origin: CGPoint { x: 0.0, y: 0.0 },
                size: CGSize {
                    width: 10_000.0,
                    height: 100.0,
                },
            };
            let label = msg_id_rect(raw, sel(c"initWithFrame:"), rect);
            msg_void_int(label, sel(c"setBezeled:"), 0);
            msg_void_int(label, sel(c"setDrawsBackground:"), 0);
            label_set_kerned(
                label,
                text,
                ns_font_role(role_font(style.role)),
                ns_color_token(theme::INK_HI),
                style.kern,
            );
            let f: unsafe extern "C" fn(Id, Sel, CGSize) -> CGSize =
                core::mem::transmute(objc_msgSend as *const ());
            let fit = f(
                label,
                sel(c"sizeThatFits:"),
                CGSize {
                    width: 10_000.0,
                    height: 100.0,
                },
            );
            let _: Id = msg_id(label, sel(c"autorelease"));
            (fit.width, fit.height)
        }
    }
}

/// Realise a solved tree: paint Box/Text primitives, delegate Widget
/// leaves to the design-system builders. `origin` shifts the solved
/// (root-relative) rects into `parent`'s coordinate space; `ci_of`
/// resolves declared ParamIds to control (table) indices.
pub(crate) unsafe fn mount(
    v: &mut PluginEditorView,
    parent: Id,
    node: &UiNode,
    l: &Layout,
    origin: (f64, f64),
    ci_of: &dyn Fn(stave_core::params::ParamId) -> Option<usize>,
) -> Id {
    let rect = CGRect {
        origin: CGPoint {
            x: origin.0 + l.rect.x,
            y: origin.1 + l.rect.y,
        },
        size: CGSize {
            width: l.rect.w,
            height: l.rect.h,
        },
    };
    match &node.kind {
        Kind::Box => {
            let styled = node.style.background.is_some()
                || node.style.border.is_some()
                || node.style.shadow.is_some();
            // `show_when` pages need a real view to hide, so a
            // conditional box always takes the container path.
            if styled || node.when.is_some() {
                // Styled boxes are real CONTAINERS (flipped, so the
                // solver's top-left origin holds inside): children
                // mount INTO them with box-relative coordinates —
                // this is what lets tick() dim a whole card by view.
                let bx = unsafe { styled_box(parent, rect, node) };
                if let Some((key, index)) = node.when {
                    if key & stave_core::editor::VIEW_WHEN_BIT != 0 {
                        // A VIEW page: editor-local state, starts at 0.
                        let k = (key & 0xFFFF) as u16;
                        let cur = v.tabs.view_state.get(&k).copied().unwrap_or(0);
                        unsafe {
                            msg_void_int(bx, sel(c"setHidden:"), (cur != index as i64) as i64)
                        };
                        v.tabs.pages.push(crate::widgets::tabs::Page {
                            view: bx,
                            bind: crate::widgets::tabs::Bind::View(k),
                            index,
                        });
                    } else if let Some(ci) = ci_of(stave_core::params::ParamId(key)) {
                        // Register + set the initial state; the tick
                        // keeps it live from the bus after this.
                        let cur = v.plain(ci) as i64;
                        unsafe {
                            msg_void_int(bx, sel(c"setHidden:"), (cur != index as i64) as i64)
                        };
                        v.tabs.pages.push(crate::widgets::tabs::Page {
                            view: bx,
                            bind: crate::widgets::tabs::Bind::Control(ci),
                            index,
                        });
                    }
                }
                for (c, cl) in node.children.iter().zip(l.children.iter()) {
                    unsafe { mount(v, bx, c, cl, (-l.rect.x, -l.rect.y), ci_of) };
                }
                return bx;
            }
            for (c, cl) in node.children.iter().zip(l.children.iter()) {
                unsafe { mount(v, parent, c, cl, origin, ci_of) };
            }
        }
        Kind::Text(s) => unsafe {
            let label_class = class(c"NSTextField");
            let raw = msg_id(label_class, sel(c"alloc"));
            let label = msg_id_rect(raw, sel(c"initWithFrame:"), rect);
            msg_void_int(label, sel(c"setBezeled:"), 0);
            msg_void_int(label, sel(c"setDrawsBackground:"), 0);
            msg_void_int(label, sel(c"setEditable:"), 0);
            msg_void_int(label, sel(c"setSelectable:"), 0);
            // Captions center; everything else reads left.
            if l.text.role == TypeRole::Caption {
                msg_void_int(label, sel(c"setAlignment:"), 1);
            }
            let col: theme::Rgba = (
                l.text.color.r,
                l.text.color.g,
                l.text.color.b,
                l.text.color.a,
            );
            label_set_kerned(
                label,
                s,
                ns_font_role(role_font(l.text.role)),
                ns_color_token(col),
                l.text.kern,
            );
            msg_void_id(parent, sel(c"addSubview:"), label);
            match node.tag {
                Some("bypass-dim") => {
                    msg_void_int(label, sel(c"setWantsLayer:"), 1);
                    v.bypass_wordmark = label;
                }
                Some("bypass-tag") => v.bypass_tagline = label,
                _ => {}
            }
        },
        Kind::Widget(w) => match w {
            Widget::Knob {
                id,
                band,
                parts,
                accent,
            } => {
                if let Some(ci) = ci_of(*id) {
                    // The node rect is the CELL; the knob diameter
                    // derives from cell height (grid cell 92 → 52,
                    // card cell ~74 → 40, rail trim ≤32 → as-is —
                    // the builder lays label/knob/value inside).
                    let h = rect.size.height;
                    let dia = if !parts.label && !parts.value {
                        rect.size.width.min(h)
                    } else if h >= 84.0 {
                        52.0
                    } else if h >= 60.0 {
                        40.0
                    } else {
                        h
                    };
                    unsafe {
                        v.add_knob_in_cell(
                            parent,
                            ci,
                            crate::widgets::knob_cell::KnobCell {
                                rect,
                                diameter: dia,
                                band: *band,
                                parts: *parts,
                                accent: *accent,
                            },
                        );
                    }
                }
            }
            Widget::Toggle { id, solo, band } => {
                if let Some(ci) = ci_of(*id) {
                    // SOLO toggles register their enclosing container
                    // for the per-card dimming tick (`parent` is the
                    // card box when composed inside one).
                    if *solo {
                        if let Some(slot) = band {
                            if *slot < v.band_cards.len() {
                                v.band_cards[*slot] = Some((parent, ci));
                            }
                        }
                    }
                    let rgb = band_rgb(*band);
                    unsafe { v.add_toggle(parent, ci, *solo, rect, rgb) };
                }
            }
            Widget::Selector { id, labels, pill } => {
                if let Some(ci) = ci_of(*id) {
                    if *pill {
                        unsafe { add_pill_dropdown(v, parent, ci, rect, labels) };
                    } else {
                        unsafe {
                            v.add_dropdown_in_cell(
                                parent,
                                ci,
                                rect.origin.x,
                                rect.origin.y,
                                rect.size.width,
                            );
                        }
                    }
                }
            }
            Widget::Wave { probe } => {
                if let Some(pi) = mapping::find_probe(&v.bus, probe) {
                    unsafe { crate::widgets::wave::add_wave(v, parent, pi, rect) };
                }
            }
            Widget::Tabs { id, labels } => {
                if let Some(ci) = ci_of(*id) {
                    unsafe {
                        crate::widgets::tabs::add_tab_bar(
                            v,
                            parent,
                            crate::widgets::tabs::Bind::Control(ci),
                            rect,
                            labels,
                        )
                    };
                }
            }
            Widget::ModSources => unsafe {
                crate::widgets::cables::add_source_jacks(v, parent, rect)
            },
            Widget::ModRings {
                slots,
                targets,
                probes,
                extras,
            } => {
                let slot_cis: Option<Vec<(usize, usize, usize)>> = slots
                    .iter()
                    .map(|(s, t, d)| Some((ci_of(*s)?, ci_of(*t)?, ci_of(*d)?)))
                    .collect();
                let target_cis: Vec<Option<usize>> =
                    targets.iter().map(|t| t.and_then(ci_of)).collect();
                let extra_cis: Vec<Option<SlotShaping<usize>>> = extras
                    .iter()
                    .map(|e| {
                        Some(SlotShaping {
                            via: ci_of(e.via)?,
                            mode: ci_of(e.mode)?,
                            lag: ci_of(e.lag)?,
                            quant: ci_of(e.quant)?,
                        })
                    })
                    .collect();
                if let Some(slot_cis) = slot_cis {
                    let mut rs = crate::widgets::rings::RingsState::new(
                        slot_cis,
                        target_cis,
                        probes.clone(),
                    );
                    rs.extras = extra_cis;
                    v.rings = Some(rs);
                }
            }
            Widget::Scene {
                params,
                probes,
                build,
                drag,
                drag_pair,
            } => {
                let cis: Option<Vec<usize>> = params.iter().map(|id| ci_of(*id)).collect();
                // A scene draws from its first paint: probes the engine
                // has not published yet are reserved, reading 0.0.
                let pis: Option<Vec<usize>> = probes
                    .iter()
                    .map(|p| mapping::find_or_reserve_probe(&v.bus, p))
                    .collect();
                if let (Some(cis), Some(pis)) = (cis, pis) {
                    unsafe {
                        crate::widgets::scene_widget::mount_scene(
                            v, parent, rect, cis, pis, *build, *drag, *drag_pair,
                        )
                    };
                }
            }
            Widget::ViewTabs { view, labels } => unsafe {
                crate::widgets::tabs::add_tab_bar(
                    v,
                    parent,
                    crate::widgets::tabs::Bind::View(*view),
                    rect,
                    labels,
                )
            },
            Widget::History { probe } => {
                if let Some(pi) = mapping::find_probe(&v.bus, probe) {
                    unsafe { crate::widgets::history::add_history(v, parent, pi, rect) };
                }
            }
            Widget::Meter { probe, style } => {
                if let Some(pi) = mapping::find_probe(&v.bus, probe) {
                    unsafe {
                        v.add_meter(parent, pi, rect, *style);
                    }
                }
            }
            Widget::Pad { x, y } => {
                if let (Some(cx), Some(cy)) = (ci_of(*x), ci_of(*y)) {
                    unsafe {
                        crate::widgets::pad::add_pad(v, parent, cx, cy, rect);
                    }
                }
            }
            Widget::Harmonics => unsafe {
                crate::widgets::engine::add_harmonics(
                    v,
                    parent,
                    rect.origin.x,
                    rect.origin.y,
                    rect.size.width,
                    rect.size.height,
                );
            },
            Widget::Topology(pills) => unsafe {
                *v.topo_pills.borrow_mut() = pills.clone();
                crate::widgets::engine::add_topology(
                    v,
                    parent,
                    rect.origin.x,
                    rect.origin.y,
                    rect.size.width,
                    rect.size.height,
                );
            },
            Widget::PresetBar { presets } => unsafe {
                let mut resolved_presets = Vec::with_capacity(presets.len());
                for preset in presets {
                    let mut pairs = Vec::with_capacity(preset.values.len());
                    for (id, value) in &preset.values {
                        let Some(control_index) = ci_of(*id) else {
                            pairs.clear();
                            break;
                        };
                        pairs.push((control_index, *value));
                    }
                    if pairs.len() == preset.values.len() {
                        resolved_presets.push((preset.name.to_string(), pairs));
                    }
                }
                v.presets = resolved_presets;
                v.add_preset_pill(parent, rect);
            },
            Widget::TransferPlot { keys, band } => {
                let t = ci_of(keys.threshold);
                let r = ci_of(keys.ratio);
                let mk = ci_of(keys.makeup);
                let gr = mapping::find_probe(&v.bus, keys.gr_probe);
                if let (Some(t), Some(r), Some(mk), Some(gr)) = (t, r, mk, gr) {
                    unsafe {
                        add_transfer_plot(
                            v,
                            parent,
                            rect,
                            (t, r, mk),
                            keys.style.and_then(&ci_of),
                            keys.knee.and_then(&ci_of),
                            gr,
                            band_rgb(Some(*band)),
                        );
                    }
                }
            }
            Widget::Power(id) => {
                if let Some(ci) = ci_of(*id) {
                    unsafe { v.add_power_button(parent, rect, ci) };
                }
            }
            Widget::Spectrum(keys) => unsafe {
                crate::widgets::spectrum::create_spectrum(v, parent, rect, keys, ci_of);
            },
        },
        Kind::Glyph(_name) => {
            // Icon registry lands with the first composed glyph use
            // (power symbol); until then glyphs occupy layout only.
        }
        Kind::Shape(sh) => unsafe { paint_shape(parent, rect, node, *sh) },
        Kind::Spacer => {}
    }
    core::ptr::null_mut()
}

/// Vector primitives as layer-backed views: Rect/RRect/Line fill with
/// `background` (radius for RRect from style); Circle = RRect with
/// radius = min(w,h)/2. Strokes via `border`.
unsafe fn paint_shape(parent: Id, rect: CGRect, node: &UiNode, sh: stave_core::ui::Shape) {
    unsafe {
        let view_cls = class(c"NSView");
        let raw = msg_id(view_cls, sel(c"alloc"));
        let sv = msg_id_rect(raw, sel(c"initWithFrame:"), rect);
        msg_void_int(sv, sel(c"setWantsLayer:"), 1);
        let layer = msg_id(sv, sel(c"layer"));
        if !layer.is_null() {
            let f: unsafe extern "C" fn(Id, Sel, *mut c_void) =
                core::mem::transmute(objc_msgSend as *const ());
            if let Some(bg) = node.style.background {
                f(
                    layer,
                    sel(c"setBackgroundColor:"),
                    make_cg_color(bg.r, bg.g, bg.b, bg.a),
                );
            }
            let fr: unsafe extern "C" fn(Id, Sel, f64) =
                core::mem::transmute(objc_msgSend as *const ());
            let radius = match sh {
                stave_core::ui::Shape::RRect => node.style.radius,
                stave_core::ui::Shape::Circle => rect.size.width.min(rect.size.height) * 0.5,
                _ => 0.0,
            };
            fr(layer, sel(c"setCornerRadius:"), radius);
            if let Some(b) = node.style.border {
                fr(layer, sel(c"setBorderWidth:"), b.width);
                f(
                    layer,
                    sel(c"setBorderColor:"),
                    make_cg_color(b.color.r, b.color.g, b.color.b, b.color.a),
                );
            }
        }
        msg_void_id(parent, sel(c"addSubview:"), sv);
    }
}

fn band_rgb(band: Option<usize>) -> (f64, f64, f64) {
    match band {
        Some(0) => theme::BAND_LOW,
        Some(1) => theme::BAND_MID,
        Some(2) => theme::BAND_HIGH,
        _ => theme::BAND_MID,
    }
}

/// Paint a Box's background/border/radius/shadow as a layer-backed
/// FLIPPED container (the solver's top-left origin holds inside).
/// Returns the container view — children mount into it.
unsafe fn styled_box(parent: Id, rect: CGRect, node: &UiNode) -> Id {
    unsafe {
        let view_cls = flipped_view_class();
        let raw = msg_id(view_cls, sel(c"alloc"));
        let bx = msg_id_rect(raw, sel(c"initWithFrame:"), rect);
        msg_void_int(bx, sel(c"setWantsLayer:"), 1);
        let layer = msg_id(bx, sel(c"layer"));
        if !layer.is_null() {
            let f: unsafe extern "C" fn(Id, Sel, *mut c_void) =
                core::mem::transmute(objc_msgSend as *const ());
            if let Some(bg) = node.style.background {
                f(
                    layer,
                    sel(c"setBackgroundColor:"),
                    make_cg_color(bg.r, bg.g, bg.b, bg.a),
                );
            }
            let fr: unsafe extern "C" fn(Id, Sel, f64) =
                core::mem::transmute(objc_msgSend as *const ());
            fr(layer, sel(c"setCornerRadius:"), node.style.radius);
            if let Some(b) = node.style.border {
                fr(layer, sel(c"setBorderWidth:"), b.width);
                f(
                    layer,
                    sel(c"setBorderColor:"),
                    make_cg_color(b.color.r, b.color.g, b.color.b, b.color.a),
                );
            }
            if let Some(sh) = node.style.shadow {
                f(
                    layer,
                    sel(c"setShadowColor:"),
                    make_cg_color(0.0, 0.0, 0.0, 1.0),
                );
                let fo: unsafe extern "C" fn(Id, Sel, f32) =
                    core::mem::transmute(objc_msgSend as *const ());
                fo(layer, sel(c"setShadowOpacity:"), sh.opacity as f32);
                fr(layer, sel(c"setShadowRadius:"), sh.radius);
                let foff: unsafe extern "C" fn(Id, Sel, CGSize) =
                    core::mem::transmute(objc_msgSend as *const ());
                foff(
                    layer,
                    sel(c"setShadowOffset:"),
                    CGSize {
                        width: 0.0,
                        height: sh.dy,
                    },
                );
                msg_void_int(bx, sel(c"setClipsToBounds:"), 0);
            }
        }
        msg_void_id(parent, sel(c"addSubview:"), bx);
        bx
    }
}

/// Compact selector — the design-system DROPDOWN (see
/// widgets::dropdown). Items come from the control's DECLARED step
/// names — no hardcoded label arrays.
pub(crate) unsafe fn add_pill_dropdown(
    v: &mut PluginEditorView,
    parent: Id,
    ci: usize,
    rect: CGRect,
    _labels: &[&str],
) {
    unsafe { crate::widgets::dropdown::add_dropdown_field(v, parent, ci, rect) };
}

/// The per-band transfer plot: curve NSImageView + dB corner ticks +
/// live GR overlay, cached for the tick repaint. `rect` is the curve
/// area; overlays position relative to it.
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn add_transfer_plot(
    v: &mut PluginEditorView,
    parent: Id,
    rect: CGRect,
    cis: (usize, usize, usize),
    style_ci: Option<usize>,
    knee_ci: Option<usize>,
    gr_pi: usize,
    accent_rgb: (f64, f64, f64),
) {
    unsafe {
        let label_class = class(c"NSTextField");
        let curve_class = class(c"NSImageView");
        let curve_raw = msg_id(curve_class, sel(c"alloc"));
        let curve_view = msg_id_rect(curve_raw, sel(c"initWithFrame:"), rect);
        msg_void_int(curve_view, sel(c"setImageScaling:"), 0);
        msg_void_int(curve_view, sel(c"setWantsLayer:"), 1);
        let l = msg_id(curve_view, sel(c"layer"));
        if !l.is_null() {
            let bg = make_cg_color(0.07, 0.07, 0.09, 1.0);
            let f: unsafe extern "C" fn(Id, Sel, *mut c_void) =
                core::mem::transmute(objc_msgSend as *const ());
            f(l, sel(c"setBackgroundColor:"), bg);
            let f2: unsafe extern "C" fn(Id, Sel, f64) =
                core::mem::transmute(objc_msgSend as *const ());
            f2(l, sel(c"setCornerRadius:"), 3.0);
        }
        msg_void_id(parent, sel(c"addSubview:"), curve_view);

        // Corner dB ticks: −60 lower-left, 0 upper-right.
        let tick_font = ns_font_mono_digit(8.0, NS_FONT_WEIGHT_REGULAR);
        let tick_color = ns_color_token(theme::INK_MUTED);
        let lo_raw = msg_id(label_class, sel(c"alloc"));
        let lo_rect = CGRect {
            origin: CGPoint {
                x: rect.origin.x + 2.0,
                y: rect.origin.y + rect.size.height - 12.0,
            },
            size: CGSize {
                width: 24.0,
                height: 10.0,
            },
        };
        let lo = msg_id_rect(lo_raw, sel(c"initWithFrame:"), lo_rect);
        for m in [
            c"setBezeled:",
            c"setDrawsBackground:",
            c"setEditable:",
            c"setSelectable:",
        ] {
            msg_void_int(lo, sel(m), 0);
        }
        msg_void_id(lo, sel(c"setFont:"), tick_font);
        msg_void_id(lo, sel(c"setTextColor:"), tick_color);
        msg_void_id(lo, sel(c"setStringValue:"), ns_string("−60"));
        msg_void_id(parent, sel(c"addSubview:"), lo);

        let hi_raw = msg_id(label_class, sel(c"alloc"));
        let hi_rect = CGRect {
            origin: CGPoint {
                x: rect.origin.x + rect.size.width - 16.0,
                y: rect.origin.y + 2.0,
            },
            size: CGSize {
                width: 14.0,
                height: 10.0,
            },
        };
        let hi = msg_id_rect(hi_raw, sel(c"initWithFrame:"), hi_rect);
        for m in [
            c"setBezeled:",
            c"setDrawsBackground:",
            c"setEditable:",
            c"setSelectable:",
        ] {
            msg_void_int(hi, sel(m), 0);
        }
        msg_void_int(hi, sel(c"setAlignment:"), 2);
        msg_void_id(hi, sel(c"setFont:"), tick_font);
        msg_void_id(hi, sel(c"setTextColor:"), tick_color);
        msg_void_id(hi, sel(c"setStringValue:"), ns_string("0"));
        msg_void_id(parent, sel(c"addSubview:"), hi);

        // Live GR readout — accent mono, bottom-right, blank until active.
        let gr_raw = msg_id(label_class, sel(c"alloc"));
        let gr_rect = CGRect {
            origin: CGPoint {
                x: rect.origin.x + rect.size.width - 72.0,
                y: rect.origin.y + rect.size.height - 14.0,
            },
            size: CGSize {
                width: 56.0,
                height: 12.0,
            },
        };
        let gr_text = msg_id_rect(gr_raw, sel(c"initWithFrame:"), gr_rect);
        for m in [
            c"setBezeled:",
            c"setDrawsBackground:",
            c"setEditable:",
            c"setSelectable:",
        ] {
            msg_void_int(gr_text, sel(m), 0);
        }
        msg_void_int(gr_text, sel(c"setAlignment:"), 2);
        msg_void_id(
            gr_text,
            sel(c"setFont:"),
            ns_font_mono_digit(8.5, NS_FONT_WEIGHT_SEMIBOLD),
        );
        msg_void_id(
            gr_text,
            sel(c"setTextColor:"),
            ns_color(accent_rgb.0, accent_rgb.1, accent_rgb.2, 1.0),
        );
        msg_void_id(gr_text, sel(c"setStringValue:"), ns_string(""));
        msg_void_id(parent, sel(c"addSubview:"), gr_text);

        v.transfer_curves
            .push(crate::widgets::transfer::TransferCurve {
                view: curve_view,
                gr_text,
                thresh_ci: cis.0,
                ratio_ci: cis.1,
                makeup_ci: cis.2,
                style_ci,
                knee_ci,
                gr_pi,
                accent_rgb,
                op_trail: alloc::vec::Vec::with_capacity(12),
            });
    }
}

extern crate alloc;

/// The OWNED window-rooted tree path: a plugin's composed tree, mounted
/// verbatim. `false` when the plugin declares none.
pub(crate) unsafe fn try_attach_owned(v: &mut PluginEditorView, parent_ns_view: Id) -> bool {
    {
        if let Some(tree) = v.ui.clone() {
            unsafe {
                let target_cls = toggle_target_class();
                let target_raw = msg_id(target_cls, sel(c"alloc"));
                let target = msg_id(target_raw, sel(c"init"));
                let ivar = class_getInstanceVariable(target_cls, c"editor_ptr".as_ptr());
                if !ivar.is_null() && !target.is_null() {
                    object_setIvar(target, ivar, v.self_ptr as Id);
                }
                v.toggle_target = target;
            }
            let table = v.binding.table.clone();
            let ci_of = move |id: stave_core::params::ParamId| -> Option<usize> {
                (0..table.count()).find(|&i| table.by_index(i).map(|p| p.meta().id) == Some(id))
            };
            v.dropdowns.host = parent_ns_view;
            let solved = stave_core::ui::solve(&tree, (4_000.0, 4_000.0), &CocoaMetrics);
            unsafe {
                mount(v, parent_ns_view, &tree, &solved, (0.0, 0.0), &ci_of);
                // Inlet jacks need the mounted knobs; the cable
                // overlay must sit ABOVE the whole tree.
                crate::widgets::cables::mount_target_jacks_and_overlay(v, parent_ns_view);
                v.start_timer();
            }
            return true;
        }
    }
    false
}

impl PluginEditorView {
    /// Window size for owned trees: the solved root IS the window.
    pub fn ui_root_dims(&self) -> Option<(f64, f64)> {
        let tree = self.ui.as_ref()?;
        let solved = stave_core::ui::solve(tree, (4_000.0, 4_000.0), &crate::mount::CocoaMetrics);
        Some((solved.rect.w, solved.rect.h))
    }
}
