//! The face runtime: one per open editor. It lays the tree out, bakes
//! the static layer once per pixel scale, paints the live layer over a
//! copy of it, decides when a repaint is due, and turns pointer input
//! into parameter edits. Platform hosts feed it events and blit its
//! canvas; everything here is portable and testable headless.

extern crate alloc;
use alloc::sync::Arc;
use alloc::vec::Vec;
use std::collections::HashMap;

use stave_raster::{Canvas, TextPainter};
use stave_ui::layout::Rect;
use stave_ui::scene::{Pt, Scene};
use stave_ui::{FixedMetrics, Kind, Layout};

use crate::material::Shadows;
use crate::part::{step_of, step_value, Bake, Bind, Control, FaceNode, Focus, Frame, Gesture, Host, Part, Sprites};
use crate::theme::Theme;

/// A plugin's face: a theme and a part tree.
#[derive(Clone)]
pub struct Face {
    pub theme: Theme,
    pub root: FaceNode,
}

impl Face {
    pub fn new(theme: Theme, root: FaceNode) -> Self {
        Face { theme, root }
    }
}

/// Keyboard modifiers that change a gesture.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    /// Fine adjustment (Shift).
    pub fine: bool,
    /// Reset to default (Option/Alt-click, as well as double-click).
    pub reset: bool,
}

/// The pointer shape a host should show.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Cursor {
    Arrow,
    /// Over a draggable control.
    Grab,
    /// While dragging.
    Grabbing,
    /// Over a clickable control.
    Point,
}

struct Item {
    part: Arc<dyn Part>,
    rect: Rect,
    /// Page conditions inherited from the tree: shown only while every
    /// (page set, page) holds.
    when: Vec<(u32, u16)>,
}

struct Ctl {
    item: usize,
    control: Control,
}

/// A gesture in flight.
struct Drag {
    ctl: usize,
    from: Pt,
    /// The value(s) at the grab, re-anchored when Shift toggles.
    start: f64,
    fine: bool,
    moved: bool,
}

pub struct Runtime {
    face: Face,
    size: (f64, f64),
    items: Vec<Item>,
    ctls: Vec<Ctl>,
    baked: Option<(u64, Canvas)>,
    sprites: Sprites,
    drag: Option<Drag>,
    hover: Option<usize>,
    memo: Vec<u64>,
    pages: HashMap<u32, u32>,
}

fn flatten(node: &FaceNode, layout: &Layout, when: &[(u32, u16)], out: &mut Vec<Item>) {
    let mut when = when.to_vec();
    if let Some(w) = node.when {
        when.push(w);
    }
    if let Kind::Widget(part) = &node.kind {
        out.push(Item {
            part: part.clone(),
            rect: layout.rect,
            when: when.clone(),
        });
    }
    for (child, cl) in node.children.iter().zip(&layout.children) {
        flatten(child, cl, &when, out);
    }
}

impl Runtime {
    pub fn new(face: Face) -> Self {
        // A face is as big as its content: measure with no space to
        // spare (fills and spacers take nothing), then lay out at that
        // size so they share whatever the widest row left over.
        let m = FixedMetrics::default();
        let natural = stave_ui::solve(&face.root, (0.0, 0.0), &m).rect;
        let size = (natural.w.ceil(), natural.h.ceil());
        let layout = stave_ui::solve(&face.root, size, &m);
        let mut items = Vec::new();
        flatten(&face.root, &layout, &[], &mut items);
        let mut ctls = Vec::new();
        for (i, it) in items.iter().enumerate() {
            for control in it.part.controls(it.rect) {
                ctls.push(Ctl { item: i, control });
            }
        }
        Runtime {
            face,
            size,
            items,
            ctls,
            baked: None,
            sprites: Sprites::default(),
            drag: None,
            hover: None,
            memo: Vec::new(),
            pages: HashMap::new(),
        }
    }

    /// Every part's solved rect, in paint order (for tests and tools).
    pub fn rects(&self) -> Vec<Rect> {
        self.items.iter().map(|it| it.rect).collect()
    }

    /// The face's natural size in points.
    pub fn size(&self) -> (f64, f64) {
        self.size
    }

    pub fn theme(&self) -> &Theme {
        &self.face.theme
    }

    /// Every probe any part reads (hosts resolve these up front).
    pub fn probes(&self) -> Vec<&'static str> {
        let mut v: Vec<&'static str> = self.items.iter().flat_map(|it| it.part.probes()).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// Every control's gesture (for binding checks).
    pub fn gestures(&self) -> Vec<Gesture> {
        self.ctls.iter().map(|c| c.control.gesture.clone()).collect()
    }

    /// Every parameter the face moves or reads.
    pub fn binds(&self) -> Vec<Bind> {
        let mut v: Vec<Bind> = self.ctls.iter().flat_map(|c| c.control.gesture.binds()).collect();
        v.extend(self.items.iter().flat_map(|it| it.part.reads()));
        v.sort_unstable_by_key(|b| b.id);
        v.dedup_by_key(|b| b.id);
        v
    }

    fn shown(&self, item: usize) -> bool {
        self.items[item]
            .when
            .iter()
            .all(|&(k, p)| self.pages.get(&k).copied().unwrap_or(0) == u32::from(p))
    }

    /// The page shown for page set `key`.
    pub fn page(&self, key: u32) -> u32 {
        self.pages.get(&key).copied().unwrap_or(0)
    }

    fn page_key(&self) -> u64 {
        let mut v: Vec<(u32, u32)> = self.pages.iter().map(|(k, p)| (*k, *p)).collect();
        v.sort_unstable();
        crate::part::sprite_key(v)
    }

    fn focus(&self, item: usize) -> Focus {
        let owns = |c: Option<usize>| c.is_some_and(|c| self.ctls[c].item == item);
        Focus {
            hover: owns(self.hover),
            active: owns(self.drag.as_ref().map(|d| d.ctl)),
        }
    }

    fn bake(&self, host: &dyn Host, scale: f64, text: &mut dyn TextPainter) -> Canvas {
        let mut occ = Vec::new();
        for (i, it) in self.items.iter().enumerate() {
            if self.shown(i) {
                it.part.occluders(it.rect, &self.face.theme, &mut occ);
            }
        }
        let shadows = Shadows::new(self.size.0, self.size.1, &occ);
        let mut scene = Scene::new();
        for (_, it) in self.items.iter().enumerate().filter(|(i, _)| self.shown(*i)) {
            it.part.bake(
                it.rect,
                &mut Bake {
                    theme: &self.face.theme,
                    scale,
                    face: self.size,
                    shadows: &shadows,
                    host,
                    scene: &mut scene,
                },
            );
        }
        let mut canvas = Canvas::new((self.size.0 * scale).round() as usize, (self.size.1 * scale).round() as usize);
        stave_raster::render(&scene, scale, &mut canvas, text);
        canvas
    }

    /// Paint the whole face at `scale` px/pt into `out`.
    pub fn render(&mut self, host: &dyn Host, scale: f64, time: f64, out: &mut Canvas, text: &mut dyn TextPainter) {
        let key = scale.to_bits() ^ self.page_key().rotate_left(17);
        if !self.baked.as_ref().is_some_and(|(k, _)| *k == key) {
            let c = self.bake(host, scale, text);
            self.baked = Some((key, c));
        }
        let base = &self.baked.as_ref().expect("baked above").1;
        out.w = base.w;
        out.h = base.h;
        out.px.clear();
        out.px.extend_from_slice(&base.px);
        let mut scene = Scene::new();
        for (i, it) in self.items.iter().enumerate() {
            if !self.shown(i) {
                continue;
            }
            let f = Frame {
                theme: &self.face.theme,
                host,
                scale,
                time,
                focus: self.focus(i),
                sprites: &self.sprites,
                pages: &self.pages,
            };
            it.part.paint(it.rect, &f, &mut scene);
        }
        stave_raster::render(&scene, scale, out, text);
        self.memo = self.observe(host, time);
    }

    fn observe(&self, host: &dyn Host, time: f64) -> Vec<u64> {
        let mut v: Vec<u64> = self.binds().iter().map(|b| host.normalized(b.id).to_bits()).collect();
        v.extend(self.probes().iter().map(|p| host.probe(p).to_bits()));
        v.push(self.hover.map_or(u64::MAX, |h| h as u64));
        v.push(self.page_key());
        v.push(host.preset().map_or(u64::MAX, |p| p as u64));
        v.push(self.drag.as_ref().map_or(u64::MAX, |d| d.ctl as u64));
        let animating = self.items.iter().enumerate().any(|(i, it)| {
            it.part.animating(&Frame {
                theme: &self.face.theme,
                host,
                scale: 1.0,
                time,
                focus: self.focus(i),
                sprites: &self.sprites,
                pages: &self.pages,
            })
        });
        if animating {
            v.push(time.to_bits());
        }
        v
    }

    /// True when something the face shows has changed since the last
    /// `render` (a value, a probe, the pointer, or an animation).
    pub fn needs_paint(&self, host: &dyn Host, time: f64) -> bool {
        self.baked.is_none() || self.observe(host, time) != self.memo
    }

    fn hit(&self, p: Pt) -> Option<usize> {
        // Topmost first: later parts paint over earlier ones.
        (0..self.ctls.len())
            .rev()
            .find(|&i| self.shown(self.ctls[i].item) && self.ctls[i].control.hit.contains(p))
    }

    /// The pointer moved with no button down. Returns true if the hover
    /// target changed.
    pub fn pointer_move(&mut self, p: Pt) -> bool {
        let h = self.hit(p);
        let changed = h != self.hover;
        self.hover = h;
        changed
    }

    /// The pointer left the face.
    pub fn pointer_exit(&mut self) {
        if self.drag.is_none() {
            self.hover = None;
        }
    }

    pub fn cursor(&self) -> Cursor {
        if self.drag.is_some() {
            return Cursor::Grabbing;
        }
        match self.hover.map(|c| &self.ctls[c].control.gesture) {
            Some(Gesture::Rotary { .. } | Gesture::Stepped { .. } | Gesture::Pad { .. }) => Cursor::Grab,
            Some(_) => Cursor::Point,
            None => Cursor::Arrow,
        }
    }

    /// Button down at `p`; `clicks` is the click count (2 = double).
    pub fn pointer_down(&mut self, host: &mut dyn Host, p: Pt, mods: Mods, clicks: u32) -> bool {
        let Some(ci) = self.hit(p) else {
            return false;
        };
        self.hover = Some(ci);
        let gesture = self.ctls[ci].control.gesture.clone();
        match gesture {
            Gesture::Rotary { bind, .. } | Gesture::Stepped { bind, .. } if clicks >= 2 || mods.reset => {
                let d = host.default_normalized(bind.id);
                host.begin(bind.id);
                host.set(bind.id, d);
                host.end(bind.id);
            }
            Gesture::Rotary { bind, .. } | Gesture::Stepped { bind, .. } => {
                host.begin(bind.id);
                self.drag = Some(Drag {
                    ctl: ci,
                    from: p,
                    start: host.normalized(bind.id),
                    fine: mods.fine,
                    moved: false,
                });
            }
            Gesture::Choose { bind, step } => {
                let n = host.steps(bind.id);
                host.begin(bind.id);
                host.set(bind.id, step_value(step, n));
                host.end(bind.id);
            }
            Gesture::Latch { bind } => {
                let on = host.normalized(bind.id) >= 0.5;
                host.begin(bind.id);
                host.set(bind.id, if on { 0.0 } else { 1.0 });
                host.end(bind.id);
            }
            Gesture::Momentary { bind } => {
                host.begin(bind.id);
                host.set(bind.id, 1.0);
                self.drag = Some(Drag {
                    ctl: ci,
                    from: p,
                    start: 0.0,
                    fine: false,
                    moved: false,
                });
            }
            Gesture::Nudge { bind, delta } => {
                let n = host.steps(bind.id).max(2);
                let s = i64::from(step_of(host.normalized(bind.id), n)) + i64::from(delta);
                host.begin(bind.id);
                host.set(bind.id, step_value(s.clamp(0, i64::from(n) - 1) as u32, n));
                host.end(bind.id);
            }
            Gesture::Preset { delta } => {
                let n = host.presets().len() as i64;
                if n > 0 {
                    let at = host.preset().map_or(if delta > 0 { -1 } else { 0 }, |p| p as i64);
                    host.apply_preset((at + i64::from(delta)).rem_euclid(n) as usize);
                }
            }
            Gesture::Page { key, page } => {
                self.pages.insert(key, page);
            }
            Gesture::Pad { x, y, area } => {
                host.begin(x.id);
                host.begin(y.id);
                self.drag = Some(Drag {
                    ctl: ci,
                    from: p,
                    start: 0.0,
                    fine: mods.fine,
                    moved: false,
                });
                set_pad(host, x, y, area, p);
            }
        }
        true
    }

    pub fn pointer_drag(&mut self, host: &mut dyn Host, p: Pt, mods: Mods) {
        let Some(d) = self.drag.as_mut() else {
            return;
        };
        let gesture = self.ctls[d.ctl].control.gesture.clone();
        match gesture {
            Gesture::Rotary { bind, travel } => {
                if mods.fine != d.fine {
                    // Re-anchor so switching precision never jumps.
                    d.start = host.normalized(bind.id);
                    d.from = p;
                    d.fine = mods.fine;
                }
                let along = (d.from.y - p.y) + 0.5 * (p.x - d.from.x);
                let k = if d.fine { 0.1 } else { 1.0 };
                d.moved |= along.abs() > 0.5;
                host.set(bind.id, (d.start + along / travel * k).clamp(0.0, 1.0));
            }
            Gesture::Stepped { bind, pitch } => {
                let n = host.steps(bind.id).max(2);
                let along = (d.from.y - p.y) + (p.x - d.from.x);
                let steps = (along / pitch).trunc() as i64;
                if steps != 0 {
                    d.moved = true;
                }
                let s0 = i64::from(step_of(d.start, n));
                let s = (s0 + steps).clamp(0, i64::from(n) - 1) as u32;
                host.set(bind.id, step_value(s, n));
            }
            Gesture::Pad { x, y, area } => {
                d.moved = true;
                set_pad(host, x, y, area, p);
            }
            _ => {}
        }
    }

    pub fn pointer_up(&mut self, host: &mut dyn Host) {
        let Some(d) = self.drag.take() else {
            return;
        };
        match self.ctls[d.ctl].control.gesture.clone() {
            Gesture::Rotary { bind, .. } => host.end(bind.id),
            Gesture::Stepped { bind, .. } => {
                if !d.moved {
                    // A click without a drag steps forward, wrapping.
                    let n = host.steps(bind.id).max(2);
                    let s = (step_of(d.start, n) + 1) % n;
                    host.set(bind.id, step_value(s, n));
                }
                host.end(bind.id);
            }
            Gesture::Momentary { bind } => {
                host.set(bind.id, 0.0);
                host.end(bind.id);
            }
            Gesture::Pad { x, y, .. } => {
                host.end(x.id);
                host.end(y.id);
            }
            _ => {}
        }
    }

    /// Scroll over the face: `dy` in lines (positive = up / increase).
    pub fn wheel(&mut self, host: &mut dyn Host, p: Pt, dy: f64, mods: Mods) {
        if self.drag.is_some() {
            return;
        }
        let Some(ci) = self.hit(p) else {
            return;
        };
        match self.ctls[ci].control.gesture.clone() {
            Gesture::Rotary { bind, .. } => {
                let k = if mods.fine { 0.002 } else { 0.01 };
                let v = (host.normalized(bind.id) + dy * k).clamp(0.0, 1.0);
                host.begin(bind.id);
                host.set(bind.id, v);
                host.end(bind.id);
            }
            Gesture::Stepped { bind, .. } => {
                let n = host.steps(bind.id).max(2);
                let s = i64::from(step_of(host.normalized(bind.id), n)) + if dy > 0.0 { 1 } else { -1 };
                let s = s.clamp(0, i64::from(n) - 1) as u32;
                host.begin(bind.id);
                host.set(bind.id, step_value(s, n));
                host.end(bind.id);
            }
            _ => {}
        }
    }
}

fn set_pad(host: &mut dyn Host, x: Bind, y: Bind, area: Rect, p: Pt) {
    let nx = ((p.x - area.x) / area.w).clamp(0.0, 1.0);
    let ny = (1.0 - (p.y - area.y) / area.h).clamp(0.0, 1.0);
    host.set(x.id, nx);
    host.set(y.id, ny);
}
