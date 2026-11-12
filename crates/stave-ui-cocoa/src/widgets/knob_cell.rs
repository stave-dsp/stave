//! Knob + dropdown grid-cell builders.

extern crate alloc;

use crate::theme;
use crate::view::PluginEditorView;
use crate::*;

/// Grid-cell knob diameter (the design-system default).
pub(crate) const GRID_KNOB: f64 = 52.0;

/// A knob laid out in a grid cell: label above, value below, the dial
/// `diameter` wide and centred in `rect`.
pub(crate) struct KnobCell {
    pub rect: CGRect,
    pub diameter: f64,
    pub band: Option<usize>,
    pub parts: stave_core::editor::KnobParts,
    pub accent: stave_core::editor::KnobAccent,
}

impl PluginEditorView {
    /// Like add_knob but with a separately-specified `cell_w` for the
    /// caption / value labels. Useful when the cell is wider than the
    /// knob (typical in grid layouts).
    /// A stepped control in a knob-grid cell: nameplate label on top
    /// (same tier as the knobs'), NSPopUpButton centred where the knob
    /// face would sit. Selection writes the step INDEX to the bus via
    /// the shared dropdownChanged: action.
    pub(crate) unsafe fn add_dropdown_in_cell(
        &mut self,
        parent: Id,
        ci: usize,
        cell_x: f64,
        y: f64,
        cell_w: f64,
    ) {
        unsafe {
            let label_cls = class(c"NSTextField");
            // Nameplate — identical styling to the knob cells so the grid
            // reads as one family.
            let lbl_raw = msg_id(label_cls, sel(c"alloc"));
            let lbl_rect = CGRect {
                origin: CGPoint { x: cell_x - 9.0, y },
                size: CGSize {
                    width: cell_w + 18.0,
                    height: 12.0,
                },
            };
            let lbl = msg_id_rect(lbl_raw, sel(c"initWithFrame:"), lbl_rect);
            msg_void_int(lbl, sel(c"setBezeled:"), 0);
            msg_void_int(lbl, sel(c"setDrawsBackground:"), 0);
            msg_void_int(lbl, sel(c"setEditable:"), 0);
            msg_void_int(lbl, sel(c"setSelectable:"), 0);
            msg_void_int(lbl, sel(c"setAlignment:"), 1); // center
            label_set_kerned(
                lbl,
                &self.param(ci).meta().name.to_uppercase(),
                ns_font_role(theme::role_tick()),
                ns_color_token(theme::INK_LO),
                0.6,
            );
            msg_void_id(parent, sel(c"addSubview:"), lbl);

            let btn_h = crate::widgets::dropdown::FIELD_H;
            let btn_rect = CGRect {
                origin: CGPoint {
                    x: cell_x - 4.0,
                    y: y + 14.0 + (GRID_KNOB - btn_h) * 0.5,
                },
                size: CGSize {
                    width: cell_w + 8.0,
                    height: btn_h,
                },
            };
            crate::widgets::dropdown::add_dropdown_field(self, parent, ci, btn_rect);
        }
    }

    pub(crate) unsafe fn add_knob_in_cell(&mut self, parent: Id, ci: usize, cell: KnobCell) {
        let KnobCell {
            rect,
            diameter: size,
            band: band_idx,
            parts,
            accent,
        } = cell;
        let (cell_x, y, cell_w, cell_h) = (
            rect.origin.x,
            rect.origin.y,
            rect.size.width,
            rect.size.height,
        );
        unsafe {
            let label_cls = class(c"NSTextField");
            let knob_x = cell_x + (cell_w - size) * 0.5;
            let x = cell_x;
            let size_for_knob = size;
            // Label above — uppercase tracked, INK_LO. The field overflows the
            // cell by 9 px each side (centred) so long names like "THRESHOLD"
            // don't clip in narrow grid columns; neighbours stay clear because
            // the text itself is centred and shorter than the padded box.
            if parts.label {
                let lbl_raw = msg_id(label_cls, sel(c"alloc"));
                let lbl_rect = CGRect {
                    origin: CGPoint { x: x - 9.0, y },
                    size: CGSize {
                        width: cell_w + 18.0,
                        height: 12.0,
                    },
                };
                let lbl = msg_id_rect(lbl_raw, sel(c"initWithFrame:"), lbl_rect);
                msg_void_int(lbl, sel(c"setBezeled:"), 0);
                msg_void_int(lbl, sel(c"setDrawsBackground:"), 0);
                msg_void_int(lbl, sel(c"setEditable:"), 0);
                msg_void_int(lbl, sel(c"setSelectable:"), 0);
                msg_void_int(lbl, sel(c"setAlignment:"), 1);
                let raw_name = self.param(ci).meta().name;
                let cleaned = raw_name
                    .strip_prefix("lo ")
                    .or_else(|| raw_name.strip_prefix("mid "))
                    .or_else(|| raw_name.strip_prefix("hi "))
                    .unwrap_or(raw_name);
                label_set_kerned(
                    lbl,
                    &cleaned.to_uppercase(),
                    ns_font_role(theme::role_tick()),
                    ns_color_token(theme::INK_LO),
                    0.6,
                );
                msg_void_id(parent, sel(c"addSubview:"), lbl);
            }

            // Knob NSView — centered in the cell.
            let kcls = knob_view_class();
            let kraw = msg_id(kcls, sel(c"alloc"));
            let knob_y = if parts.label {
                y + 14.0
            } else if parts.value {
                y
            } else {
                y + (cell_h - size_for_knob) * 0.5
            };
            let knob_rect = CGRect {
                origin: CGPoint {
                    x: knob_x,
                    y: knob_y,
                },
                size: CGSize {
                    width: size_for_knob,
                    height: size_for_knob,
                },
            };
            let knob = msg_id_rect(kraw, sel(c"initWithFrame:"), knob_rect);
            msg_void_int(knob, sel(c"setWantsLayer:"), 1);
            // Stash editor_ptr + band+1 (0 sentinel = unset).
            let ed_ivar = class_getInstanceVariable(kcls, c"editor_ptr".as_ptr());
            if !ed_ivar.is_null() {
                object_setIvar(knob, ed_ivar, self.self_ptr as Id);
            }
            let bi_ivar = class_getInstanceVariable(kcls, c"band_idx".as_ptr());
            if !bi_ivar.is_null() {
                let stored = band_idx.map(|i| (i + 1) as i64).unwrap_or(0);
                object_setIvar(knob, bi_ivar, stored as Id);
            }
            let accent_ivar = class_getInstanceVariable(kcls, c"accent_mode".as_ptr());
            if !accent_ivar.is_null() {
                let stored = match accent {
                    stave_core::editor::KnobAccent::ThemeAccent => 0_i64,
                    stave_core::editor::KnobAccent::Neutral => 1_i64,
                };
                object_setIvar(knob, accent_ivar, stored as Id);
            }
            msg_void_int(knob, sel(c"setTag:"), ci as i64);
            msg_void_id(parent, sel(c"addSubview:"), knob);

            // Value readout below — slightly larger, INK_HI, SF Mono
            // tabular. The VALUE is the data; it should out-weigh the
            // name above.
            let val = if parts.value {
                let val_raw = msg_id(label_cls, sel(c"alloc"));
                let val_rect = CGRect {
                    origin: CGPoint {
                        x: x - 9.0,
                        y: knob_y + size_for_knob + 2.0,
                    },
                    size: CGSize {
                        width: cell_w + 18.0,
                        height: 16.0,
                    },
                };
                let val = msg_id_rect(val_raw, sel(c"initWithFrame:"), val_rect);
                msg_void_int(val, sel(c"setBezeled:"), 0);
                msg_void_int(val, sel(c"setDrawsBackground:"), 0);
                msg_void_int(val, sel(c"setEditable:"), 0);
                msg_void_int(val, sel(c"setSelectable:"), 0);
                msg_void_int(val, sel(c"setAlignment:"), 1);
                msg_void_id(
                    val,
                    sel(c"setFont:"),
                    ns_font_role_mono((9.5, theme::W_MEDIUM)),
                );
                msg_void_id(val, sel(c"setTextColor:"), ns_color_token(theme::INK_HI));
                msg_void_id(
                    val,
                    sel(c"setStringValue:"),
                    ns_string(&format_value(self.param(ci))),
                );
                msg_void_id(parent, sel(c"addSubview:"), val);
                val
            } else {
                core::ptr::null_mut()
            };

            let bits = self.plain(ci).to_bits();
            self.knobs.push((ci, knob, val, bits));
        }
    }
}
