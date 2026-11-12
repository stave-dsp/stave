//! The round power/bypass button widget.

extern crate alloc;

use crate::theme;
use crate::view::PluginEditorView;
use crate::*;

impl PluginEditorView {
    /// The round power/bypass button. Rect-driven (markup Widget::
    /// Power or the framed legacy path); registers the tick-styling
    /// caches (bypass_ci/button/last_on).
    pub(crate) unsafe fn add_power_button(&mut self, parent_ns_view: Id, r: CGRect, bp_ci: usize) {
        unsafe {
            {
                self.bypass_ci = Some(bp_ci);
                let d = r.size.width.min(r.size.height);
                let button_class = class(c"NSButton");
                let raw = msg_id(button_class, sel(c"alloc"));
                let rect = CGRect {
                    origin: CGPoint {
                        x: r.origin.x,
                        y: r.origin.y,
                    },
                    size: CGSize {
                        width: d,
                        height: d,
                    },
                };
                let bp = msg_id_rect(raw, sel(c"initWithFrame:"), rect);
                msg_void_int(bp, sel(c"setButtonType:"), 2);
                msg_void_int(bp, sel(c"setBordered:"), 0);
                msg_void_int(bp, sel(c"setWantsLayer:"), 1);
                let lyr = msg_id(bp, sel(c"layer"));
                if !lyr.is_null() {
                    msg_void_ptr(
                        lyr,
                        sel(c"setBackgroundColor:"),
                        make_cg_color(theme::BG_2.0, theme::BG_2.1, theme::BG_2.2, 1.0),
                    );
                    msg_void_double(lyr, sel(c"setCornerRadius:"), d * 0.5);
                    msg_void_double(lyr, sel(c"setBorderWidth:"), 1.0);
                    msg_void_ptr(
                        lyr,
                        sel(c"setBorderColor:"),
                        make_cg_color(theme::LINE.0, theme::LINE.1, theme::LINE.2, 0.8),
                    );
                }
                button_set_attributed_title(
                    bp,
                    "\u{23FB}",
                    ns_font_role((11.0, theme::W_SEMIBOLD)),
                    ns_color_token(theme::INK_LO),
                    0.0,
                );
                msg_void_int(bp, sel(c"setTag:"), bp_ci as i64);
                if !self.toggle_target.is_null() {
                    msg_void_id(bp, sel(c"setTarget:"), self.toggle_target);
                    let f_sel: unsafe extern "C" fn(Id, Sel, Sel) =
                        core::mem::transmute(objc_msgSend as *const ());
                    f_sel(bp, sel(c"setAction:"), sel(c"click:"));
                }
                msg_void_id(parent_ns_view, sel(c"addSubview:"), bp);
                self.bypass_button = bp;
                let on = self.plain(bp_ci) > 0.5;
                self.bypass_last_on = on;
                self.style_power_button(on);
            }
        }
    }
}
