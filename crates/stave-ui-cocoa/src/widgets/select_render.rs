//! Shared drawing helpers for Stave select fields and cascade panels.

use core::ffi::c_void;

use stave_ui::scene::{pt, Path, Scene};
use stave_ui::style::Color;

use crate::theme;
use crate::*;

pub(crate) fn select_color(value: theme::Rgba) -> Color {
    Color::rgba(value.0, value.1, value.2, value.3)
}

pub(crate) fn rounded_select_path(x: f64, y: f64, width: f64, height: f64, radius: f64) -> Path {
    Path::new()
        .move_to(pt(x + radius, y))
        .line_to(pt(x + width - radius, y))
        .quad_to(pt(x + width, y), pt(x + width, y + radius))
        .line_to(pt(x + width, y + height - radius))
        .quad_to(
            pt(x + width, y + height),
            pt(x + width - radius, y + height),
        )
        .line_to(pt(x + radius, y + height))
        .quad_to(pt(x, y + height), pt(x, y + height - radius))
        .line_to(pt(x, y + radius))
        .quad_to(pt(x, y), pt(x + radius, y))
        .close()
}

pub(crate) unsafe fn draw_select_scene(scene: &Scene) {
    unsafe {
        let current_context = msg_id(class(c"NSGraphicsContext"), sel(c"currentContext"));
        if current_context.is_null() {
            return;
        }
        let context: unsafe extern "C" fn(Id, Sel) -> *mut c_void =
            core::mem::transmute(objc_msgSend as *const ());
        let context = context(current_context, sel(c"CGContext"));
        if !context.is_null() {
            super::scene_render::render(context, scene);
        }
    }
}
