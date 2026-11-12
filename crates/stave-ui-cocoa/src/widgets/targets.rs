// The shared NSObject action target — StaveToggleTarget serves
// every toggle, slider, dropdown, preset pill, and tab bar in an
// editor.

use core::ffi::{c_char, c_void};

extern crate alloc;

use crate::objc::*;
use crate::view::PluginEditorView;
use crate::*;

/// The shared preamble of every StaveToggleTarget action: recover the
/// editor from the target's ivar. One spelling, five handlers.
unsafe fn target_editor(this: Id) -> Option<&'static PluginEditorView> {
    let ivar = unsafe {
        class_getInstanceVariable(
            objc_lookUpClass(c"StaveToggleTarget".as_ptr()),
            c"editor_ptr".as_ptr(),
        )
    };
    if ivar.is_null() {
        return None;
    }
    let editor_ptr = unsafe { object_getIvar(this, ivar) } as *const PluginEditorView;
    unsafe { editor_ptr.as_ref() }
}

/// Mutable twin (view-state writes — the tab handler). Same ivar,
/// same lifetime contract: the editor outlives its targets.
unsafe fn target_editor_mut(this: Id) -> Option<&'static mut PluginEditorView> {
    let ivar = unsafe {
        class_getInstanceVariable(
            objc_lookUpClass(c"StaveToggleTarget".as_ptr()),
            c"editor_ptr".as_ptr(),
        )
    };
    if ivar.is_null() {
        return None;
    }
    let editor_ptr = unsafe { object_getIvar(this, ivar) } as *mut PluginEditorView;
    unsafe { editor_ptr.as_mut() }
}

/// An NSControl's integer message (tag, indexOfSelectedItem, …).
unsafe fn sender_i64(sender: Id, message: &core::ffi::CStr) -> i64 {
    unsafe {
        let f: unsafe extern "C" fn(Id, Sel) -> i64 =
            core::mem::transmute(objc_msgSend as *const ());
        f(sender, sel(message))
    }
}

unsafe fn select_relative_preset(this: Id, offset: isize) {
    let Some(editor) = (unsafe { target_editor_mut(this) }) else {
        return;
    };
    crate::widgets::dropdown::select_relative_preset(editor, offset);
}

pub(crate) unsafe extern "C" fn toggle_target_previous_preset(this: Id, _cmd: Sel, _sender: Id) {
    unsafe { select_relative_preset(this, -1) };
}

pub(crate) unsafe extern "C" fn toggle_target_next_preset(this: Id, _cmd: Sel, _sender: Id) {
    unsafe { select_relative_preset(this, 1) };
}

/// Click handler for SOLO/MUTE buttons. Reads the sender's tag (=ci),
/// the receiver's `editor_ptr` ivar (=PluginEditorView*), flips the
/// param's value, then forces the button's setState to match
/// so the UI doesn't fight us until the next 30 Hz tick.
pub(crate) unsafe extern "C" fn toggle_target_click(this: Id, _cmd: Sel, sender: Id) {
    let Some(editor) = (unsafe { target_editor(this) }) else {
        return;
    };
    let ci = unsafe { sender_i64(sender, c"tag") } as usize;
    if ci >= editor.param_count() {
        return;
    }
    let cur = editor.plain(ci);
    let new_val: f64 = if cur > 0.5 { 0.0 } else { 1.0 };
    editor.write_control_plain(ci, new_val, true);
    // Keep the button visual in sync immediately.
    let state_arg: i64 = if new_val > 0.5 { 1 } else { 0 };
    unsafe { msg_void_int(sender, sel(c"setState:"), state_arg) };
}

/// NSPopUpButton selection-changed handler. Sender's tag = ci of the
/// param; the new value is `indexOfSelectedItem` (0..N-1).
pub(crate) unsafe extern "C" fn toggle_target_dropdown_changed(this: Id, _cmd: Sel, sender: Id) {
    let Some(editor) = (unsafe { target_editor(this) }) else {
        return;
    };
    let ci = unsafe { sender_i64(sender, c"tag") } as usize;
    if ci >= editor.param_count() {
        return;
    }
    let idx = unsafe { sender_i64(sender, c"indexOfSelectedItem") };
    let last = i64::from(editor.param(ci).step_count().max(1)) - 1;
    editor.write_control_plain(ci, idx.clamp(0, last) as f64, true);
}

/// Style-cycle click. Reads the param's current value, rotates it
/// 0→1→2→3→0 (style index space), writes back. Tick repaints the
/// button title to the new style name.
pub(crate) unsafe extern "C" fn toggle_target_cycle_style(this: Id, _cmd: Sel, sender: Id) {
    let Some(editor) = (unsafe { target_editor(this) }) else {
        return;
    };
    let ci = unsafe { sender_i64(sender, c"tag") } as usize;
    if ci >= editor.param_count() {
        return;
    }
    let cur = editor.plain(ci);
    let next = (((cur as i32) + 1) & 0b11) as f64;
    editor.write_control_plain(ci, next, true);
}

/// Slider drag handler. NSSlider posts its action continuously while
/// dragging when setContinuous:YES (default). Sender's tag = ci; we
/// read the normalised position, denormalise, store to bus.
pub(crate) unsafe extern "C" fn toggle_target_slider_moved(this: Id, _cmd: Sel, sender: Id) {
    let Some(editor) = (unsafe { target_editor(this) }) else {
        return;
    };
    let ci = unsafe { sender_i64(sender, c"tag") } as usize;
    if ci >= editor.param_count() {
        return;
    }
    let norm = unsafe { msg_double(sender, sel(c"doubleValue")) };
    let plain = editor.param(ci).normalized_to_plain(norm.clamp(0.0, 1.0));
    // NSSlider gives no down/up hooks here; bracket per move.
    editor.write_control_plain(ci, plain, true);
}

/// Tab-chip click. The tag packs (page index << 16 | key) — see
/// `widgets::tabs` — because one i64 is all NSControl carries.
pub(crate) unsafe extern "C" fn toggle_target_tab_picked(this: Id, _cmd: Sel, sender: Id) {
    let Some(editor) = (unsafe { target_editor_mut(this) }) else {
        return;
    };
    let tag = unsafe { sender_i64(sender, c"tag") };
    let key = tag & 0xFFFF;
    let idx = tag >> crate::widgets::tabs::TAB_TAG_SHIFT;
    if key & crate::widgets::tabs::TAB_VIEW_BIT != 0 {
        // A VIEW bar: editor-local page flip — no param, no gesture,
        // no session (the tick restyles + flips pages from here).
        editor.tabs.view_state.insert((key & 0x7FFF) as u16, idx);
        return;
    }
    let ci = key as usize;
    if ci >= editor.param_count() {
        return;
    }
    editor.write_control_plain(ci, idx as f64, true);
}

/// Lazily register an NSObject subclass with an `editor_ptr` ivar and
/// `click:` + `sliderMoved:` methods. One instance per editor view
/// serves as the target for every SOLO/MUTE button AND every slider.
pub(crate) unsafe fn toggle_target_class() -> Class {
    let existing = unsafe { objc_lookUpClass(c"StaveToggleTarget".as_ptr()) };
    if !existing.is_null() {
        return existing;
    }
    let nsobject = class(c"NSObject");
    let cls = unsafe { objc_allocateClassPair(nsobject, c"StaveToggleTarget".as_ptr(), 0) };
    // ivar: void* editor_ptr. Encoding "^v" = pointer to void.
    let _ = unsafe {
        class_addIvar(
            cls,
            c"editor_ptr".as_ptr(),
            core::mem::size_of::<*const c_void>(),
            3, // log2(8) — alignment of *const c_void on arm64
            c"^v".as_ptr(),
        )
    };
    // click: + sliderMoved: both have signature "v@:@".
    let types = c"v@:@";
    let _ = unsafe {
        class_addMethod(
            cls,
            sel(c"click:"),
            toggle_target_click as *const c_void,
            types.as_ptr() as *const c_char,
        )
    };
    let _ = unsafe {
        class_addMethod(
            cls,
            sel(c"sliderMoved:"),
            toggle_target_slider_moved as *const c_void,
            types.as_ptr() as *const c_char,
        )
    };
    let _ = unsafe {
        class_addMethod(
            cls,
            sel(c"cycleStyle:"),
            toggle_target_cycle_style as *const c_void,
            types.as_ptr() as *const c_char,
        )
    };
    let _ = unsafe {
        class_addMethod(
            cls,
            sel(c"dropdownChanged:"),
            toggle_target_dropdown_changed as *const c_void,
            types.as_ptr() as *const c_char,
        )
    };
    let _ = unsafe {
        class_addMethod(
            cls,
            sel(c"previousPreset:"),
            toggle_target_previous_preset as *const c_void,
            types.as_ptr() as *const c_char,
        )
    };
    let _ = unsafe {
        class_addMethod(
            cls,
            sel(c"nextPreset:"),
            toggle_target_next_preset as *const c_void,
            types.as_ptr() as *const c_char,
        )
    };
    let _ = unsafe {
        class_addMethod(
            cls,
            sel(c"tabPicked:"),
            toggle_target_tab_picked as *const c_void,
            types.as_ptr() as *const c_char,
        )
    };
    unsafe { objc_registerClassPair(cls) };
    cls
}

/// Lazily register a top-left-origin NSView subclass. Returns a Class
/// that alloc/init'd as flipped — y=0 means top.
pub(crate) unsafe fn flipped_view_class() -> Class {
    let existing = unsafe { objc_lookUpClass(c"StaveFlippedView".as_ptr()) };
    if !existing.is_null() {
        return existing;
    }
    let nsview = class(c"NSView");
    let cls = unsafe { objc_allocateClassPair(nsview, c"StaveFlippedView".as_ptr(), 0) };
    let types = c"B@:";
    let _ = unsafe {
        class_addMethod(
            cls,
            sel(c"isFlipped"),
            is_flipped_yes as *const c_void,
            types.as_ptr() as *const c_char,
        )
    };
    unsafe { objc_registerClassPair(cls) };
    cls
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct CGPoint {
    pub(crate) x: f64,
    pub(crate) y: f64,
}
#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct CGSize {
    pub(crate) width: f64,
    pub(crate) height: f64,
}
#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct CGRect {
    pub(crate) origin: CGPoint,
    pub(crate) size: CGSize,
}

/// Custom interactive views accept the activating click — without this,
/// the first click on an unfocused plugin window only focuses it and the
/// gesture is swallowed (the "first drag does nothing" bug).
#[inline]
pub(crate) unsafe extern "C" fn accepts_first_mouse_yes(_this: Id, _cmd: Sel, _event: Id) -> bool {
    true
}

pub(crate) fn sel(s: &core::ffi::CStr) -> Sel {
    unsafe { sel_registerName(s.as_ptr()) }
}

#[inline]
pub(crate) fn class(s: &core::ffi::CStr) -> Class {
    unsafe { objc_getClass(s.as_ptr()) }
}

#[inline]
pub(crate) unsafe fn msg_id(receiver: Id, selector: Sel) -> Id {
    let f: unsafe extern "C" fn(Id, Sel) -> Id =
        unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(receiver, selector) }
}
#[inline]
pub(crate) unsafe fn msg_id_rect(receiver: Id, selector: Sel, rect: CGRect) -> Id {
    let f: unsafe extern "C" fn(Id, Sel, CGRect) -> Id =
        unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(receiver, selector, rect) }
}
#[inline]
pub(crate) unsafe fn msg_void_id(receiver: Id, selector: Sel, arg: Id) {
    let f: unsafe extern "C" fn(Id, Sel, Id) =
        unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(receiver, selector, arg) };
}
#[inline]
pub(crate) unsafe fn msg_void_rect(receiver: Id, selector: Sel, rect: CGRect) {
    let f: unsafe extern "C" fn(Id, Sel, CGRect) =
        unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(receiver, selector, rect) };
}
#[inline]
pub(crate) unsafe fn msg_void_double(receiver: Id, selector: Sel, v: f64) {
    let f: unsafe extern "C" fn(Id, Sel, f64) =
        unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(receiver, selector, v) };
}
#[inline]
pub(crate) unsafe fn msg_void_f32(receiver: Id, selector: Sel, v: f32) {
    let f: unsafe extern "C" fn(Id, Sel, f32) =
        unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(receiver, selector, v) };
}
#[inline]
pub(crate) unsafe fn msg_void_ptr(receiver: Id, selector: Sel, v: *mut c_void) {
    let f: unsafe extern "C" fn(Id, Sel, *mut c_void) =
        unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(receiver, selector, v) };
}
#[inline]
pub(crate) unsafe fn msg_double(receiver: Id, selector: Sel) -> f64 {
    let f: unsafe extern "C" fn(Id, Sel) -> f64 =
        unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(receiver, selector) }
}
#[inline]
pub(crate) unsafe fn msg_void_int(receiver: Id, selector: Sel, v: i64) {
    let f: unsafe extern "C" fn(Id, Sel, i64) =
        unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(receiver, selector, v) };
}
#[inline]
pub(crate) unsafe fn msg_void_point(receiver: Id, selector: Sel, p: CGPoint) {
    let f: unsafe extern "C" fn(Id, Sel, CGPoint) =
        unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(receiver, selector, p) };
}

/// Give a compact shell control the same accent-tint hover used by select
/// menu rows. The tracking area follows the view if layout moves it.
pub(crate) unsafe fn install_shell_control_hover_tracking(view: Id) {
    unsafe {
        let raw = msg_id(class(c"NSTrackingArea"), sel(c"alloc"));
        let initialize: unsafe extern "C" fn(Id, Sel, CGRect, u64, Id, Id) -> Id =
            core::mem::transmute(objc_msgSend as *const ());
        let tracking_area = initialize(
            raw,
            sel(c"initWithRect:options:owner:userInfo:"),
            CGRect {
                origin: CGPoint { x: 0.0, y: 0.0 },
                size: CGSize {
                    width: 0.0,
                    height: 0.0,
                },
            },
            0x281,
            view,
            core::ptr::null_mut(),
        );
        msg_void_id(view, sel(c"addTrackingArea:"), tracking_area);
    }
}

pub(crate) unsafe extern "C" fn shell_control_mouse_entered(this: Id, _command: Sel, _event: Id) {
    unsafe { set_shell_control_hover(this, true) };
}

pub(crate) unsafe extern "C" fn shell_control_mouse_exited(this: Id, _command: Sel, _event: Id) {
    unsafe { set_shell_control_hover(this, false) };
}

pub(crate) unsafe fn register_shell_control_hover_methods(class: Class) {
    unsafe {
        let _ = class_addMethod(
            class,
            sel(c"mouseEntered:"),
            shell_control_mouse_entered as *const c_void,
            c"v@:@".as_ptr(),
        );
        let _ = class_addMethod(
            class,
            sel(c"mouseExited:"),
            shell_control_mouse_exited as *const c_void,
            c"v@:@".as_ptr(),
        );
    }
}

unsafe fn set_shell_control_hover(view: Id, hovered: bool) {
    unsafe {
        let layer = msg_id(view, sel(c"layer"));
        if !layer.is_null() {
            let accent = theme::ACCENT;
            msg_void_ptr(
                layer,
                sel(c"setBackgroundColor:"),
                make_cg_color(
                    accent.0,
                    accent.1,
                    accent.2,
                    if hovered { 0.14 } else { 0.0 },
                ),
            );
        }
    }
}
#[inline]
pub(crate) unsafe fn insert_sublayer_at(layer: Id, sublayer: Id, idx: u32) {
    let f: unsafe extern "C" fn(Id, Sel, Id, u32) =
        unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(layer, sel(c"insertSublayer:atIndex:"), sublayer, idx) };
}

/// Install a modern slate background on a layer: a soft vertical gradient
/// plus a subtle radial vignette for depth. Inserted behind everything else.
pub(crate) unsafe fn install_background(host_layer: Id, bounds: CGRect) {
    let grad_cls = class(c"CAGradientLayer");
    let arr_cls = class(c"NSMutableArray");
    let mk_colors = |a: theme::Rgba, b: theme::Rgba| -> Id {
        let arr = unsafe { msg_id(msg_id(arr_cls, sel(c"alloc")), sel(c"init")) };
        let c0 = unsafe { make_cg_color(a.0, a.1, a.2, a.3) };
        let c1 = unsafe { make_cg_color(b.0, b.1, b.2, b.3) };
        unsafe {
            msg_void_id(arr, sel(c"addObject:"), c0);
            msg_void_id(arr, sel(c"addObject:"), c1);
        }
        arr
    };

    // 1) Vertical slate gradient — lighter at the top, deeper at the bottom.
    let grad = unsafe { msg_id(msg_id(grad_cls, sel(c"alloc")), sel(c"init")) };
    unsafe {
        msg_void_rect(grad, sel(c"setFrame:"), bounds);
        msg_void_id(
            grad,
            sel(c"setColors:"),
            mk_colors(theme::BG_GRAD_TOP, theme::BG_GRAD_BOTTOM),
        );
        msg_void_point(grad, sel(c"setStartPoint:"), CGPoint { x: 0.5, y: 1.0 });
        msg_void_point(grad, sel(c"setEndPoint:"), CGPoint { x: 0.5, y: 0.0 });
        msg_void_int(grad, sel(c"setAutoresizingMask:"), 18); // width|height
        insert_sublayer_at(host_layer, grad, 0);
    }

    // 2) Radial vignette — clear in the centre, soft dark toward the corners.
    let vign = unsafe { msg_id(msg_id(grad_cls, sel(c"alloc")), sel(c"init")) };
    unsafe {
        msg_void_rect(vign, sel(c"setFrame:"), bounds);
        msg_void_id(vign, sel(c"setType:"), ns_string("radial"));
        msg_void_id(
            vign,
            sel(c"setColors:"),
            mk_colors((0.0, 0.0, 0.0, 0.0), theme::VIGNETTE),
        );
        msg_void_point(vign, sel(c"setStartPoint:"), CGPoint { x: 0.5, y: 0.5 });
        msg_void_point(vign, sel(c"setEndPoint:"), CGPoint { x: 1.0, y: 1.0 });
        msg_void_int(vign, sel(c"setAutoresizingMask:"), 18);
        insert_sublayer_at(host_layer, vign, 1);
    }
}
pub(crate) const NS_FONT_WEIGHT_REGULAR: f64 = 0.0;
pub(crate) const NS_FONT_WEIGHT_SEMIBOLD: f64 = 0.3;
pub(crate) const NS_FONT_WEIGHT_HEAVY: f64 = 0.56;

/// System (SF) font at a given size and weight.
#[inline]
pub(crate) unsafe fn ns_font_w(size: f64, weight: f64) -> Id {
    let cls = class(c"NSFont");
    let f: unsafe extern "C" fn(Id, Sel, f64, f64) -> Id =
        unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe { f(cls, sel(c"systemFontOfSize:weight:"), size, weight) }
}

/// SF Mono with tabular digits, at a given size and weight. Perfect for
/// value readouts because the columns of digits don't dance as values
/// change.
#[inline]
pub(crate) unsafe fn ns_font_mono_digit(size: f64, weight: f64) -> Id {
    let cls = class(c"NSFont");
    let f: unsafe extern "C" fn(Id, Sel, f64, f64) -> Id =
        unsafe { core::mem::transmute(objc_msgSend as *const ()) };
    unsafe {
        f(
            cls,
            sel(c"monospacedDigitSystemFontOfSize:weight:"),
            size,
            weight,
        )
    }
}

/// Set a label's text with explicit per-glyph kerning (positive widens,
/// negative tightens). Goes through NSAttributedString. Use sparingly —
/// the call chain is hot per init, not per frame.
#[inline]
/// Set an NSButton's attributed title (font + color + kern, centered).
pub(crate) unsafe fn button_set_attributed_title(
    button: Id,
    text: &str,
    font: Id,
    color: Id,
    kern: f64,
) {
    unsafe {
        let nsstr = ns_string(text);
        let kern_num = {
            let cls = class(c"NSNumber");
            let f: unsafe extern "C" fn(Id, Sel, f64) -> Id =
                core::mem::transmute(objc_msgSend as *const ());
            f(cls, sel(c"numberWithDouble:"), kern)
        };
        let para = {
            let pcls = class(c"NSMutableParagraphStyle");
            let p = msg_id(msg_id(pcls, sel(c"alloc")), sel(c"init"));
            msg_void_int(p, sel(c"setAlignment:"), 1); // center (modern NSTextAlignment: 1 = center, 2 = right)
            p
        };
        let objs: [Id; 4] = [font, color, kern_num, para];
        let keys: [Id; 4] = [
            ns_string("NSFont"),
            ns_string("NSColor"),
            ns_string("NSKern"),
            ns_string("NSParagraphStyle"),
        ];
        let f: unsafe extern "C" fn(Id, Sel, *const Id, *const Id, usize) -> Id =
            core::mem::transmute(objc_msgSend as *const ());
        let dict = f(
            class(c"NSDictionary"),
            sel(c"dictionaryWithObjects:forKeys:count:"),
            objs.as_ptr(),
            keys.as_ptr(),
            4,
        );
        let attr_cls = class(c"NSAttributedString");
        let alloc_ = msg_id(attr_cls, sel(c"alloc"));
        let init_attr: unsafe extern "C" fn(Id, Sel, Id, Id) -> Id =
            core::mem::transmute(objc_msgSend as *const ());
        let attr = init_attr(alloc_, sel(c"initWithString:attributes:"), nsstr, dict);
        msg_void_id(button, sel(c"setAttributedTitle:"), attr);
    }
}

pub(crate) unsafe fn label_set_kerned(label: Id, text: &str, font: Id, color: Id, kern: f64) {
    unsafe {
        // Build NSDictionary attrs = { NSFontAttributeName: font,
        //                              NSForegroundColorAttributeName: color,
        //                              NSKernAttributeName: NSNumber(kern) }.
        let nsstr = ns_string(text);
        let kern_num = {
            let cls = class(c"NSNumber");
            let f: unsafe extern "C" fn(Id, Sel, f64) -> Id =
                core::mem::transmute(objc_msgSend as *const ());
            f(cls, sel(c"numberWithDouble:"), kern)
        };
        let font_name = ns_string("NSFont");
        let color_name = ns_string("NSColor");
        let kern_name = ns_string("NSKern");
        let para_name = ns_string("NSParagraphStyle");
        // Attributed strings ignore the field's `setAlignment:` unless a
        // paragraph style carries it — so read the field's alignment and bake it
        // in. Lets knob labels (alignment = center) center while captions
        // (default = natural/left) stay left, with no call-site changes.
        let alignment: i64 = {
            let g: unsafe extern "C" fn(Id, Sel) -> i64 =
                core::mem::transmute(objc_msgSend as *const ());
            g(label, sel(c"alignment"))
        };
        let para = {
            let pcls = class(c"NSMutableParagraphStyle");
            let p = msg_id(msg_id(pcls, sel(c"alloc")), sel(c"init"));
            msg_void_int(p, sel(c"setAlignment:"), alignment);
            p
        };
        let dict_cls = class(c"NSDictionary");
        // Build via dictionaryWithObjects:forKeys:count:
        let objs: [Id; 4] = [font, color, kern_num, para];
        let keys: [Id; 4] = [font_name, color_name, kern_name, para_name];
        let f: unsafe extern "C" fn(Id, Sel, *const Id, *const Id, usize) -> Id =
            core::mem::transmute(objc_msgSend as *const ());
        let dict = f(
            dict_cls,
            sel(c"dictionaryWithObjects:forKeys:count:"),
            objs.as_ptr(),
            keys.as_ptr(),
            4,
        );
        // NSAttributedString *attr = [[NSAttributedString alloc] initWithString:str attributes:dict];
        let attr_cls = class(c"NSAttributedString");
        let alloc_ = msg_id(attr_cls, sel(c"alloc"));
        let init_attr: unsafe extern "C" fn(Id, Sel, Id, Id) -> Id =
            core::mem::transmute(objc_msgSend as *const ());
        let attr = init_attr(alloc_, sel(c"initWithString:attributes:"), nsstr, dict);
        msg_void_id(label, sel(c"setAttributedStringValue:"), attr);
    }
}
