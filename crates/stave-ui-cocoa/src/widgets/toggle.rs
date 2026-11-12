//! Solo/mute toggle pill builder.

use core::ffi::c_void;
use core::ptr;

extern crate alloc;

use crate::theme;
use crate::view::PluginEditorView;
use crate::*;

impl PluginEditorView {
    /// 0/1 button presented as a square push button (SOLO / MUTE).
    /// `accent_rgb` is the band's identity colour, tinting the pill.
    /// The pill is 26 px tall at `rect`'s origin and width.
    pub(crate) unsafe fn add_toggle(
        &mut self,
        parent_card: Id,
        ci: usize,
        is_solo: bool,
        rect: CGRect,
        accent_rgb: (f64, f64, f64),
    ) {
        let raw = unsafe { msg_id(class(c"NSButton"), sel(c"alloc")) };
        let rect = CGRect {
            origin: rect.origin,
            size: CGSize {
                width: rect.size.width,
                height: 26.0,
            },
        };
        let btn = unsafe { msg_id_rect(raw, sel(c"initWithFrame:"), rect) };
        unsafe {
            msg_void_int(btn, sel(c"setButtonType:"), 2); // NSPushOnPushOff
                                                          // Borderless layer-backed pill — matches the style and
                                                          // bypass buttons rather than the stock macOS bezel.
            msg_void_int(btn, sel(c"setBordered:"), 0);
            msg_void_int(btn, sel(c"setWantsLayer:"), 1);
            // Positional (Card contract: toggles[0] = SOLO) — 7.4.
            // Per-role tints for bg and border.
            let (br, bg_, bb) = if is_solo {
                accent_rgb
            } else {
                (theme::WARN.0, theme::WARN.1, theme::WARN.2)
            };
            let lyr = msg_id(btn, sel(c"layer"));
            if !lyr.is_null() {
                let bg = make_cg_color(br * 0.18, bg_ * 0.18, bb * 0.18, 0.85);
                let f_bg: unsafe extern "C" fn(Id, Sel, *mut c_void) =
                    core::mem::transmute(objc_msgSend as *const ());
                f_bg(lyr, sel(c"setBackgroundColor:"), bg);
                let f2: unsafe extern "C" fn(Id, Sel, f64) =
                    core::mem::transmute(objc_msgSend as *const ());
                f2(lyr, sel(c"setCornerRadius:"), 5.0);
                let fb: unsafe extern "C" fn(Id, Sel, f64) =
                    core::mem::transmute(objc_msgSend as *const ());
                fb(lyr, sel(c"setBorderWidth:"), 1.0);
                let border = make_cg_color(br * 0.55, bg_ * 0.55, bb * 0.55, 0.9);
                let f_b: unsafe extern "C" fn(Id, Sel, *mut c_void) =
                    core::mem::transmute(objc_msgSend as *const ());
                f_b(lyr, sel(c"setBorderColor:"), border);
            }
            let label = if is_solo { "SOLO" } else { "MUTE" };
            // Heavy small caps title, kerned, with SOLO tinted in the
            // band accent and MUTE in a warm red so they read at a glance.
            let title_color = if is_solo {
                ns_color(accent_rgb.0, accent_rgb.1, accent_rgb.2, 1.0)
            } else {
                ns_color_token(theme::WARN)
            };
            label_set_kerned(
                btn,
                label,
                ns_font_role(theme::role_group()),
                title_color,
                1.5,
            );
            // Setting an attributed title on NSButton requires the
            // attributed-title-specific setter; the kerned helper above
            // already writes setAttributedStringValue: which buttons treat
            // as their bezel title. Belt-and-braces: also push the styled
            // title via setAttributedTitle: so the bezel actually picks it
            // up (the AttributedStringValue path is for label controls).
            // The label_set_kerned call constructed an NSAttributedString;
            // construct it once more here and route it through
            // setAttributedTitle:.
            let nsstr = ns_string(label);
            let font = ns_font_role(theme::role_group());
            let font_name = ns_string("NSFont");
            let color_name = ns_string("NSColor");
            let kern_name = ns_string("NSKern");
            let kern_num = {
                let cls = class(c"NSNumber");
                let f: unsafe extern "C" fn(Id, Sel, f64) -> Id =
                    core::mem::transmute(objc_msgSend as *const ());
                f(cls, sel(c"numberWithDouble:"), 1.5)
            };
            let dict_cls = class(c"NSDictionary");
            let objs: [Id; 3] = [font, title_color, kern_num];
            let keys: [Id; 3] = [font_name, color_name, kern_name];
            let f_d: unsafe extern "C" fn(Id, Sel, *const Id, *const Id, usize) -> Id =
                core::mem::transmute(objc_msgSend as *const ());
            let dict = f_d(
                dict_cls,
                sel(c"dictionaryWithObjects:forKeys:count:"),
                objs.as_ptr(),
                keys.as_ptr(),
                3,
            );
            let attr_cls = class(c"NSAttributedString");
            let attr_alloc = msg_id(attr_cls, sel(c"alloc"));
            let init_attr: unsafe extern "C" fn(Id, Sel, Id, Id) -> Id =
                core::mem::transmute(objc_msgSend as *const ());
            let attr = init_attr(attr_alloc, sel(c"initWithString:attributes:"), nsstr, dict);
            msg_void_id(btn, sel(c"setAttributedTitle:"), attr);
            let btn_font = ns_font_role(theme::role_group());
            msg_void_id(btn, sel(c"setFont:"), btn_font);
            // Current state: bus value > 0.5 = on.
            let cur = self.plain(ci);
            msg_void_int(btn, sel(c"setState:"), if cur > 0.5 { 1 } else { 0 });
            // Wire click → toggle_target.click:. The button's tag
            // carries the control index so a single shared target can
            // service every band's SOLO + MUTE.
            msg_void_int(btn, sel(c"setTag:"), ci as i64);
            if !self.toggle_target.is_null() {
                msg_void_id(btn, sel(c"setTarget:"), self.toggle_target);
                let f_sel: unsafe extern "C" fn(Id, Sel, Sel) =
                    core::mem::transmute(objc_msgSend as *const ());
                f_sel(btn, sel(c"setAction:"), sel(c"click:"));
            }
            msg_void_id(parent_card, sel(c"addSubview:"), btn);
        }
        // SOLO/MUTE buttons are bus-authoritative: the tick reads the
        // bus and pushes state to the button, never the reverse. Click
        // handlers (writing user intent back to the bus) will come via
        // NSButton target/action.
        self.toggle_buttons.push((ci, btn));
    }

    /// # Safety
    /// Main thread, after `attach`.
    pub unsafe fn detach(&mut self) {
        // Invalidate timer.
        if !self.timer.is_null() {
            unsafe { msg_id(self.timer, sel(c"invalidate")) };
            self.timer = ptr::null_mut();
        }
        // Slider/label memory is owned by the parent view via addSubview; the
        // parent's removal handles it.
        self.sliders.clear();
    }
}
