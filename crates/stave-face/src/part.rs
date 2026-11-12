//! The part contract. A face is a layout tree whose leaves are parts;
//! the runtime lays the tree out, then asks each part, in its solved
//! rect, for three things:
//!
//! * `bake` — the STATIC picture (enclosure, hardware, engraving),
//!   rendered once per size and pixel scale and cached;
//! * `paint` — the LIVE picture over it (pointers, lamps, readouts,
//!   displays), every frame something it reads has changed;
//! * `grabs` — where it can be grabbed and what a grab does (from live
//!   values when a target moves). The runtime owns the gesture: drag,
//!   fine-drag, double-click reset, scroll, stepping, latching.
//!
//! Each part declares once what it reads ([`Reads`]: params and taps),
//! so the runtime knows when to repaint. Parts bind parameters by
//! [`Bind`] (a ParamId and its name) and read taps by [`TapId`], both
//! through the plugin's [`Controller`]; they never see the plugin.

use std::sync::Arc;
use std::vec::Vec;
use core::cell::RefCell;
use std::collections::HashMap;

use stave_core::params::{AnyParam, ParamId};
use stave_core::telemetry::TapId;
use stave_core::Controller;
use stave_ui::layout::Rect;
use stave_ui::scene::{Image, Op, Pt, Scene};
use stave_ui::Fonts;

use crate::material::Occluder;
use crate::theme::Theme;

/// A parameter a part reads or moves: its ParamId and display name.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Bind {
    pub id: ParamId,
    pub name: &'static str,
}

impl Bind {
    pub const fn new(id: ParamId, name: &'static str) -> Self {
        Bind { id, name }
    }
}

/// A parameter as a face binds it: `p.drive.bind()`.
pub trait BindParam {
    fn bind(&self) -> Bind;
}

impl<T: AnyParam + ?Sized> BindParam for T {
    fn bind(&self) -> Bind {
        let m = self.meta();
        Bind::new(m.id, m.name)
    }
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
    /// The pointer is over one of the part's grabs.
    pub hover: bool,
    /// One of the part's grabs is being dragged.
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
    /// The plugin's parameters, taps, capture and presets.
    pub controller: &'a dyn Controller,
    /// Device pixels per point.
    pub scale: f64,
    /// Seconds since the face opened (for animation).
    pub time: f64,
    pub focus: Focus,
    pub sprites: &'a Sprites,
    /// Editor-local page state (see `Pager`).
    pub pages: &'a HashMap<u32, u32>,
    /// The parameters the part declared it reads ([`Part::reads`]), in
    /// order — so a draw fn never hard-codes ids.
    pub binds: &'a [Bind],
}

impl Frame<'_> {
    /// The `i`th declared bind's normalized value (see `binds`).
    pub fn bound(&self, i: usize) -> f64 {
        self.binds.get(i).map_or(0.0, |b| self.controller.normalized(b.id))
    }
    /// The `i`th declared bind's parameter: its range, text and steps
    /// (`denormalize` turns a normalized position into its value).
    pub fn param(&self, i: usize) -> Option<&dyn AnyParam> {
        self.controller.param(self.binds.get(i)?.id)
    }
    /// The page shown for page set `key`.
    pub fn page(&self, key: u32) -> u32 {
        self.pages.get(&key).copied().unwrap_or(0)
    }
    /// `b`'s normalized value.
    pub fn value(&self, b: Bind) -> f64 {
        self.controller.normalized(b.id)
    }
    /// The latest value of tap `id`.
    pub fn tap(&self, id: TapId) -> f64 {
        self.controller.tap(id)
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
    pub controller: &'a dyn Controller,
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

/// Where a part can be grabbed.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Area {
    Circle { c: Pt, r: f64 },
    Rect(Rect),
}

impl Area {
    pub fn contains(&self, p: Pt) -> bool {
        match *self {
            Area::Circle { c, r } => (p.x - c.x).hypot(p.y - c.y) <= r,
            Area::Rect(r) => p.x >= r.x && p.x <= r.x + r.w && p.y >= r.y && p.y <= r.y + r.h,
        }
    }

    /// The point a pointer aims at to grab it.
    pub fn centre(&self) -> Pt {
        match *self {
            Area::Circle { c, .. } => c,
            Area::Rect(r) => Pt {
                x: r.x + r.w / 2.0,
                y: r.y + r.h / 2.0,
            },
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
            | Gesture::Momentary { bind } => std::vec![*bind],
            Gesture::Pad { x, y, .. } => std::vec![*x, *y],
            Gesture::Nudge { bind, .. } => std::vec![*bind],
            Gesture::Preset { .. } | Gesture::Page { .. } => Vec::new(),
        }
    }
}

/// One place a part can be grabbed, and what grabbing it does.
#[derive(Clone, Debug, PartialEq)]
pub struct Grab {
    pub area: Area,
    pub gesture: Gesture,
}

/// What a part's `paint` reads, declared once: the runtime repaints when
/// any of it moves, and hands `params` to the draw code in order
/// ([`Frame::binds`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reads {
    pub params: Vec<Bind>,
    pub taps: Vec<TapId>,
}

impl Reads {
    /// Reads nothing live (enclosures, print).
    pub const NONE: Reads = Reads {
        params: Vec::new(),
        taps: Vec::new(),
    };

    /// Reads these parameters.
    pub fn params(params: &[Bind]) -> Self {
        Reads {
            params: params.to_vec(),
            taps: Vec::new(),
        }
    }

    /// Reads these taps.
    pub fn taps(taps: &[TapId]) -> Self {
        Reads {
            params: Vec::new(),
            taps: taps.to_vec(),
        }
    }

    /// And these taps.
    pub fn and_taps(mut self, taps: &[TapId]) -> Self {
        self.taps.extend_from_slice(taps);
        self
    }
}

/// A part's stable name, so tests and tools address it by name rather
/// than coordinates: its bound parameter, or a name given with
/// `node.id("…")` (or chosen by the part, like a title's "title").
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PartId {
    Param(ParamId),
    Name(&'static str),
}

/// One leaf of a face.
pub trait Part: Send + Sync + 'static {
    /// The part's natural size in points, from the theme's spacing and
    /// type scale and its text as `fonts` sets it. Layout asks for it
    /// where the node leaves a size `Auto` ([`measured`]); a fixed-size
    /// [`leaf`] never asks.
    fn size(&self, _theme: &Theme, _fonts: &dyn Fonts) -> (f64, f64) {
        (0.0, 0.0)
    }
    /// Raised shapes that shade the enclosure around them.
    fn occluders(&self, _r: Rect, _theme: &Theme, _out: &mut Vec<Occluder>) {}
    /// The static picture, in face coordinates within `r`.
    fn bake(&self, _r: Rect, _cx: &mut Bake<'_>) {}
    /// The live picture.
    fn paint(&self, _r: Rect, _f: &Frame<'_>, _s: &mut Scene) {}
    /// Where the part can be grabbed, topmost last. May read live values
    /// (a handle that moves with its parameter, a step count).
    fn grabs(&self, _r: Rect, _f: &Frame<'_>, _out: &mut Vec<Grab>) {}
    /// The parameters and taps `paint` reads, declared once.
    fn reads(&self) -> Reads {
        Reads::NONE
    }
    /// True while the part animates with time alone (a spinning
    /// record): the runtime keeps repainting.
    fn animating(&self, _f: &Frame<'_>) -> bool {
        false
    }
    /// This part's name when its node has none: by default its first
    /// read parameter.
    fn id(&self) -> Option<PartId> {
        self.reads().params.first().map(|b| PartId::Param(b.id))
    }
}

/// A face tree: layout nodes with parts at the leaves.
pub type FaceNode = stave_ui::Node<Arc<dyn Part>>;

/// Wrap a part as a fixed-size leaf.
pub fn leaf(part: impl Part, w: f64, h: f64) -> FaceNode {
    stave_ui::widget(Arc::new(part) as Arc<dyn Part>).size(w, h)
}

/// Wrap a part as a leaf that sizes itself ([`Part::size`]): measured
/// against the face's theme and the platform's fonts when it is laid out.
pub fn measured(part: impl Part) -> FaceNode {
    stave_ui::widget(Arc::new(part) as Arc<dyn Part>)
}

/// Show `node` only while page set `key` shows `page` (see `Pager`).
/// Page set keys are below `0x8000_0000` (above is [`on_step`]'s).
pub fn on_page(node: FaceNode, key: u32, page: u32) -> FaceNode {
    debug_assert!(key < STEP_KEY, "page set keys are below 0x8000_0000");
    node.when_key(key, page as u16)
}

/// The visibility keys at and above this are a stepped parameter's id
/// (top bit set): [`on_step`].
pub(crate) const STEP_KEY: u32 = 0x8000_0000;

/// Show `node` only while the stepped parameter `bind` is on step `step`
/// (an algorithm's controls under its selector).
pub fn on_step(node: FaceNode, bind: Bind, step: u32) -> FaceNode {
    node.when_key(STEP_KEY | bind.id.0, step as u16)
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
