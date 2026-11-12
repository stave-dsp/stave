//! Stave-owned select controls and overlay menus.
//!
//! This is the shared replacement for native host dropdowns. A field
//! provides one of two data sources today:
//!
//! * a stepped parameter, represented as a flat list of choices;
//! * the editor's preset catalog, represented as a hierarchy of groups
//!   and selectable leaves.
//!
//! The closed field, modal scrim, menu panel, hover, scrolling, and
//! selection behavior are all owned here. Preset paths use `/` as their
//! hierarchy separator (`FACTORY / CLEAN / WARM`). The field displays
//! only the leaf (`WARM`); the open menu retains the full hierarchy.

use core::ffi::c_void;

extern crate alloc;
use alloc::vec::Vec;

use stave_ui::scene::{pt, Path, Scene, Stroke};
use stave_ui::style::Color;

use super::select_model::{
    control_step_title, field_title, nodes_for_source, selected_control_step, selected_value,
    OpenSelectMenu, SelectField, SelectNode, SelectPanel, SelectSource,
};
use super::select_render::{draw_select_scene, rounded_select_path, select_color};
use crate::objc::*;
use crate::theme;
use crate::view::PluginEditorView;
use crate::*;

pub(crate) const FIELD_H: f64 = 26.0;
const MENU_ROW_HEIGHT: f64 = 28.0;
const MENU_PADDING: f64 = 6.0;
const MAX_VISIBLE_ROWS: usize = 11;
const ROOT_PRESET_MENU_WIDTH: f64 = 220.0;
const CHILD_MENU_WIDTH: f64 = 208.0;
const MENU_GAP: f64 = 4.0;

// ------------------------------------------------------------ mounting

unsafe fn add_select_field(
    view: &mut PluginEditorView,
    parent: Id,
    source: SelectSource,
    rect: CGRect,
    centered: bool,
) {
    unsafe {
        let class = field_class();
        let raw = msg_id(class as Id, sel(c"alloc"));
        let field = msg_id_rect(raw, sel(c"initWithFrame:"), rect);
        set_editor_ivar(class, field, view.self_ptr);
        let field_index = view.dropdowns.fields.len();
        msg_void_int(field, sel(c"setTag:"), field_index as i64);
        msg_void_int(field, sel(c"setWantsLayer:"), 1);
        if source == SelectSource::Presets {
            install_shell_control_hover_tracking(field);
        }
        msg_void_id(parent, sel(c"addSubview:"), field);

        let preset_field = source == SelectSource::Presets;
        let label = make_label(CGRect {
            origin: CGPoint {
                x: if preset_field {
                    0.0
                } else if centered {
                    4.0
                } else {
                    12.0
                },
                y: (rect.size.height - 16.0) * 0.5 - if preset_field { 1.5 } else { 0.0 },
            },
            size: CGSize {
                width: if preset_field {
                    rect.size.width
                } else {
                    rect.size.width - if centered { 26.0 } else { 42.0 }
                },
                height: 16.0,
            },
        });
        msg_void_int(
            label,
            sel(c"setAlignment:"),
            if centered || preset_field { 1 } else { 0 },
        );
        label_set_kerned(
            label,
            &field_title(view, source),
            ns_font_role(theme::role_group()),
            ns_color_token(if preset_field {
                theme::SHELL_CONTROL_INK
            } else {
                theme::INK_MD
            }),
            0.6,
        );
        msg_void_id(field, sel(c"addSubview:"), label);

        let last_control_value = match source {
            SelectSource::Control(control_index) => view.plain(control_index).to_bits(),
            SelectSource::Presets => 0,
        };
        view.dropdowns.fields.push(SelectField {
            source,
            view: field,
            label,
            last_control_value,
        });
    }
}

/// Mount a themed selector for a stepped parameter.
pub(crate) unsafe fn add_dropdown_field(
    view: &mut PluginEditorView,
    parent: Id,
    control_index: usize,
    rect: CGRect,
) {
    unsafe {
        add_select_field(
            view,
            parent,
            SelectSource::Control(control_index),
            rect,
            true,
        )
    };
}

/// Mount the preset catalog field. The same overlay and interaction
/// primitive serves stepped controls and hierarchical preset paths.
pub(crate) unsafe fn add_preset_dropdown_field(
    view: &mut PluginEditorView,
    parent: Id,
    rect: CGRect,
) {
    unsafe { add_select_field(view, parent, SelectSource::Presets, rect, false) };
}

/// Keep stepped-control field labels honest against host automation.
pub(crate) fn update_dropdowns(view: &mut PluginEditorView) {
    for field_index in 0..view.dropdowns.fields.len() {
        let (control_index, label, field_view, previous) = {
            let field = &view.dropdowns.fields[field_index];
            let SelectSource::Control(control_index) = field.source else {
                continue;
            };
            (
                control_index,
                field.label,
                field.view,
                field.last_control_value,
            )
        };
        let current = view.plain(control_index).to_bits();
        if previous == current {
            continue;
        }
        view.dropdowns.fields[field_index].last_control_value = current;
        unsafe {
            label_set_kerned(
                label,
                &control_step_title(
                    view,
                    control_index,
                    selected_control_step(view, control_index),
                ),
                ns_font_role(theme::role_group()),
                ns_color_token(theme::INK_MD),
                0.6,
            );
            msg_void_int(field_view, sel(c"setNeedsDisplay:"), 1);
        }
    }
}

pub(crate) fn select_preset(view: &mut PluginEditorView, preset_index: usize) {
    if preset_index >= view.presets.len() {
        return;
    }
    view.dropdowns.selected_preset = preset_index;
    let title = field_title(view, SelectSource::Presets);
    let preset_fields: Vec<(Id, Id)> = view
        .dropdowns
        .fields
        .iter()
        .filter(|field| field.source == SelectSource::Presets)
        .map(|field| (field.view, field.label))
        .collect();
    for (field, label) in preset_fields {
        unsafe {
            label_set_kerned(
                label,
                &title,
                ns_font_role(theme::role_group()),
                ns_color_token(theme::SHELL_CONTROL_INK),
                0.6,
            );
            msg_void_int(field, sel(c"setNeedsDisplay:"), 1);
        }
    }
    view.apply_preset(preset_index);
}

pub(crate) fn select_relative_preset(view: &mut PluginEditorView, offset: isize) {
    if view.presets.is_empty() {
        return;
    }
    let count = view.presets.len() as isize;
    let current = view.dropdowns.selected_preset.min(view.presets.len() - 1) as isize;
    let next = (current + offset).rem_euclid(count) as usize;
    select_preset(view, next);
}

// ------------------------------------------------------- open/dismiss

unsafe fn redraw_fields_for_source(view: &PluginEditorView, source: SelectSource) {
    for field in &view.dropdowns.fields {
        if field.source == source {
            unsafe { msg_void_int(field.view, sel(c"setNeedsDisplay:"), 1) };
        }
    }
}

unsafe fn refresh_panel_labels(view: &mut PluginEditorView, panel_index: usize) {
    let selected = view
        .dropdowns
        .open
        .as_ref()
        .map(|open| selected_value(view, open.source))
        .unwrap_or(0);
    let Some(panel) = view
        .dropdowns
        .open
        .as_mut()
        .and_then(|open| open.panels.get_mut(panel_index))
    else {
        return;
    };
    let menu_bounds = unsafe { msg_rect(panel.menu, sel(c"bounds")) };
    for visible_index in 0..panel.labels.len() {
        let label = panel.labels[visible_index];
        let node_index = panel.scroll_offset + visible_index;
        let Some(node) = panel.nodes.get(node_index) else {
            unsafe { msg_void_int(label, sel(c"setHidden:"), 1) };
            continue;
        };
        unsafe {
            msg_void_int(label, sel(c"setHidden:"), 0);
            msg_void_rect(
                label,
                sel(c"setFrame:"),
                CGRect {
                    origin: CGPoint {
                        x: 12.0,
                        y: menu_bounds.size.height
                            - MENU_PADDING
                            - (visible_index as f64 + 1.0) * MENU_ROW_HEIGHT
                            + (MENU_ROW_HEIGHT - 16.0) * 0.5,
                    },
                    size: CGSize {
                        width: (menu_bounds.size.width
                            - if node.children.is_empty() { 22.0 } else { 36.0 })
                        .max(1.0),
                        height: 16.0,
                    },
                },
            );
            let tone = if node.selection == Some(selected) {
                theme::ACCENT
            } else {
                theme::INK_MD
            };
            label_set_kerned(
                label,
                &node.title,
                ns_font_role(theme::role_group()),
                ns_color_token(tone),
                0.6,
            );
        }
    }
    unsafe { msg_void_int(panel.menu, sel(c"setNeedsDisplay:"), 1) };
}

fn visible_row_count(node_count: usize, available_height: f64) -> usize {
    node_count
        .clamp(1, MAX_VISIBLE_ROWS)
        .min(
            ((available_height - MENU_PADDING * 2.0) / MENU_ROW_HEIGHT)
                .floor()
                .max(1.0) as usize,
        )
        .max(1)
}

unsafe fn make_panel(
    editor: *mut PluginEditorView,
    scrim: Id,
    panel_index: usize,
    nodes: Vec<SelectNode>,
    frame: CGRect,
    visible_rows: usize,
) -> SelectPanel {
    unsafe {
        let raw = msg_id(menu_class() as Id, sel(c"alloc"));
        let menu = msg_id_rect(raw, sel(c"initWithFrame:"), frame);
        set_editor_ivar(menu_class(), menu, editor);
        msg_void_int(menu, sel(c"setTag:"), panel_index as i64);
        msg_void_id(scrim, sel(c"addSubview:"), menu);

        let mut labels = Vec::with_capacity(visible_rows);
        for _ in 0..visible_rows {
            let label = make_label(CGRect {
                origin: CGPoint { x: 0.0, y: 0.0 },
                size: CGSize {
                    width: frame.size.width,
                    height: 16.0,
                },
            });
            msg_void_id(menu, sel(c"addSubview:"), label);
            labels.push(label);
        }
        let tracking_area = {
            let raw = msg_id(class(c"NSTrackingArea"), sel(c"alloc"));
            let initialize: unsafe extern "C" fn(Id, Sel, CGRect, u64, Id, Id) -> Id =
                core::mem::transmute(objc_msgSend as *const ());
            initialize(
                raw,
                sel(c"initWithRect:options:owner:userInfo:"),
                msg_rect(menu, sel(c"bounds")),
                0x282,
                menu,
                core::ptr::null_mut(),
            )
        };
        msg_void_id(menu, sel(c"addTrackingArea:"), tracking_area);
        SelectPanel {
            menu,
            nodes,
            labels,
            hover: None,
            scroll_offset: 0,
            visible_rows,
        }
    }
}

unsafe fn open_menu(view: &mut PluginEditorView, field: Id, source: SelectSource) {
    let host = view.dropdowns.host;
    if host.is_null() {
        return;
    }
    unsafe {
        dismiss(view);
        let nodes = nodes_for_source(view, source);
        let field_bounds = msg_rect(field, sel(c"bounds"));
        let field_frame_in_host: CGRect = {
            let convert: unsafe extern "C" fn(Id, Sel, CGRect, Id) -> CGRect =
                core::mem::transmute(objc_msgSend as *const ());
            convert(field, sel(c"convertRect:toView:"), field_bounds, host)
        };
        let host_bounds = msg_rect(host, sel(c"bounds"));
        let menu_width = if source == SelectSource::Presets {
            ROOT_PRESET_MENU_WIDTH
        } else {
            field_frame_in_host.size.width.max(96.0)
        }
        .min((host_bounds.size.width - 16.0).max(96.0));
        let desired_height =
            nodes.len().clamp(1, MAX_VISIBLE_ROWS) as f64 * MENU_ROW_HEIGHT + MENU_PADDING * 2.0;
        let below_y = field_frame_in_host.origin.y + field_frame_in_host.size.height + MENU_GAP;
        let room_below = host_bounds.size.height - below_y - 8.0;
        let room_above = field_frame_in_host.origin.y - 8.0;
        let opens_below = room_below >= desired_height || room_below >= room_above;
        let available_height = if opens_below { room_below } else { room_above };
        let visible_rows = visible_row_count(nodes.len(), available_height);
        let menu_height = visible_rows as f64 * MENU_ROW_HEIGHT + MENU_PADDING * 2.0;
        let menu_y = if opens_below {
            below_y
        } else {
            (field_frame_in_host.origin.y - MENU_GAP - menu_height).max(0.0)
        };
        let menu_x = field_frame_in_host
            .origin
            .x
            .min(host_bounds.size.width - menu_width - 8.0)
            .max(8.0);
        let raw = msg_id(scrim_class() as Id, sel(c"alloc"));
        let scrim = msg_id_rect(raw, sel(c"initWithFrame:"), host_bounds);
        set_editor_ivar(scrim_class(), scrim, view.self_ptr);
        msg_void_id(host, sel(c"addSubview:"), scrim);

        let root_panel = make_panel(
            view.self_ptr,
            scrim,
            0,
            nodes,
            CGRect {
                origin: CGPoint {
                    x: menu_x,
                    y: menu_y,
                },
                size: CGSize {
                    width: menu_width,
                    height: menu_height,
                },
            },
            visible_rows,
        );
        let root_menu = root_panel.menu;
        view.dropdowns.open = Some(OpenSelectMenu {
            scrim,
            source,
            panels: vec![root_panel],
        });
        refresh_panel_labels(view, 0);
        redraw_fields_for_source(view, source);
        let window = msg_id(root_menu, sel(c"window"));
        if !window.is_null() {
            msg_void_id(window, sel(c"makeFirstResponder:"), root_menu);
        }
    }
}

unsafe fn close_panels_after(view: &mut PluginEditorView, panel_index: usize) {
    let Some(open) = view.dropdowns.open.as_mut() else {
        return;
    };
    while open.panels.len() > panel_index + 1 {
        if let Some(panel) = open.panels.pop() {
            let _ = unsafe { msg_id(panel.menu, sel(c"removeFromSuperview")) };
        }
    }
}

unsafe fn open_child_panel(
    view: &mut PluginEditorView,
    parent_panel_index: usize,
    visible_row_index: usize,
    nodes: Vec<SelectNode>,
) {
    if nodes.is_empty() {
        return;
    }
    unsafe { close_panels_after(view, parent_panel_index) };
    let Some((scrim, parent_menu)) = view.dropdowns.open.as_ref().and_then(|open| {
        open.panels
            .get(parent_panel_index)
            .map(|panel| (open.scrim, panel.menu))
    }) else {
        return;
    };
    unsafe {
        let host_bounds = msg_rect(scrim, sel(c"bounds"));
        let parent_frame = msg_rect(parent_menu, sel(c"frame"));
        let menu_width = CHILD_MENU_WIDTH.min((host_bounds.size.width - 16.0).max(96.0));
        let visible_rows = visible_row_count(nodes.len(), host_bounds.size.height - 16.0);
        let menu_height = visible_rows as f64 * MENU_ROW_HEIGHT + MENU_PADDING * 2.0;
        let right_x = parent_frame.origin.x + parent_frame.size.width + MENU_GAP;
        let menu_x = if right_x + menu_width <= host_bounds.size.width - 8.0 {
            right_x
        } else {
            (parent_frame.origin.x - MENU_GAP - menu_width).max(8.0)
        };
        let aligned_y =
            parent_frame.origin.y + MENU_PADDING + visible_row_index as f64 * MENU_ROW_HEIGHT;
        let menu_y = aligned_y
            .min(host_bounds.size.height - menu_height - 8.0)
            .max(8.0);
        let panel_index = parent_panel_index + 1;
        let panel = make_panel(
            view.self_ptr,
            scrim,
            panel_index,
            nodes,
            CGRect {
                origin: CGPoint {
                    x: menu_x,
                    y: menu_y,
                },
                size: CGSize {
                    width: menu_width,
                    height: menu_height,
                },
            },
            visible_rows,
        );
        let child_menu = panel.menu;
        if let Some(open) = view.dropdowns.open.as_mut() {
            open.panels.push(panel);
        }
        refresh_panel_labels(view, panel_index);
        let window = msg_id(child_menu, sel(c"window"));
        if !window.is_null() {
            msg_void_id(window, sel(c"makeFirstResponder:"), child_menu);
        }
    }
}

pub(crate) unsafe fn dismiss(view: &mut PluginEditorView) {
    if let Some(open) = view.dropdowns.open.take() {
        let _ = unsafe { msg_id(open.scrim, sel(c"removeFromSuperview")) };
        unsafe { redraw_fields_for_source(view, open.source) };
    }
}

fn row_at(menu_height: f64, visible_rows: usize, point: CGPoint) -> Option<usize> {
    let from_top = menu_height - point.y - MENU_PADDING;
    if from_top < 0.0 {
        return None;
    }
    let row = (from_top / MENU_ROW_HEIGHT) as usize;
    (row < visible_rows).then_some(row)
}

// ------------------------------------------------------ objc methods

unsafe fn editor_of(class: Class, object: Id) -> Option<&'static mut PluginEditorView> {
    unsafe {
        let ivar = class_getInstanceVariable(class, c"editor_ptr".as_ptr());
        if ivar.is_null() {
            return None;
        }
        (object_getIvar(object, ivar) as *mut PluginEditorView).as_mut()
    }
}

unsafe fn set_editor_ivar(class: Class, object: Id, editor: *mut PluginEditorView) {
    unsafe {
        let ivar = class_getInstanceVariable(class, c"editor_ptr".as_ptr());
        if !ivar.is_null() {
            object_setIvar(object, ivar, editor as Id);
        }
    }
}

unsafe fn local_point(object: Id, event: Id) -> CGPoint {
    unsafe {
        let location_in_window: unsafe extern "C" fn(Id, Sel) -> CGPoint =
            core::mem::transmute(objc_msgSend as *const ());
        let window_point = location_in_window(event, sel(c"locationInWindow"));
        let convert: unsafe extern "C" fn(Id, Sel, CGPoint, Id) -> CGPoint =
            core::mem::transmute(objc_msgSend as *const ());
        convert(
            object,
            sel(c"convertPoint:fromView:"),
            window_point,
            core::ptr::null_mut(),
        )
    }
}

unsafe fn msg_rect(object: Id, selector: Sel) -> CGRect {
    unsafe {
        let send: unsafe extern "C" fn(Id, Sel) -> CGRect =
            core::mem::transmute(objc_msgSend as *const ());
        send(object, selector)
    }
}

unsafe extern "C" fn field_draw(this: Id, _command: Sel, _dirty: CGRect) {
    let bounds = unsafe { msg_rect(this, sel(c"bounds")) };
    let (width, height) = (bounds.size.width, bounds.size.height);
    let source = unsafe { editor_of(field_class(), this) }
        .and_then(|view| {
            let field_index = unsafe { view_tag(this, sel(c"tag")) };
            view.dropdowns
                .fields
                .get(field_index as usize)
                .map(|field| field.source)
        })
        .unwrap_or(SelectSource::Presets);
    let mut scene = Scene::new();
    if source != SelectSource::Presets {
        scene.solid(
            rounded_select_path(0.5, 0.5, width - 1.0, height - 1.0, 5.0),
            select_color(theme::BG_1),
        );
        scene.stroke(
            rounded_select_path(0.5, 0.5, width - 1.0, height - 1.0, 5.0),
            Stroke::new(select_color(theme::LINE), 1.0),
        );
        let center_y = height * 0.5;
        scene.stroke(
            Path::polyline(&[
                pt(width - 18.0, center_y + 2.0),
                pt(width - 14.0, center_y - 2.0),
                pt(width - 10.0, center_y + 2.0),
            ]),
            Stroke::round(select_color(theme::INK_LO), 1.4),
        );
    }
    unsafe { draw_select_scene(&scene) };
}

unsafe extern "C" fn field_mouse_down(this: Id, _command: Sel, _event: Id) {
    let Some(view) = (unsafe { editor_of(field_class(), this) }) else {
        return;
    };
    let field_index = unsafe { view_tag(this, sel(c"tag")) };
    if field_index < 0 {
        return;
    }
    let Some(field) = view.dropdowns.fields.get(field_index as usize) else {
        return;
    };
    let source = field.source;
    if view
        .dropdowns
        .open
        .as_ref()
        .is_some_and(|open| open.source == source)
    {
        unsafe { dismiss(view) };
    } else {
        unsafe { open_menu(view, this, source) };
    }
}

unsafe extern "C" fn menu_draw(this: Id, _command: Sel, _dirty: CGRect) {
    let Some(view) = (unsafe { editor_of(menu_class(), this) }) else {
        return;
    };
    let panel_index = unsafe { view_tag(this, sel(c"tag")) };
    if panel_index < 0 {
        return;
    }
    let Some(panel) = view
        .dropdowns
        .open
        .as_ref()
        .and_then(|open| open.panels.get(panel_index as usize))
    else {
        return;
    };
    let bounds = unsafe { msg_rect(this, sel(c"bounds")) };
    let (width, height) = (bounds.size.width, bounds.size.height);
    let mut scene = Scene::new();
    scene.solid(
        rounded_select_path(0.5, 0.5, width - 1.0, height - 1.0, 6.0),
        select_color(theme::BG_1),
    );
    scene.stroke(
        rounded_select_path(0.5, 0.5, width - 1.0, height - 1.0, 6.0),
        Stroke::new(select_color(theme::LINE), 1.0),
    );
    if let Some(hovered_node) = panel.hover {
        if hovered_node >= panel.scroll_offset
            && hovered_node < panel.scroll_offset + panel.visible_rows
        {
            let visible_index = hovered_node - panel.scroll_offset;
            let top = height - MENU_PADDING - (visible_index as f64 + 1.0) * MENU_ROW_HEIGHT;
            let accent = theme::ACCENT;
            scene.solid(
                rounded_select_path(3.0, top + 1.0, width - 6.0, MENU_ROW_HEIGHT - 2.0, 4.0),
                Color::rgba(accent.0, accent.1, accent.2, 0.14),
            );
        }
    }
    for visible_index in 0..panel.visible_rows {
        let node_index = panel.scroll_offset + visible_index;
        let Some(node) = panel.nodes.get(node_index) else {
            break;
        };
        if !node.children.is_empty() {
            let x = width - 15.0;
            let center_y = height - MENU_PADDING - (visible_index as f64 + 0.5) * MENU_ROW_HEIGHT;
            scene.stroke(
                Path::polyline(&[
                    pt(x - 2.0, center_y + 3.0),
                    pt(x + 1.0, center_y),
                    pt(x - 2.0, center_y - 3.0),
                ]),
                Stroke::round(select_color(theme::INK_LO), 1.2),
            );
        }
    }
    if panel.scroll_offset > 0 {
        scene.solid(
            rounded_select_path(width - 4.0, height - 14.0, 2.0, 8.0, 1.0),
            select_color(theme::ACCENT),
        );
    }
    if panel.scroll_offset + panel.visible_rows < panel.nodes.len() {
        scene.solid(
            rounded_select_path(width - 4.0, 6.0, 2.0, 8.0, 1.0),
            select_color(theme::ACCENT),
        );
    }
    unsafe { draw_select_scene(&scene) };
}

unsafe extern "C" fn menu_mouse_moved(this: Id, _command: Sel, event: Id) {
    let Some(view) = (unsafe { editor_of(menu_class(), this) }) else {
        return;
    };
    let point = unsafe { local_point(this, event) };
    let bounds = unsafe { msg_rect(this, sel(c"bounds")) };
    let panel_index = unsafe { view_tag(this, sel(c"tag")) };
    if panel_index < 0 {
        return;
    }
    let panel_index = panel_index as usize;
    let Some((hover_changed, visible_row_index, child_nodes)) =
        view.dropdowns.open.as_mut().and_then(|open| {
            let panel = open.panels.get_mut(panel_index)?;
            let visible_row_index = row_at(bounds.size.height, panel.visible_rows, point);
            let hovered = visible_row_index
                .map(|visible_index| panel.scroll_offset + visible_index)
                .filter(|node_index| *node_index < panel.nodes.len());
            let hover_changed = hovered != panel.hover;
            if hover_changed {
                panel.hover = hovered;
            }
            let child_nodes = hovered
                .and_then(|node_index| panel.nodes.get(node_index))
                .map(|node| node.children.clone())
                .unwrap_or_default();
            Some((hover_changed, visible_row_index, child_nodes))
        })
    else {
        return;
    };
    if hover_changed {
        unsafe {
            msg_void_int(this, sel(c"setNeedsDisplay:"), 1);
            close_panels_after(view, panel_index);
            if !child_nodes.is_empty() {
                if let Some(visible_row_index) = visible_row_index {
                    open_child_panel(view, panel_index, visible_row_index, child_nodes);
                }
            }
        }
    }
}

fn apply_select_row(view: &mut PluginEditorView, source: SelectSource, selection: usize) {
    match source {
        SelectSource::Control(control_index) => {
            view.write_control_plain(control_index, selection as f64, true);
        }
        SelectSource::Presets => select_preset(view, selection),
    }
}

unsafe extern "C" fn menu_mouse_down(this: Id, _command: Sel, event: Id) {
    let Some(view) = (unsafe { editor_of(menu_class(), this) }) else {
        return;
    };
    let point = unsafe { local_point(this, event) };
    let bounds = unsafe { msg_rect(this, sel(c"bounds")) };
    let panel_index = unsafe { view_tag(this, sel(c"tag")) };
    if panel_index < 0 {
        return;
    }
    let Some((source, visible_row_index, node)) = view.dropdowns.open.as_ref().and_then(|open| {
        let panel = open.panels.get(panel_index as usize)?;
        let visible_row_index = row_at(bounds.size.height, panel.visible_rows, point)?;
        let node_index = panel.scroll_offset + visible_row_index;
        panel
            .nodes
            .get(node_index)
            .cloned()
            .map(|node| (open.source, visible_row_index, node))
    }) else {
        return;
    };
    if !node.children.is_empty() {
        unsafe { open_child_panel(view, panel_index as usize, visible_row_index, node.children) };
    } else if let Some(selection) = node.selection {
        apply_select_row(view, source, selection);
        unsafe { dismiss(view) };
    }
}

unsafe extern "C" fn menu_scroll(this: Id, _command: Sel, event: Id) {
    let Some(view) = (unsafe { editor_of(menu_class(), this) }) else {
        return;
    };
    let delta = unsafe { msg_double(event, sel(c"scrollingDeltaY")) };
    let panel_index = unsafe { view_tag(this, sel(c"tag")) };
    if panel_index < 0 {
        return;
    }
    let Some(panel) = view
        .dropdowns
        .open
        .as_mut()
        .and_then(|open| open.panels.get_mut(panel_index as usize))
    else {
        return;
    };
    let maximum_offset = panel.nodes.len().saturating_sub(panel.visible_rows);
    let next_offset = if delta > 0.0 {
        panel.scroll_offset.saturating_sub(1)
    } else if delta < 0.0 {
        (panel.scroll_offset + 1).min(maximum_offset)
    } else {
        panel.scroll_offset
    };
    if next_offset != panel.scroll_offset {
        panel.scroll_offset = next_offset;
        panel.hover = None;
        unsafe {
            close_panels_after(view, panel_index as usize);
            refresh_panel_labels(view, panel_index as usize);
        };
    }
}

unsafe extern "C" fn menu_hit_test(this: Id, _command: Sel, point: CGPoint) -> Id {
    // AppKit asks an NSView to hit-test using the coordinate system of
    // its superview. Compare against the frame, not the local bounds;
    // otherwise the scrim steals clicks whenever the menu is offset.
    let frame = unsafe { msg_rect(this, sel(c"frame")) };
    let inside = point.x >= frame.origin.x
        && point.x <= frame.origin.x + frame.size.width
        && point.y >= frame.origin.y
        && point.y <= frame.origin.y + frame.size.height;
    if inside {
        this
    } else {
        core::ptr::null_mut()
    }
}

unsafe extern "C" fn scrim_mouse_down(this: Id, _command: Sel, _event: Id) {
    if let Some(view) = unsafe { editor_of(scrim_class(), this) } {
        unsafe { dismiss(view) };
    }
}

unsafe extern "C" fn scrim_is_flipped(_this: Id, _command: Sel) -> bool {
    true
}

unsafe extern "C" fn accepts_first_responder(_this: Id, _command: Sel) -> bool {
    true
}

// ------------------------------------------------------ class setup

unsafe fn make_label(rect: CGRect) -> Id {
    unsafe {
        let raw = msg_id(class(c"NSTextField"), sel(c"alloc"));
        let label = msg_id_rect(raw, sel(c"initWithFrame:"), rect);
        msg_void_int(label, sel(c"setBezeled:"), 0);
        msg_void_int(label, sel(c"setDrawsBackground:"), 0);
        msg_void_int(label, sel(c"setEditable:"), 0);
        msg_void_int(label, sel(c"setSelectable:"), 0);
        label
    }
}

unsafe fn get_or_make(name: &core::ffi::CStr, build: fn(Class)) -> Class {
    unsafe {
        let existing = objc_lookUpClass(name.as_ptr());
        if !existing.is_null() {
            return existing;
        }
        let class = objc_allocateClassPair(class(c"NSView"), name.as_ptr(), 0);
        let _ = class_addIvar(
            class,
            c"editor_ptr".as_ptr(),
            core::mem::size_of::<*const c_void>(),
            3,
            c"^v".as_ptr(),
        );
        let _ = class_addIvar(
            class,
            c"field_index".as_ptr(),
            core::mem::size_of::<i64>(),
            3,
            c"q".as_ptr(),
        );
        build(class);
        objc_registerClassPair(class);
        class
    }
}

unsafe extern "C" fn view_tag(this: Id, _command: Sel) -> i64 {
    unsafe {
        let class = object_getClass(this);
        let ivar = class_getInstanceVariable(class, c"field_index".as_ptr());
        if ivar.is_null() {
            return -1;
        }
        object_getIvar(this, ivar) as i64
    }
}

unsafe extern "C" fn view_set_tag(this: Id, _command: Sel, value: i64) {
    unsafe {
        let class = object_getClass(this);
        let ivar = class_getInstanceVariable(class, c"field_index".as_ptr());
        if !ivar.is_null() {
            object_setIvar(this, ivar, value as Id);
        }
    }
}

unsafe fn field_class() -> Class {
    unsafe {
        get_or_make(c"StaveSelectField", |class| {
            let _ = class_addMethod(
                class,
                sel(c"tag"),
                view_tag as *const c_void,
                c"q@:".as_ptr(),
            );
            let _ = class_addMethod(
                class,
                sel(c"setTag:"),
                view_set_tag as *const c_void,
                c"v@:q".as_ptr(),
            );
            let _ = class_addMethod(
                class,
                sel(c"drawRect:"),
                field_draw as *const c_void,
                c"v@:{CGRect={CGPoint=dd}{CGSize=dd}}".as_ptr(),
            );
            let _ = class_addMethod(
                class,
                sel(c"mouseDown:"),
                field_mouse_down as *const c_void,
                c"v@:@".as_ptr(),
            );
            register_shell_control_hover_methods(class);
        })
    }
}

unsafe fn menu_class() -> Class {
    unsafe {
        get_or_make(c"StaveSelectMenu", |class| {
            let _ = class_addMethod(
                class,
                sel(c"tag"),
                view_tag as *const c_void,
                c"q@:".as_ptr(),
            );
            let _ = class_addMethod(
                class,
                sel(c"setTag:"),
                view_set_tag as *const c_void,
                c"v@:q".as_ptr(),
            );
            let _ = class_addMethod(
                class,
                sel(c"drawRect:"),
                menu_draw as *const c_void,
                c"v@:{CGRect={CGPoint=dd}{CGSize=dd}}".as_ptr(),
            );
            let _ = class_addMethod(
                class,
                sel(c"mouseDown:"),
                menu_mouse_down as *const c_void,
                c"v@:@".as_ptr(),
            );
            let _ = class_addMethod(
                class,
                sel(c"mouseMoved:"),
                menu_mouse_moved as *const c_void,
                c"v@:@".as_ptr(),
            );
            let _ = class_addMethod(
                class,
                sel(c"scrollWheel:"),
                menu_scroll as *const c_void,
                c"v@:@".as_ptr(),
            );
            let _ = class_addMethod(
                class,
                sel(c"hitTest:"),
                menu_hit_test as *const c_void,
                c"@@:{CGPoint=dd}".as_ptr(),
            );
            let _ = class_addMethod(
                class,
                sel(c"acceptsFirstResponder"),
                accepts_first_responder as *const c_void,
                c"B@:".as_ptr(),
            );
        })
    }
}

unsafe fn scrim_class() -> Class {
    unsafe {
        get_or_make(c"StaveSelectScrim", |class| {
            let _ = class_addMethod(
                class,
                sel(c"mouseDown:"),
                scrim_mouse_down as *const c_void,
                c"v@:@".as_ptr(),
            );
            let _ = class_addMethod(
                class,
                sel(c"isFlipped"),
                scrim_is_flipped as *const c_void,
                c"B@:".as_ptr(),
            );
        })
    }
}
