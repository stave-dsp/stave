//! PATCH CABLES — the modulation UX (user-redirected 2026-07-07;
//! it replaces the matrix table + chip drag): every modulation
//! source is an OUTLET jack, every modulatable knob grows an INLET
//! jack, and routes are cables dragged between them. The cables are
//! a pure VIEW of the same slot params the retired table wrote
//! (routes are params) — automation, presets, and state
//! machinery see nothing new.
//!
//! Interaction contract:
//! * drag an outlet onto an inlet → connect (fresh routes land at
//!   depth 0.5, immediately audible);
//! * drag the same outlet onto the same inlet → disconnect;
//! * GRAB a connected inlet's socket → the cable comes off in your
//!   hand (the modular idiom): drop it on another inlet to re-route,
//!   drop it in space to remove;
//! * a connected inlet grows a SMALL SLIDER under its jack — the
//!   route's depth, bipolar ±1 about a center notch (the knob ring
//!   and the cable's opacity both track);
//! * everything derives from the `ModRings`/`ModSources`
//!   declarations — no new widget kinds.
//!
//! Pieces: `StaveJack` (one tiny NSView per jack, mouse owner),
//! `StaveCables` (a hitTest-transparent overlay ABOVE the tree that
//! draws every cable + the drag ghost), and `plan()` — the pure,
//! unit-tested slot policy (toggle → first free → steal last).

use core::ffi::c_void;

extern crate alloc;
use alloc::vec::Vec;

use stave_ui::scene::{pt, Path, Pt, Scene, Stroke};
use stave_ui::signal::Changed;
use stave_ui::style::Color;

use crate::objc::*;
use crate::theme;
use crate::view::PluginEditorView;
use crate::*;

const JACK: f64 = 14.0;
/// Inlet views are TALLER: the socket on top, the depth slider
/// under it (user-specced). Sliders exist only while connected.
const TGT_W: f64 = 40.0;
const TGT_H: f64 = 30.0;
/// Half-width of the depth slider's throw (depth ±1 maps to ±this).
const THROW: f64 = 15.0;

pub(crate) struct CableState {
    pub(crate) overlay: Id,
    pub(crate) jacks: Vec<Jack>,
    pub(crate) drag: Option<Drag>,
    pub(crate) memo: Changed<Vec<u64>>,
}

impl Default for CableState {
    fn default() -> Self {
        Self {
            overlay: core::ptr::null_mut(),
            jacks: Vec::new(),
            drag: None,
            memo: Changed::new(),
        }
    }
}

pub(crate) struct Jack {
    pub(crate) kind: JackKind,
    pub(crate) view: Id,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) enum JackKind {
    /// Source enum step k (1-based — 0 is OFF and has no jack).
    Source(usize),
    /// Target index into the ModRings targets list.
    Target(usize),
}

pub(crate) enum Drag {
    /// A cable in flight from a source jack; `to` in overlay coords.
    Cable { from: usize, to: CGPoint },
    /// A cable pulled OFF an inlet, still plugged into source k:
    /// drop on an inlet to re-route slot, drop in space to remove.
    Recable { slot: usize, k: usize, to: CGPoint },
    /// A depth edit riding a connected inlet's slider.
    Depth { depth_ci: usize },
}

// ------------------------------------------------------ pure policy
/// Which slot a fresh cable (source k → target t) uses. `routes` is
/// each slot's current (src value, tgt value).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Plan {
    /// The identical route is live — the drag toggles it OFF.
    Disconnect(usize),
    /// Write the route here (an OFF slot, or the stolen last slot).
    Connect(usize),
}

pub(crate) fn plan(routes: &[(usize, usize)], k: usize, t: usize) -> Plan {
    if let Some(i) = routes.iter().position(|&(s, tt)| s == k && tt == t) {
        return Plan::Disconnect(i);
    }
    if let Some(i) = routes.iter().position(|&(s, _)| s == 0) {
        return Plan::Connect(i);
    }
    Plan::Connect(routes.len().saturating_sub(1))
}

/// Live routes as (slot, src k, target t, depth).
fn routes(v: &PluginEditorView) -> Vec<(usize, usize, usize, f64)> {
    let Some(r) = v.rings.as_ref() else {
        return Vec::new();
    };
    r.slots
        .iter()
        .enumerate()
        .filter_map(|(i, &(s, t, d))| {
            let k = v.plain(s) as usize;
            (k > 0).then(|| (i, k, v.plain(t) as usize, v.plain(d)))
        })
        .collect()
}

fn col(c: theme::Rgba) -> Color {
    Color::rgba(c.0, c.1, c.2, c.3)
}

/// Jacks on hidden pages (opal's per-op knobs live behind the OP
/// pager) take no cables and no drops: the cable appears IN CONTEXT
/// when its page shows, exactly like the knob it feeds.
unsafe fn hidden(view: Id) -> bool {
    unsafe {
        let f: unsafe extern "C" fn(Id, Sel) -> bool =
            core::mem::transmute(objc_msgSend as *const ());
        f(view, sel(c"isHiddenOrHasHiddenAncestor"))
    }
}

// ------------------------------------------------------------ mounting
/// The `ModSources` arm — the PATCH BAY, two rows: source OUTLETS
/// on top (LFOs first — they are the bay's protagonists — then the
/// per-voice and global sources, enum order), labeled INLETS for
/// ORPHAN targets below (targets with no always-visible knob:
/// pitch, the per-op levels behind opal's pager, ratio 2). Labels
/// come from the params' OWN step names.
pub(crate) unsafe fn add_source_jacks(v: &mut PluginEditorView, parent: Id, rect: CGRect) {
    let Some(r) = v.rings.as_ref() else { return };
    let (src_ci, tgt_ci) = (r.slots[0].0, r.slots[0].1);
    let steps = v.param(src_ci).step_count() as usize;
    let orphans: Vec<usize> = (0..r.targets.len())
        .filter(|&t| r.targets[t].is_none())
        .collect();
    let mut order: Vec<usize> = (1..steps).collect();
    order.sort_by_key(|&k| {
        let lfo = v
            .param(src_ci)
            .step_name(k as u32)
            .unwrap_or("")
            .starts_with("LFO");
        (usize::from(!lfo), k)
    });
    let sources: Vec<(JackKind, alloc::string::String)> = order
        .into_iter()
        .map(|k| {
            (
                JackKind::Source(k),
                v.param(src_ci)
                    .step_name(k as u32)
                    .unwrap_or("")
                    .to_uppercase(),
            )
        })
        .collect();
    let inlets: Vec<(JackKind, alloc::string::String)> = orphans
        .into_iter()
        .map(|t| {
            (
                JackKind::Target(t),
                v.param(tgt_ci)
                    .step_name(t as u32)
                    .unwrap_or("")
                    .to_uppercase(),
            )
        })
        .collect();
    let two_rows = !inlets.is_empty();
    let row_h = if two_rows {
        rect.size.height * 0.5
    } else {
        rect.size.height
    };
    for (row, items) in [(0usize, sources), (1, inlets)] {
        if items.is_empty() {
            continue;
        }
        let row_y = rect.origin.y + row as f64 * row_h;
        unsafe { add_jack_row(v, parent, rect, row_y, row_h, items) };
    }
}

unsafe fn add_jack_row(
    v: &mut PluginEditorView,
    parent: Id,
    rect: CGRect,
    row_y: f64,
    row_h: f64,
    items: Vec<(JackKind, alloc::string::String)>,
) {
    let n = items.len().max(1) as f64;
    let item_w = rect.size.width / n;
    for (i, (kind, label)) in items.into_iter().enumerate() {
        let x = rect.origin.x + i as f64 * item_w;
        let jy = row_y + (row_h - JACK) * 0.5;
        unsafe {
            // Inlets are tall (jack + depth slider); align the
            // SOCKETS, let the slider hang below the row line.
            let frame = match kind {
                JackKind::Source(_) => CGRect {
                    origin: CGPoint {
                        x: x + item_w * 0.5 - JACK - 22.0,
                        y: jy,
                    },
                    size: CGSize {
                        width: JACK,
                        height: JACK,
                    },
                },
                JackKind::Target(_) => CGRect {
                    origin: CGPoint {
                        x: x + item_w * 0.5 - TGT_W * 0.5 - 22.0,
                        y: jy,
                    },
                    size: CGSize {
                        width: TGT_W,
                        height: TGT_H,
                    },
                },
            };
            let jack = add_jack(v, parent, frame, kind);
            let _ = jack;
            let lbl_raw = msg_id(class(c"NSTextField"), sel(c"alloc"));
            let lbl = msg_id_rect(
                lbl_raw,
                sel(c"initWithFrame:"),
                CGRect {
                    origin: CGPoint {
                        x: x + item_w * 0.5 - 18.0,
                        y: jy - 1.0,
                    },
                    size: CGSize {
                        width: item_w * 0.5 + 14.0,
                        height: 14.0,
                    },
                },
            );
            msg_void_int(lbl, sel(c"setBezeled:"), 0);
            msg_void_int(lbl, sel(c"setDrawsBackground:"), 0);
            msg_void_int(lbl, sel(c"setEditable:"), 0);
            msg_void_int(lbl, sel(c"setSelectable:"), 0);
            label_set_kerned(
                lbl,
                &label,
                ns_font_role(theme::role_tick()),
                ns_color_token(theme::INK_LO),
                0.6,
            );
            msg_void_id(parent, sel(c"addSubview:"), lbl);
        }
    }
}

/// The post-mount pass (knobs must exist): an inlet jack at the
/// top-right of every ringed knob, then the cable overlay ABOVE
/// the whole tree. Called at the end of `try_attach_owned`.
pub(crate) unsafe fn mount_target_jacks_and_overlay(v: &mut PluginEditorView, host: Id) {
    let Some(r) = v.rings.as_ref() else { return };
    let targets: Vec<(usize, usize)> = r
        .targets
        .iter()
        .enumerate()
        .filter_map(|(t, ci)| ci.map(|ci| (t, ci)))
        .collect();
    for (t, ci) in targets {
        let Some(&(_, knob, _, _)) = v.knobs.iter().find(|&&(kci, ..)| kci == ci) else {
            continue;
        };
        unsafe {
            let parent = msg_id(knob, sel(c"superview"));
            if parent.is_null() {
                continue;
            }
            let kf: CGRect = msg_rect(knob, sel(c"frame"));
            // Socket at the knob's top-right; the slider hangs just
            // off the face's right edge.
            let rect = CGRect {
                origin: CGPoint {
                    x: kf.origin.x + kf.size.width - 10.0,
                    y: kf.origin.y - 2.0,
                },
                size: CGSize {
                    width: TGT_W,
                    height: TGT_H,
                },
            };
            add_jack(v, parent, rect, JackKind::Target(t));
        }
    }
    unsafe {
        let bounds: CGRect = msg_rect(host, sel(c"bounds"));
        let raw = msg_id(overlay_class() as Id, sel(c"alloc"));
        let overlay = msg_id_rect(raw, sel(c"initWithFrame:"), bounds);
        set_editor_ivar(overlay_class(), overlay, v.self_ptr);
        msg_void_id(host, sel(c"addSubview:"), overlay);
        v.cables.overlay = overlay;
    }
}

unsafe fn add_jack(v: &mut PluginEditorView, parent: Id, rect: CGRect, kind: JackKind) -> Id {
    unsafe {
        let cls = jack_class();
        let raw = msg_id(cls as Id, sel(c"alloc"));
        let jack = msg_id_rect(raw, sel(c"initWithFrame:"), rect);
        set_editor_ivar(cls, jack, v.self_ptr);
        let idx = v.cables.jacks.len() as i64;
        set_tag_ivar(cls, jack, idx);
        msg_void_id(parent, sel(c"addSubview:"), jack);
        v.cables.jacks.push(Jack { kind, view: jack });
        jack
    }
}

/// The tick half: repaint jacks + overlay when any slot moved.
pub(crate) fn update_cables(v: &mut PluginEditorView) {
    let Some(r) = v.rings.as_ref() else { return };
    let mut now: Vec<u64> = r
        .slots
        .iter()
        .flat_map(|&(s, t, d)| [s, t, d])
        .map(|ci| v.plain(ci).to_bits())
        .collect();
    // Page flips move jacks in and out of view — cables follow.
    now.extend(
        v.cables
            .jacks
            .iter()
            .map(|j| u64::from(unsafe { hidden(j.view) })),
    );
    if v.cables.memo.changed(now) {
        unsafe { dirty_cables(v) };
    }
}

unsafe fn dirty_cables(v: &PluginEditorView) {
    unsafe {
        if !v.cables.overlay.is_null() {
            msg_void_int(v.cables.overlay, sel(c"setNeedsDisplay:"), 1);
        }
        for j in &v.cables.jacks {
            msg_void_int(j.view, sel(c"setNeedsDisplay:"), 1);
        }
    }
}

// ------------------------------------------------------ geometry
unsafe fn msg_rect(obj: Id, s: Sel) -> CGRect {
    unsafe {
        let f: unsafe extern "C" fn(Id, Sel) -> CGRect =
            core::mem::transmute(objc_msgSend as *const ());
        f(obj, s)
    }
}

/// A jack's SOCKET center in its own (y-up) coordinates: sources
/// are square; inlets carry the socket at the top, slider below.
fn socket(kind: JackKind, b: CGRect) -> CGPoint {
    match kind {
        JackKind::Source(_) => CGPoint {
            x: b.size.width * 0.5,
            y: b.size.height * 0.5,
        },
        JackKind::Target(_) => CGPoint {
            x: b.size.width * 0.5,
            y: b.size.height - 7.0,
        },
    }
}

/// A jack's socket center in overlay coordinates.
unsafe fn jack_center(v: &PluginEditorView, j: &Jack) -> Pt {
    unsafe {
        let b: CGRect = msg_rect(j.view, sel(c"bounds"));
        let c = socket(j.kind, b);
        let f: unsafe extern "C" fn(Id, Sel, CGPoint, Id) -> CGPoint =
            core::mem::transmute(objc_msgSend as *const ());
        let p = f(j.view, sel(c"convertPoint:toView:"), c, v.cables.overlay);
        pt(p.x, p.y)
    }
}

unsafe fn event_in_overlay(v: &PluginEditorView, event: Id) -> CGPoint {
    unsafe {
        let f: unsafe extern "C" fn(Id, Sel) -> CGPoint =
            core::mem::transmute(objc_msgSend as *const ());
        let win = f(event, sel(c"locationInWindow"));
        let g: unsafe extern "C" fn(Id, Sel, CGPoint, Id) -> CGPoint =
            core::mem::transmute(objc_msgSend as *const ());
        g(
            v.cables.overlay,
            sel(c"convertPoint:fromView:"),
            win,
            core::ptr::null_mut(),
        )
    }
}

/// One patch cable: a sagging cubic (gravity sells the metaphor).
/// The overlay is y-UP, so sag is −y; depth drives opacity.
fn cable_path(a: Pt, b: Pt) -> Path {
    let dist = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
    let dip = 14.0 + dist * 0.12;
    Path::new()
        .move_to(a)
        .cubic_to(pt(a.x, a.y - dip), pt(b.x, b.y - dip), b)
}

/// Every live, VISIBLE cable's endpoints in overlay coords — one
/// geometry for drawing, hit-testing, and the inspector.
unsafe fn live_cables(v: &PluginEditorView) -> Vec<(usize, Pt, Pt)> {
    let find = |kind: JackKind| v.cables.jacks.iter().find(|j| j.kind == kind);
    routes(v)
        .into_iter()
        .filter_map(|(i, k, t, _)| {
            let (src, dst) = (find(JackKind::Source(k))?, find(JackKind::Target(t))?);
            if unsafe { hidden(src.view) || hidden(dst.view) } {
                return None;
            }
            Some((i, unsafe { jack_center(v, src) }, unsafe {
                jack_center(v, dst)
            }))
        })
        .collect()
}

/// Every cable wears a small INSPECT BUTTON at its midpoint — the
/// ONLY interactive spot on the overlay (a corridor around the
/// whole cable ate the jacks, and forwarding re-dispatched through
/// the host into a stack overflow; a dedicated button is immune).
fn button_center(a: Pt, b: Pt) -> Pt {
    let dist = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
    let dip = 14.0 + dist * 0.12;
    // The cubic at t = 0.5.
    pt((a.x + b.x) * 0.5, (a.y + b.y) * 0.5 - dip * 0.75)
}

const BUTTON_R: f64 = 8.0;

/// The cable whose inspect button sits under an overlay point.
unsafe fn button_at(v: &PluginEditorView, p: Pt) -> Option<usize> {
    unsafe { live_cables(v) }
        .into_iter()
        .map(|(slot, a, b)| {
            let c = button_center(a, b);
            (slot, ((p.x - c.x).powi(2) + (p.y - c.y).powi(2)).sqrt())
        })
        .filter(|&(_, d)| d < BUTTON_R + 2.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(slot, _)| slot)
}

// ------------------------------------------------------ objc methods
unsafe fn editor_of(cls: Class, this: Id) -> Option<&'static mut PluginEditorView> {
    unsafe {
        let ivar = class_getInstanceVariable(cls, c"editor_ptr".as_ptr());
        if ivar.is_null() {
            return None;
        }
        (object_getIvar(this, ivar) as *mut PluginEditorView).as_mut()
    }
}

unsafe fn set_editor_ivar(cls: Class, this: Id, editor: *mut PluginEditorView) {
    unsafe {
        let ivar = class_getInstanceVariable(cls, c"editor_ptr".as_ptr());
        if !ivar.is_null() {
            object_setIvar(this, ivar, editor as Id);
        }
    }
}

unsafe fn set_tag_ivar(cls: Class, this: Id, value: i64) {
    unsafe {
        let ivar = class_getInstanceVariable(cls, c"ci_tag".as_ptr());
        if !ivar.is_null() {
            object_setIvar(this, ivar, value as Id);
        }
    }
}

unsafe fn tag_ivar(this: Id) -> i64 {
    unsafe {
        let cls = object_getClass(this);
        let ivar = class_getInstanceVariable(cls, c"ci_tag".as_ptr());
        if ivar.is_null() {
            return -1;
        }
        object_getIvar(this, ivar) as i64
    }
}

/// Jack drawRect: socket ring (core lights when connected); a
/// connected INLET also draws its depth slider — center-notched
/// track, fill from center, thumb at depth.
unsafe extern "C" fn jack_draw(this: Id, _cmd: Sel, _dirty: CGRect) {
    let Some(v) = (unsafe { editor_of(jack_class(), this) }) else {
        return;
    };
    let idx = unsafe { tag_ivar(this) };
    let Some(jack) = v.cables.jacks.get(idx as usize) else {
        return;
    };
    let live = routes(v);
    let b = unsafe { msg_rect(this, sel(c"bounds")) };
    let sp = socket(jack.kind, b);
    let c = pt(sp.x, sp.y);
    let mut sc = Scene::new();
    sc.solid(Path::circle(c, 5.5), col(theme::BG_0));
    sc.stroke(Path::circle(c, 5.0), Stroke::new(col(theme::INK_LO), 1.2));
    match jack.kind {
        JackKind::Source(k) => {
            let lit = live.iter().any(|&(_, s, ..)| s == k);
            let (r, tone) = if lit {
                (3.2, theme::ACCENT)
            } else {
                (2.0, theme::INK_LO)
            };
            sc.solid(Path::circle(c, r), col(tone));
        }
        JackKind::Target(t) => {
            if let Some(&(_, _, _, depth)) = live.iter().rev().find(|&&(_, _, tt, _)| tt == t) {
                sc.solid(Path::circle(c, 3.2), col(theme::ACCENT));
                // The slider, under the socket (view y-up).
                let (cx, sy) = (b.size.width * 0.5, 6.0);
                let a = theme::ACCENT;
                sc.stroke(
                    Path::polyline(&[pt(cx - THROW, sy), pt(cx + THROW, sy)]),
                    Stroke::new(Color::rgba(1.0, 1.0, 1.0, 0.18), 2.0),
                );
                sc.stroke(
                    Path::polyline(&[pt(cx, sy - 3.0), pt(cx, sy + 3.0)]),
                    Stroke::new(Color::rgba(1.0, 1.0, 1.0, 0.25), 1.0),
                );
                let tx = cx + depth.clamp(-1.0, 1.0) * THROW;
                sc.stroke(
                    Path::polyline(&[pt(cx, sy), pt(tx, sy)]),
                    Stroke::round(Color::rgba(a.0, a.1, a.2, 0.9), 2.0),
                );
                sc.solid(Path::circle(pt(tx, sy), 3.0), col(theme::ACCENT));
            }
        }
    }
    unsafe { draw_scene_here(&sc) };
}

unsafe extern "C" fn jack_mouse_down(this: Id, _cmd: Sel, event: Id) {
    let Some(v) = (unsafe { editor_of(jack_class(), this) }) else {
        return;
    };
    let idx = unsafe { tag_ivar(this) };
    let Some(jack) = v.cables.jacks.get(idx as usize) else {
        return;
    };
    match jack.kind {
        JackKind::Source(_) => {
            let to = unsafe { event_in_overlay(v, event) };
            v.cables.drag = Some(Drag::Cable {
                from: idx as usize,
                to,
            });
        }
        JackKind::Target(t) => {
            // Both live on the inlet: the SOCKET (top) grabs the
            // cable off the jack; the SLIDER strip (bottom) edits
            // depth. Split at the view's vertical middle (y-up).
            let Some(&(slot, k, ..)) = routes(v).iter().rev().find(|&&(_, _, tt, _)| tt == t)
            else {
                return;
            };
            let p = unsafe { local_pt(this, event) };
            if p.y >= TGT_H * 0.5 {
                // The cable comes off in your hand.
                let to = unsafe { event_in_overlay(v, event) };
                v.cables.drag = Some(Drag::Recable { slot, k, to });
            } else {
                let depth_ci = v.rings.as_ref().map(|r| r.slots[slot].2);
                let Some(depth_ci) = depth_ci else { return };
                v.begin_control(depth_ci);
                let d = unsafe { slider_depth(this, event) };
                v.write_control_plain(depth_ci, d, false);
                v.cables.drag = Some(Drag::Depth { depth_ci });
            }
        }
    }
    unsafe { dirty_cables(v) };
}

unsafe extern "C" fn jack_mouse_dragged(this: Id, _cmd: Sel, event: Id) {
    let Some(v) = (unsafe { editor_of(jack_class(), this) }) else {
        return;
    };
    match v.cables.drag {
        Some(Drag::Cable { from, .. }) => {
            let to = unsafe { event_in_overlay(v, event) };
            v.cables.drag = Some(Drag::Cable { from, to });
            unsafe {
                if !v.cables.overlay.is_null() {
                    msg_void_int(v.cables.overlay, sel(c"setNeedsDisplay:"), 1);
                }
            }
        }
        Some(Drag::Recable { slot, k, .. }) => {
            let to = unsafe { event_in_overlay(v, event) };
            v.cables.drag = Some(Drag::Recable { slot, k, to });
            unsafe {
                if !v.cables.overlay.is_null() {
                    msg_void_int(v.cables.overlay, sel(c"setNeedsDisplay:"), 1);
                }
            }
        }
        Some(Drag::Depth { depth_ci }) => {
            let d = unsafe { slider_depth(this, event) };
            v.write_control_plain(depth_ci, d, false);
        }
        None => {}
    }
}

/// The visible inlet under an overlay point, if any. The slop is
/// generous (20 px around the socket — measured: a natural drop
/// lands ~14 px off the center; 12 px rejected it).
unsafe fn inlet_at(v: &PluginEditorView, p: CGPoint) -> Option<usize> {
    v.cables.jacks.iter().find_map(|j| match j.kind {
        JackKind::Target(t) if !unsafe { hidden(j.view) } => {
            let c = unsafe { jack_center(v, j) };
            let hit = ((p.x - c.x).powi(2) + (p.y - c.y).powi(2)).sqrt() < 20.0;
            hit.then_some(t)
        }
        _ => None,
    })
}

/// Event location in this view's own coordinates.
unsafe fn local_pt(this: Id, event: Id) -> CGPoint {
    unsafe {
        let f: unsafe extern "C" fn(Id, Sel) -> CGPoint =
            core::mem::transmute(objc_msgSend as *const ());
        let win = f(event, sel(c"locationInWindow"));
        let g: unsafe extern "C" fn(Id, Sel, CGPoint, Id) -> CGPoint =
            core::mem::transmute(objc_msgSend as *const ());
        g(
            this,
            sel(c"convertPoint:fromView:"),
            win,
            core::ptr::null_mut(),
        )
    }
}

/// Event x → depth on an inlet's slider (absolute, clamped).
unsafe fn slider_depth(this: Id, event: Id) -> f64 {
    unsafe {
        let p = local_pt(this, event);
        let b: CGRect = msg_rect(this, sel(c"bounds"));
        ((p.x - b.size.width * 0.5) / THROW).clamp(-1.0, 1.0)
    }
}

unsafe extern "C" fn jack_mouse_up(this: Id, _cmd: Sel, event: Id) {
    let Some(v) = (unsafe { editor_of(jack_class(), this) }) else {
        return;
    };
    match v.cables.drag.take() {
        Some(Drag::Depth { depth_ci, .. }) => v.end_control(depth_ci),
        Some(Drag::Recable { slot, k, .. }) => {
            let p = unsafe { event_in_overlay(v, event) };
            let (src_ci, tgt_ci) = match v.rings.as_ref() {
                Some(r) => (r.slots[slot].0, r.slots[slot].1),
                None => return,
            };
            match unsafe { inlet_at(v, p) } {
                // Dropped on an inlet: the route follows the cable.
                Some(t) => v.write_control_plain(tgt_ci, t as f64, true),
                // Dropped in space: the cable is out — route OFF.
                None => v.write_control_plain(src_ci, 0.0, true),
            }
            let _ = k;
        }
        Some(Drag::Cable { from, .. }) => {
            let p = unsafe { event_in_overlay(v, event) };
            let Some(Jack {
                kind: JackKind::Source(k),
                ..
            }) = v.cables.jacks.get(from)
            else {
                return;
            };
            let k = *k;
            let target = unsafe { inlet_at(v, p) };
            if let (Some(t), Some(r)) = (target, v.rings.as_ref()) {
                let slots = r.slots.clone();
                let live: Vec<(usize, usize)> = slots
                    .iter()
                    .map(|&(s, tt, _)| (v.plain(s) as usize, v.plain(tt) as usize))
                    .collect();
                match plan(&live, k, t) {
                    Plan::Disconnect(i) => v.write_control_plain(slots[i].0, 0.0, true),
                    Plan::Connect(i) => {
                        v.write_control_plain(slots[i].0, k as f64, true);
                        v.write_control_plain(slots[i].1, t as f64, true);
                        v.write_control_plain(slots[i].2, 0.5, true);
                    }
                }
            }
        }
        None => {}
    }
    unsafe { dirty_cables(v) };
}

/// Overlay drawRect: every live route as a cable wearing its
/// inspect button, plus the drag ghost.
unsafe extern "C" fn overlay_draw(this: Id, _cmd: Sel, _dirty: CGRect) {
    let Some(v) = (unsafe { editor_of(overlay_class(), this) }) else {
        return;
    };
    let mut sc = Scene::new();
    let a = theme::ACCENT;
    let held = match &v.cables.drag {
        Some(Drag::Recable { slot, .. }) => Some(*slot),
        _ => None,
    };
    let depths: Vec<(usize, f64)> = routes(v).into_iter().map(|(i, _, _, d)| (i, d)).collect();
    for (i, p0, p1) in unsafe { live_cables(v) } {
        if held == Some(i) {
            continue; // in your hand — drawn as the ghost below
        }
        let depth = depths
            .iter()
            .find(|&&(s, _)| s == i)
            .map_or(0.0, |&(_, d)| d);
        let alpha = 0.35 + 0.55 * depth.abs().min(1.0);
        sc.stroke(
            cable_path(p0, p1),
            Stroke::round(Color::rgba(a.0, a.1, a.2, alpha), 2.0),
        );
        sc.solid(Path::circle(p1, 3.0), Color::rgba(a.0, a.1, a.2, alpha));
        // The inspect button: a small disc riding the cable.
        let c = button_center(p0, p1);
        sc.solid(Path::circle(c, BUTTON_R), col(theme::BG_2));
        sc.stroke(
            Path::circle(c, BUTTON_R),
            Stroke::new(col(theme::LINE), 1.0),
        );
        for k in [-3.0, 0.0, 3.0] {
            sc.solid(Path::circle(pt(c.x + k, c.y), 1.1), col(theme::INK_LO));
        }
    }
    let ghost = match &v.cables.drag {
        Some(Drag::Cable { from, to }) => v.cables.jacks.get(*from).map(|j| (j, to)),
        Some(Drag::Recable { k, to, .. }) => v
            .cables
            .jacks
            .iter()
            .find(|j| j.kind == JackKind::Source(*k))
            .map(|j| (j, to)),
        _ => None,
    };
    if let Some((j, to)) = ghost {
        let p0 = unsafe { jack_center(v, j) };
        sc.stroke(
            cable_path(p0, pt(to.x, to.y)),
            Stroke::round(Color::rgba(a.0, a.1, a.2, 0.9), 2.0),
        );
    }
    unsafe { draw_scene_here(&sc) };
}

/// The overlay is a PICTURE — except each cable's inspect BUTTON.
/// `point` arrives in the FLIPPED host's coords; the overlay draws
/// y-up.
unsafe extern "C" fn overlay_hit_test(this: Id, _cmd: Sel, point: CGPoint) -> Id {
    let Some(v) = (unsafe { editor_of(overlay_class(), this) }) else {
        return core::ptr::null_mut();
    };
    if v.cables.drag.is_some() {
        return core::ptr::null_mut(); // never intercept a live drag
    }
    let b = unsafe { msg_rect(this, sel(c"bounds")) };
    let p = pt(point.x, b.size.height - point.y);
    if unsafe { button_at(v, p) }.is_some() {
        this
    } else {
        core::ptr::null_mut()
    }
}

/// Click the button, get the inspector.
unsafe extern "C" fn overlay_mouse_down(this: Id, _cmd: Sel, event: Id) {
    let Some(v) = (unsafe { editor_of(overlay_class(), this) }) else {
        return;
    };
    let win = unsafe { event_in_overlay(v, event) };
    let b = unsafe { msg_rect(this, sel(c"bounds")) };
    let p = pt(win.x, win.y);
    if let Some(slot) = unsafe { button_at(v, p) } {
        // The inspector positions in the flipped host's coords.
        let at = CGPoint {
            x: p.x,
            y: b.size.height - p.y,
        };
        unsafe { crate::widgets::inspector::open(v, slot, at) };
    }
}

unsafe fn draw_scene_here(scene: &Scene) {
    unsafe {
        let cur: Id = msg_id(class(c"NSGraphicsContext"), sel(c"currentContext"));
        if cur.is_null() {
            return;
        }
        let f: unsafe extern "C" fn(Id, Sel) -> *mut c_void =
            core::mem::transmute(objc_msgSend as *const ());
        let ctx = f(cur, sel(c"CGContext"));
        if !ctx.is_null() {
            super::scene_render::render(ctx, scene);
        }
    }
}

// ------------------------------------------------------ class setup
unsafe fn get_or_make(name: &core::ffi::CStr, build: fn(Class)) -> Class {
    unsafe {
        let existing = objc_lookUpClass(name.as_ptr());
        if !existing.is_null() {
            return existing;
        }
        let cls = objc_allocateClassPair(class(c"NSView"), name.as_ptr(), 0);
        let _ = class_addIvar(
            cls,
            c"editor_ptr".as_ptr(),
            core::mem::size_of::<*const c_void>(),
            3,
            c"^v".as_ptr(),
        );
        let _ = class_addIvar(
            cls,
            c"ci_tag".as_ptr(),
            core::mem::size_of::<i64>(),
            3,
            c"q".as_ptr(),
        );
        build(cls);
        objc_registerClassPair(cls);
        cls
    }
}

unsafe fn jack_class() -> Class {
    unsafe {
        get_or_make(c"StaveJack", |cls| {
            for (name, imp, ty) in [
                (
                    c"drawRect:",
                    jack_draw as *const c_void,
                    c"v@:{CGRect={CGPoint=dd}{CGSize=dd}}",
                ),
                (c"mouseDown:", jack_mouse_down as *const c_void, c"v@:@"),
                (
                    c"mouseDragged:",
                    jack_mouse_dragged as *const c_void,
                    c"v@:@",
                ),
                (c"mouseUp:", jack_mouse_up as *const c_void, c"v@:@"),
            ] {
                let _ = class_addMethod(cls, sel(name), imp, ty.as_ptr());
            }
        })
    }
}

unsafe fn overlay_class() -> Class {
    unsafe {
        get_or_make(c"StaveCables", |cls| {
            let _ = class_addMethod(
                cls,
                sel(c"drawRect:"),
                overlay_draw as *const c_void,
                c"v@:{CGRect={CGPoint=dd}{CGSize=dd}}".as_ptr(),
            );
            let _ = class_addMethod(
                cls,
                sel(c"hitTest:"),
                overlay_hit_test as *const c_void,
                c"@@:{CGPoint=dd}".as_ptr(),
            );
            let _ = class_addMethod(
                cls,
                sel(c"mouseDown:"),
                overlay_mouse_down as *const c_void,
                c"v@:@".as_ptr(),
            );
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{plan, Plan};

    /// The slot policy, stated: identical live route toggles OFF;
    /// otherwise the first OFF slot; a full board steals the last.
    #[test]
    fn the_slot_policy() {
        // (src, tgt) per slot; src 0 = OFF.
        let board = [(1, 2), (0, 0), (0, 0), (0, 0)];
        assert_eq!(
            plan(&board, 1, 2),
            Plan::Disconnect(0),
            "same cable toggles"
        );
        assert_eq!(plan(&board, 1, 1), Plan::Connect(1), "first free slot");
        assert_eq!(
            plan(&board, 2, 2),
            Plan::Connect(1),
            "different source, free slot"
        );
        let full = [(1, 0), (2, 1), (3, 2), (1, 1)];
        assert_eq!(
            plan(&full, 2, 0),
            Plan::Connect(3),
            "full board steals the last"
        );
        assert_eq!(plan(&full, 3, 2), Plan::Disconnect(2), "toggle beats steal");
    }
}
