//! Factory-preset picker widget (DebugBus preset declarations).

use core::ffi::c_void;

extern crate alloc;

use super::cg::{
    CGPathAddLineToPoint, CGPathCloseSubpath, CGPathCreateMutable, CGPathMoveToPoint, CGPathRelease,
};
use crate::theme;
use crate::view::PluginEditorView;
use crate::*;

const ACTION_BUTTON_WIDTH: f64 = 32.0;
const SAVE_BUTTON_WIDTH: f64 = 36.0;
const LARGE_GAP: f64 = 8.0;

unsafe fn preset_segment_button_class() -> Class {
    unsafe {
        let existing = objc_lookUpClass(c"StavePresetSegmentButton".as_ptr());
        if !existing.is_null() {
            return existing;
        }
        let button_class =
            objc_allocateClassPair(class(c"NSButton"), c"StavePresetSegmentButton".as_ptr(), 0);
        register_shell_control_hover_methods(button_class);
        objc_registerClassPair(button_class);
        button_class
    }
}

unsafe extern "C" fn save_button_mouse_entered(this: Id, _command: Sel, _event: Id) {
    unsafe { set_save_button_hover(this, true) };
}

unsafe extern "C" fn save_button_mouse_exited(this: Id, _command: Sel, _event: Id) {
    unsafe { set_save_button_hover(this, false) };
}

unsafe fn set_save_button_hover(button: Id, hovered: bool) {
    unsafe {
        let layer = msg_id(button, sel(c"layer"));
        if layer.is_null() {
            return;
        }
        let color = if hovered { theme::BG_2 } else { theme::BG_1 };
        msg_void_ptr(
            layer,
            sel(c"setBackgroundColor:"),
            make_cg_color(color.0, color.1, color.2, color.3),
        );
    }
}

unsafe fn preset_save_button_class() -> Class {
    unsafe {
        let existing = objc_lookUpClass(c"StavePresetSaveButton".as_ptr());
        if !existing.is_null() {
            return existing;
        }
        let button_class =
            objc_allocateClassPair(class(c"NSButton"), c"StavePresetSaveButton".as_ptr(), 0);
        let _ = class_addMethod(
            button_class,
            sel(c"mouseEntered:"),
            save_button_mouse_entered as *const c_void,
            c"v@:@".as_ptr(),
        );
        let _ = class_addMethod(
            button_class,
            sel(c"mouseExited:"),
            save_button_mouse_exited as *const c_void,
            c"v@:@".as_ptr(),
        );
        objc_registerClassPair(button_class);
        button_class
    }
}

unsafe fn make_icon_stroke_layer(
    frame: CGRect,
    points: &[CGPoint],
    closed: bool,
    line_color: *mut c_void,
    line_width: f64,
) -> Id {
    unsafe {
        let shape = msg_id(class(c"CAShapeLayer"), sel(c"layer"));
        msg_void_rect(shape, sel(c"setFrame:"), frame);
        let path = CGPathCreateMutable();
        if let Some(first) = points.first() {
            CGPathMoveToPoint(path, core::ptr::null(), first.x, first.y);
            for point in &points[1..] {
                CGPathAddLineToPoint(path, core::ptr::null(), point.x, point.y);
            }
            if closed {
                CGPathCloseSubpath(path);
            }
        }
        msg_void_ptr(shape, sel(c"setPath:"), path);
        CGPathRelease(path);
        msg_void_ptr(
            shape,
            sel(c"setFillColor:"),
            make_cg_color(0.0, 0.0, 0.0, 0.0),
        );
        msg_void_ptr(shape, sel(c"setStrokeColor:"), line_color);
        msg_void_double(shape, sel(c"setLineWidth:"), line_width);
        msg_void_id(shape, sel(c"setLineCap:"), ns_string("round"));
        msg_void_id(shape, sel(c"setLineJoin:"), ns_string("round"));
        shape
    }
}

unsafe fn add_save_icon(button: Id, button_width: f64, button_height: f64) {
    unsafe {
        let button_layer = msg_id(button, sel(c"layer"));
        if button_layer.is_null() {
            return;
        }
        let line = theme::SHELL_CONTROL_INK;
        let line_color = make_cg_color(line.0, line.1, line.2, 1.0);
        let icon_size = 16.0;
        let icon_frame = CGRect {
            origin: CGPoint {
                x: (button_width - icon_size) * 0.5,
                y: (button_height - icon_size) * 0.5,
            },
            size: CGSize {
                width: icon_size,
                height: icon_size,
            },
        };
        let outline = make_icon_stroke_layer(
            icon_frame,
            &[
                CGPoint { x: 2.0, y: 2.5 },
                CGPoint { x: 2.0, y: 13.5 },
                CGPoint { x: 2.5, y: 14.0 },
                CGPoint { x: 13.5, y: 14.0 },
                CGPoint { x: 14.0, y: 13.5 },
                CGPoint { x: 14.0, y: 6.0 },
                CGPoint { x: 10.0, y: 2.0 },
                CGPoint { x: 2.5, y: 2.0 },
            ],
            true,
            line_color,
            1.05,
        );
        msg_void_id(button_layer, sel(c"addSublayer:"), outline);

        let top_shutter = make_icon_stroke_layer(
            icon_frame,
            &[
                CGPoint { x: 6.0, y: 2.0 },
                CGPoint { x: 6.0, y: 4.5 },
                CGPoint { x: 10.0, y: 4.5 },
                CGPoint { x: 10.0, y: 2.0 },
            ],
            false,
            line_color,
            0.9,
        );
        msg_void_id(button_layer, sel(c"addSublayer:"), top_shutter);

        let lower_label = make_icon_stroke_layer(
            icon_frame,
            &[
                CGPoint { x: 4.0, y: 14.0 },
                CGPoint { x: 4.0, y: 8.5 },
                CGPoint { x: 4.5, y: 8.0 },
                CGPoint { x: 11.5, y: 8.0 },
                CGPoint { x: 12.0, y: 8.5 },
                CGPoint { x: 12.0, y: 14.0 },
            ],
            false,
            line_color,
            0.9,
        );
        msg_void_id(button_layer, sel(c"addSublayer:"), lower_label);

        for y in [10.2, 12.2] {
            let data_line = msg_id(class(c"CALayer"), sel(c"layer"));
            msg_void_rect(
                data_line,
                sel(c"setFrame:"),
                CGRect {
                    origin: CGPoint {
                        x: icon_frame.origin.x + 5.4,
                        y: icon_frame.origin.y + y,
                    },
                    size: CGSize {
                        width: 5.2,
                        height: 0.7,
                    },
                },
            );
            msg_void_double(data_line, sel(c"setCornerRadius:"), 0.35);
            msg_void_ptr(data_line, sel(c"setBackgroundColor:"), line_color);
            msg_void_id(button_layer, sel(c"addSublayer:"), data_line);
        }
    }
}

unsafe fn add_preset_action_button(
    parent: Id,
    target: Id,
    rect: CGRect,
    title: &str,
    action: Option<&core::ffi::CStr>,
    primary: bool,
    segmented: bool,
) {
    unsafe {
        let button_type = if segmented {
            preset_segment_button_class() as Id
        } else if primary {
            preset_save_button_class() as Id
        } else {
            class(c"NSButton")
        };
        let raw = msg_id(button_type, sel(c"alloc"));
        let button = msg_id_rect(raw, sel(c"initWithFrame:"), rect);
        msg_void_int(button, sel(c"setButtonType:"), 0);
        msg_void_int(button, sel(c"setBordered:"), 0);
        msg_void_int(button, sel(c"setWantsLayer:"), 1);
        let layer = msg_id(button, sel(c"layer"));
        if !layer.is_null() && !segmented {
            let background = theme::BG_1;
            let border = if primary {
                theme::SHELL_CONTROL_LINE
            } else {
                theme::LINE
            };
            msg_void_ptr(
                layer,
                sel(c"setBackgroundColor:"),
                make_cg_color(background.0, background.1, background.2, 1.0),
            );
            msg_void_double(layer, sel(c"setCornerRadius:"), 5.0);
            msg_void_double(layer, sel(c"setBorderWidth:"), 1.0);
            msg_void_ptr(
                layer,
                sel(c"setBorderColor:"),
                make_cg_color(
                    border.0,
                    border.1,
                    border.2,
                    if primary { 0.9 } else { 0.7 },
                ),
            );
        }
        let (font, color, kern) = if primary {
            (
                ns_font_role((9.0, theme::W_SEMIBOLD)),
                ns_color_token(theme::ACCENT),
                1.2,
            )
        } else {
            (
                ns_font_role((15.0, theme::W_MEDIUM)),
                ns_color_token(theme::SHELL_CONTROL_INK),
                0.0,
            )
        };
        button_set_attributed_title(button, title, font, color, kern);
        if segmented {
            install_shell_control_hover_tracking(button);
        } else if primary {
            install_shell_control_hover_tracking(button);
            msg_void_id(button, sel(c"setToolTip:"), ns_string("Save preset"));
            msg_void_id(
                button,
                sel(c"setAccessibilityLabel:"),
                ns_string("Save preset"),
            );
            add_save_icon(button, rect.size.width, rect.size.height);
        }
        if let Some(action) = action {
            if !target.is_null() {
                msg_void_id(button, sel(c"setTarget:"), target);
                let set_action: unsafe extern "C" fn(Id, Sel, Sel) =
                    core::mem::transmute(objc_msgSend as *const ());
                set_action(button, sel(c"setAction:"), sel(action));
            }
        }
        msg_void_id(parent, sel(c"addSubview:"), button);
    }
}

unsafe fn add_segmented_surface(parent: Id, rect: CGRect) -> Id {
    unsafe {
        let raw = msg_id(class(c"NSView"), sel(c"alloc"));
        let surface = msg_id_rect(raw, sel(c"initWithFrame:"), rect);
        msg_void_int(surface, sel(c"setWantsLayer:"), 1);
        let layer = msg_id(surface, sel(c"layer"));
        if !layer.is_null() {
            msg_void_ptr(
                layer,
                sel(c"setBackgroundColor:"),
                make_cg_color(theme::BG_1.0, theme::BG_1.1, theme::BG_1.2, 1.0),
            );
            msg_void_double(layer, sel(c"setCornerRadius:"), 5.0);
            msg_void_int(layer, sel(c"setMasksToBounds:"), 1);
            msg_void_double(layer, sel(c"setBorderWidth:"), 1.0);
            msg_void_ptr(
                layer,
                sel(c"setBorderColor:"),
                make_cg_color(
                    theme::SHELL_CONTROL_LINE.0,
                    theme::SHELL_CONTROL_LINE.1,
                    theme::SHELL_CONTROL_LINE.2,
                    0.9,
                ),
            );
        }
        msg_void_id(parent, sel(c"addSubview:"), surface);
        surface
    }
}

unsafe fn add_segment_divider(parent: Id, x: f64, height: f64) {
    unsafe {
        let raw = msg_id(class(c"NSView"), sel(c"alloc"));
        let divider = msg_id_rect(
            raw,
            sel(c"initWithFrame:"),
            CGRect {
                origin: CGPoint { x, y: 1.0 },
                size: CGSize {
                    width: 1.0,
                    height: height - 2.0,
                },
            },
        );
        msg_void_int(divider, sel(c"setWantsLayer:"), 1);
        let layer = msg_id(divider, sel(c"layer"));
        if !layer.is_null() {
            msg_void_ptr(
                layer,
                sel(c"setBackgroundColor:"),
                make_cg_color(
                    theme::SHELL_CONTROL_LINE.0,
                    theme::SHELL_CONTROL_LINE.1,
                    theme::SHELL_CONTROL_LINE.2,
                    0.9,
                ),
            );
        }
        msg_void_id(parent, sel(c"addSubview:"), divider);
    }
}

impl PluginEditorView {
    /// Full preset navigator: previous, dropdown, next, and save.
    /// Rect-driven and mounted from markup via Widget::PresetBar.
    pub(crate) unsafe fn add_preset_pill(&mut self, parent_ns_view: Id, r: CGRect) {
        unsafe {
            let control_height = r.size.height.min(32.0);
            let control_y = r.origin.y + (r.size.height - control_height) * 0.5;
            let navigation_width = r.size.width - SAVE_BUTTON_WIDTH - LARGE_GAP;
            let navigation = add_segmented_surface(
                parent_ns_view,
                CGRect {
                    origin: CGPoint {
                        x: r.origin.x,
                        y: control_y,
                    },
                    size: CGSize {
                        width: navigation_width,
                        height: control_height,
                    },
                },
            );
            let popup_width = navigation_width - ACTION_BUTTON_WIDTH * 2.0;
            let field_rect = CGRect {
                origin: CGPoint {
                    x: ACTION_BUTTON_WIDTH,
                    y: 0.0,
                },
                size: CGSize {
                    width: popup_width,
                    height: control_height,
                },
            };
            crate::widgets::dropdown::add_preset_dropdown_field(self, navigation, field_rect);

            add_preset_action_button(
                navigation,
                self.toggle_target,
                CGRect {
                    origin: CGPoint { x: 0.0, y: 0.0 },
                    size: CGSize {
                        width: ACTION_BUTTON_WIDTH,
                        height: control_height,
                    },
                },
                "\u{2039}",
                Some(c"previousPreset:"),
                false,
                true,
            );
            add_preset_action_button(
                navigation,
                self.toggle_target,
                CGRect {
                    origin: CGPoint {
                        x: navigation_width - ACTION_BUTTON_WIDTH,
                        y: 0.0,
                    },
                    size: CGSize {
                        width: ACTION_BUTTON_WIDTH,
                        height: control_height,
                    },
                },
                "\u{203A}",
                Some(c"nextPreset:"),
                false,
                true,
            );
            add_segment_divider(navigation, ACTION_BUTTON_WIDTH, control_height);
            add_segment_divider(
                navigation,
                navigation_width - ACTION_BUTTON_WIDTH - 1.0,
                control_height,
            );
            add_preset_action_button(
                parent_ns_view,
                self.toggle_target,
                CGRect {
                    origin: CGPoint {
                        x: r.origin.x + navigation_width + LARGE_GAP,
                        y: control_y,
                    },
                    size: CGSize {
                        width: SAVE_BUTTON_WIDTH,
                        height: control_height,
                    },
                },
                "",
                None,
                true,
                false,
            );
        }
    }
}
