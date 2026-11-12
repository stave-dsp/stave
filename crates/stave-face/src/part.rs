//! The part contract. A face is a `ui!` tree whose leaves are parts;
//! the runtime lays the tree out, then asks each part, in its solved
//! rect, for three things:
//!
//! * `bake` — the STATIC picture (enclosure, hardware, engraving),
//!   rendered once per size and pixel scale and cached;
//! * `paint` — the LIVE picture over it (pointers, lamps, readouts,
//!   displays), every frame something it reads has changed;
//! * `controls` — where it can be grabbed and what a grab does. The
//!   runtime owns the gesture: drag, fine-drag, double-click reset,
//!   scroll, stepping, latching.
//!
//! Parts bind parameters by [`Bind`] (a ParamId and its name) and read
//! telemetry by probe name; they never see the plugin.

extern crate alloc;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cell::RefCell;
use std::collections::HashMap;

use stave_ui::layout::Rect;
use stave_ui::scene::{Image, Op, Pt, Scene};

use crate::material::Occluder;
use crate::theme::Theme;

/// A parameter a part reads or moves: its ParamId and display name.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Bind {
    pub id: u32,
    pub name: &'static str,
}

impl Bind {
    pub const fn new(id: u32, name: &'static str) -> Self {
        Bind { id, name }
    }
}

/// What a face reads and writes, supplied by the platform host over the
/// plugin's parameter table and debug bus. Values are NORMALIZED (0–1,
/// the parameter's own taper); stepped parameters land on `i / (n−1)`.
pub trait Host {
    fn normalized(&self, id: u32) -> f64;
    fn default_normalized(&self, id: u32) -> f64;
    /// Number of discrete steps, 0 for a continuous parameter.
    fn steps(&self, id: u32) -> u32;
    fn step_name(&self, id: u32, index: u32) -> Option<String>;
    /// The current value as the plugin formats it ("1.20 s", "VINYL").
    fn value_text(&self, id: u32) -> String;
    /// The latest value of a named probe (0 if unknown).
    fn probe(&self, name: &str) -> f64;
    /// Open, move and close an edit gesture (the host's automation bracket).
    fn begin(&mut self, id: u32);
    fn set(&mut self, id: u32, normalized: f64);
    fn end(&mut self, id: u32);

    /// The latest captured audio of `channel` into `out`; returns how
    /// many frames were written (0 when nothing is captured).
    fn capture(&self, _channel: usize, _out: &mut [f32]) -> usize {
        0
    }
    /// The plugin's published transfer curve (output values over the
    /// input span), empty when it publishes none.
    fn curve(&self) -> Vec<f32> {
        Vec::new()
    }
    /// Factory preset names.
    fn presets(&self) -> Vec<String> {
        Vec::new()
    }
    /// The preset last applied, if any.
    fn preset(&self) -> Option<usize> {
        None
    }
    /// Apply factory preset `index` (every value through the gesture path).
    fn apply_preset(&mut self, _index: usize) {}
}

/// The step a normalized value sits on, for an `n`-step parameter.
pub fn step_of(normalized: f64, n: u32) -> u32 {
    if n <= 1 {
        return 0;
    }
    (normalized.clamp(0.0, 1.0) * f64::from(n - 1)).round() as u32
}

/// The normalized value of step `i` of `n`.
pub fn step_value(i: u32, n: u32) -> f64 {
    if n <= 1 {
        0.0
    } else {
        f64::from(i.min(n - 1)) / f64::from(n - 1)
    }
}

/// How the pointer relates to a part this frame.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Focus {
    /// The pointer is over one of the part's controls.
    pub hover: bool,
    /// One of the part's controls is being dragged.
    pub active: bool,
}

/// Bitmaps a part draws live (lamp glows, lever positions), made once per
/// key and pixel scale and kept for the face's life.
#[derive(Default)]
pub struct Sprites {
    cache: RefCell<HashMap<(u64, u64), Image>>,
}

impl Sprites {
    /// The sprite for `key` at `scale`, made by `make` on first use.
    pub fn get(&self, key: u64, scale: f64, make: impl FnOnce() -> Image) -> Image {
        let k = (key, scale.to_bits());
        if let Some(img) = self.cache.borrow().get(&k) {
            return img.clone();
        }
        let img = make();
        self.cache.borrow_mut().insert(k, img.clone());
        img
    }
}

/// A stable key for a sprite from any hashable description.
pub fn sprite_key(parts: impl core::hash::Hash) -> u64 {
    use core::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    parts.hash(&mut h);
    h.finish()
}

/// Everything a part's live `paint` may read.
pub struct Frame<'a> {
    pub theme: &'a Theme,
    pub host: &'a dyn Host,
    /// Device pixels per point.
    pub scale: f64,
    /// Seconds since the face opened (for animation).
    pub time: f64,
    pub focus: Focus,
    pub sprites: &'a Sprites,
    /// Editor-local page state (see `Pager`).
    pub pages: &'a HashMap<u32, u32>,
    /// The parameters a display declared it reads (`Display::reads`),
    /// in order — so a draw fn never hard-codes ids.
    pub binds: &'a [Bind],
}

impl Frame<'_> {
    /// The `i`th declared bind's normalized value (see `binds`).
    pub fn bound(&self, i: usize) -> f64 {
        self.binds.get(i).map_or(0.0, |b| self.host.normalized(b.id))
    }
    /// The page shown for page set `key`.
    pub fn page(&self, key: u32) -> u32 {
        self.pages.get(&key).copied().unwrap_or(0)
    }
    pub fn value(&self, b: Bind) -> f64 {
        self.host.normalized(b.id)
    }
    pub fn probe(&self, name: &str) -> f64 {
        self.host.probe(name)
    }
    /// Draw a cached sprite centred on `c`, `size` points square.
    pub fn sprite(&self, s: &mut Scene, c: Pt, size: f64, key: u64, make: impl FnOnce(f64) -> Image) {
        let img = self.sprites.get(key, self.scale, || make(self.scale));
        s.ops.push(Op::Image {
            at: Pt { x: c.x - size / 2.0, y: c.y - size / 2.0 },
            w: size,
            h: size,
            image: img,
        });
    }
}

/// Everything a part's static `bake` may use. Vector ops and images go
/// into `scene`, painted in tree order over the enclosure.
pub struct Bake<'a> {
    pub theme: &'a Theme,
    pub scale: f64,
    /// The whole face's size in points.
    pub face: (f64, f64),
    /// Every occluder on the face, for parts that shade the surface.
    pub shadows: &'a crate::material::Shadows,
    /// For static facts only (option names); never live values.
    pub host: &'a dyn Host,
    pub scene: &'a mut Scene,
}

impl Bake<'_> {
    pub fn image(&mut self, x: f64, y: f64, w: f64, h: f64, image: Image) {
        self.scene.ops.push(Op::Image {
            at: Pt { x, y },
            w,
            h,
            image,
        });
    }
}

/// Where a control can be grabbed.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Hit {
    Circle { c: Pt, r: f64 },
    Rect(Rect),
}

impl Hit {
    pub fn contains(&self, p: Pt) -> bool {
        match *self {
            Hit::Circle { c, r } => (p.x - c.x).hypot(p.y - c.y) <= r,
            Hit::Rect(r) => p.x >= r.x && p.x <= r.x + r.w && p.y >= r.y && p.y <= r.y + r.h,
        }
    }
}

/// What a grab does. The runtime implements each one the same way on
/// every face.
#[derive(Clone, Debug, PartialEq)]
pub enum Gesture {
    /// Continuous: vertical (and horizontal) drag across `travel`
    /// points sweeps the whole range; Shift is ten times finer;
    /// double-click resets; the wheel nudges.
    Rotary { bind: Bind, travel: f64 },
    /// Discrete: a click steps forward (wrapping), a drag steps every
    /// `pitch` points, the wheel steps.
    Stepped { bind: Bind, pitch: f64 },
    /// Click to set this exact step.
    Choose { bind: Bind, step: u32 },
    /// Click flips between 0 and 1.
    Latch { bind: Bind },
    /// 1 while held, 0 on release.
    Momentary { bind: Bind },
    /// Two parameters on a plane: x left→right, y bottom→top, over `area`.
    Pad { x: Bind, y: Bind, area: Rect },
    /// Step a discrete parameter by `delta`, clamped (◀ ▶ buttons).
    Nudge { bind: Bind, delta: i32 },
    /// Step through the factory presets by `delta`, wrapping.
    Preset { delta: i32 },
    /// Show page `page` of the editor-local page set `key`.
    Page { key: u32, page: u32 },
}

impl Gesture {
    pub fn binds(&self) -> Vec<Bind> {
        match self {
            Gesture::Rotary { bind, .. }
            | Gesture::Stepped { bind, .. }
            | Gesture::Choose { bind, .. }
            | Gesture::Latch { bind }
            | Gesture::Momentary { bind } => alloc::vec![*bind],
            Gesture::Pad { x, y, .. } => alloc::vec![*x, *y],
            Gesture::Nudge { bind, .. } => alloc::vec![*bind],
            Gesture::Preset { .. } | Gesture::Page { .. } => Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Control {
    pub hit: Hit,
    pub gesture: Gesture,
}

/// One leaf of a face.
pub trait Part: Send + Sync + 'static {
    /// Raised shapes that shade the enclosure around them.
    fn occluders(&self, _r: Rect, _theme: &Theme, _out: &mut Vec<Occluder>) {}
    /// The static picture, in face coordinates within `r`.
    fn bake(&self, _r: Rect, _cx: &mut Bake<'_>) {}
    /// The live picture.
    fn paint(&self, _r: Rect, _f: &Frame<'_>, _s: &mut Scene) {}
    /// Grab regions and their gestures.
    fn controls(&self, _r: Rect) -> Vec<Control> {
        Vec::new()
    }
    /// A grab target placed from LIVE values (a node that moves, a
    /// handle on a curve): the gesture a press at `p` makes, if any.
    /// Tried before the fixed `controls`.
    fn hit(&self, _r: Rect, _f: &Frame<'_>, _p: Pt) -> Option<Gesture> {
        None
    }
    /// Probes `paint` reads (so the host can resolve them up front).
    fn probes(&self) -> Vec<&'static str> {
        Vec::new()
    }
    /// True while the part animates with time alone (a spinning
    /// record): the runtime keeps repainting.
    fn animating(&self, _f: &Frame<'_>) -> bool {
        false
    }
    /// The parameters `paint` reads, beyond those its controls move.
    fn reads(&self) -> Vec<Bind> {
        Vec::new()
    }
}

/// A face tree: `ui!` layout nodes with parts at the leaves.
pub type FaceNode = stave_ui::Node<Arc<dyn Part>>;

/// Wrap a part as a fixed-size leaf.
pub fn leaf(part: impl Part, w: f64, h: f64) -> FaceNode {
    stave_ui::widget(Arc::new(part) as Arc<dyn Part>).size(w, h)
}

/// Show `node` only while page set `key` shows `page` (see `Pager`).
pub fn on_page(node: FaceNode, key: u32, page: u32) -> FaceNode {
    node.when_key(key, page as u16)
}

/// The pages of page set `key`, overlaid, each stretched to the size of
/// the largest (so flipping pages never moves the rest of the face).
pub fn pages(key: u32, nodes: Vec<FaceNode>) -> FaceNode {
    let kids = nodes
        .into_iter()
        .enumerate()
        .map(|(i, n)| on_page(n.width(stave_ui::Dim::Fill(1.0)).height(stave_ui::Dim::Fill(1.0)), key, i as u32))
        .collect();
    stave_ui::stack(kids)
}
