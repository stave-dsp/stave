//! [`FaceDriver`]: see the `face` module docs.

use std::sync::{Arc, Mutex};

use stave::controller::{GestureSink, PluginController};
use stave::params::{AnyParam, IndexedParams, ParamId, Params};
use stave::{Controller, Plugin, Preset, Telemetry};
use stave_face::{Act, Cursor, Face, Gesture, Grab, Key, Layer, Mods, PartId, Runtime, Trace, Visibility};
use stave_raster::{BoxText, Canvas};
use stave_ui::layout::Rect;
use stave_ui::scene::{pt, Op, Pt};

/// One edit the face sent the host, in order.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Edit {
    Begin(ParamId),
    /// The new value, in the param's own units.
    Perform(ParamId, f64),
    End(ParamId),
}

/// The sink a driven face's controller tells: every edit, logged.
#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<Edit>>>);

impl Log {
    fn push(&self, e: Edit) {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).push(e);
    }
    fn all(&self) -> Vec<Edit> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
    fn take(&self) -> Vec<Edit> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(|p| p.into_inner()))
    }
}

impl GestureSink for Log {
    fn begin(&self, id: ParamId) {
        self.push(Edit::Begin(id));
    }
    fn perform(&self, id: ParamId, value: f64) {
        self.push(Edit::Perform(id, value));
    }
    fn end(&self, id: ParamId) {
        self.push(Edit::End(id));
    }
}

/// Where a test aims: a part by name (`"tilt"`, a node id, a role like
/// `"title"`), a [`PartId`], a [`ParamId`] (the part bound to it), or an
/// exact point (`pt(x, y)` or `(x, y)`, in points, y down).
pub trait Aim {
    /// The point to aim at on `ui`'s face (a named part: the centre of
    /// its first grab, else of its rect).
    fn point(&self, ui: &FaceDriver) -> Pt;
}

impl Aim for Pt {
    fn point(&self, _: &FaceDriver) -> Pt {
        *self
    }
}

impl Aim for (f64, f64) {
    fn point(&self, _: &FaceDriver) -> Pt {
        pt(self.0, self.1)
    }
}

impl Aim for PartId {
    fn point(&self, ui: &FaceDriver) -> Pt {
        let grabs = ui.runtime.grabs(&ui.controller, *self);
        if let Some(g) = grabs.first() {
            return g.area.centre();
        }
        let r = ui.runtime.rect(*self).unwrap_or_else(|| panic!("no part {self:?} on this face; it has {}", ui.part_names()));
        pt(r.x + r.w / 2.0, r.y + r.h / 2.0)
    }
}

impl Aim for ParamId {
    fn point(&self, ui: &FaceDriver) -> Pt {
        PartId::Param(*self).point(ui)
    }
}

impl Aim for &str {
    fn point(&self, ui: &FaceDriver) -> Pt {
        ui.part(self).point(ui)
    }
}

/// Where a drag ends: anywhere a test can [`Aim`], or an offset from
/// where it started ([`dx`], [`dy`], [`by`]).
pub trait Toward {
    fn end(&self, ui: &FaceDriver, from: Pt) -> Pt;
}

impl<A: Aim> Toward for A {
    fn end(&self, ui: &FaceDriver, _from: Pt) -> Pt {
        self.point(ui)
    }
}

/// A move relative to the drag's start, in points (y down: a negative
/// `dy` is up).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Offset {
    pub dx: f64,
    pub dy: f64,
}

impl Toward for Offset {
    fn end(&self, _: &FaceDriver, from: Pt) -> Pt {
        pt(from.x + self.dx, from.y + self.dy)
    }
}

/// `dy` points down (negative: up).
pub fn dy(dy: f64) -> Offset {
    Offset { dx: 0.0, dy }
}

/// `dx` points right.
pub fn dx(dx: f64) -> Offset {
    Offset { dx, dy: 0.0 }
}

/// `dx` right and `dy` down.
pub fn by(dx: f64, dy: f64) -> Offset {
    Offset { dx, dy }
}

/// A parameter a test reads or sets: by [`ParamId`] or by name (any case).
pub trait ParamRef {
    fn id(&self, ui: &FaceDriver) -> ParamId;
}

impl ParamRef for ParamId {
    fn id(&self, _: &FaceDriver) -> ParamId {
        *self
    }
}

impl ParamRef for &str {
    fn id(&self, ui: &FaceDriver) -> ParamId {
        (0..ui.table.count())
            .filter_map(|i| ui.table.by_index(i))
            .find(|p| p.meta().name.eq_ignore_ascii_case(self))
            .unwrap_or_else(|| panic!("no parameter named {self:?}"))
            .meta()
            .id
    }
}

/// One line of text the face shows, with the part that drew it.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub part: Option<PartId>,
    pub text: String,
    /// The line's box (x, y, w, h) by [`BoxText`] metrics.
    pub bounds: Rect,
    /// Drawn live (a readout, a value) rather than baked (a legend).
    pub live: bool,
}

/// Steps a drag moves through between its ends (a real pointer reports
/// many positions; the face must not depend on how many).
const DRAG_STEPS: usize = 8;

/// A face under test, as a person uses it (see [`crate::face`]).
pub struct FaceDriver {
    runtime: Runtime,
    controller: PluginController,
    table: Arc<dyn Params>,
    log: Log,
    trace: Trace,
    pointer: Option<Pt>,
    pressed: bool,
    time: f64,
}

impl FaceDriver {
    /// The face of `P` over a fresh instance of its params and presets.
    ///
    /// # Panics
    /// If `P` declares no face.
    pub fn of<P: Plugin>() -> Self {
        Self::with_params::<P>(P::Params::default())
    }

    /// The face of `P` over `params` (set up the state to start from).
    pub fn with_params<P: Plugin>(params: P::Params) -> Self {
        let params = Arc::new(params);
        let face = P::face(&params).unwrap_or_else(|| panic!("{} declares no face", P::INFO.name));
        let presets = P::presets(&params);
        Self::new(face, Arc::new(IndexedParams::new(params)), presets)
    }

    /// Any face, over a parameter table and its presets.
    pub fn new(face: Face, table: Arc<dyn Params>, presets: Vec<Preset>) -> Self {
        let log = Log::default();
        let controller = PluginController::new(table.clone(), Telemetry::new(), presets.into(), Box::new(log.clone()));
        FaceDriver {
            runtime: Runtime::new(face),
            controller,
            table,
            log,
            trace: Trace::default(),
            pointer: None,
            pressed: false,
            time: 0.0,
        }
    }

    // ------------------------------------------------------------ acts

    /// Send one event, recording it in the [`trace`](Self::trace).
    pub fn act(&mut self, act: Act) -> bool {
        match act {
            Act::Hover(p) | Act::Press { at: p, .. } | Act::Move { at: p, .. } | Act::Wheel { at: p, .. } => {
                self.pointer = Some(p)
            }
            Act::Exit => self.pointer = None,
            _ => {}
        }
        match act {
            Act::Press { .. } => self.pressed = true,
            Act::Release => self.pressed = false,
            _ => {}
        }
        self.trace.0.push(act);
        self.runtime.apply(&self.controller, &act)
    }

    /// Replay a recorded session.
    pub fn replay(&mut self, trace: &Trace) -> &mut Self {
        for act in &trace.0 {
            self.act(*act);
        }
        self
    }

    /// Move the pointer to `at` (a drag, if the button is down).
    pub fn hover(&mut self, at: impl Aim) -> &mut Self {
        let p = at.point(self);
        if self.pressed {
            self.act(Act::Move { at: p, mods: Mods::default() });
        } else {
            self.act(Act::Hover(p));
        }
        self
    }

    /// The pointer leaves the face.
    pub fn exit(&mut self) -> &mut Self {
        self.act(Act::Exit);
        self
    }

    /// Press the button at `at` (and hold it).
    pub fn press(&mut self, at: impl Aim) -> &mut Self {
        self.press_with(at, Mods::default(), 1)
    }

    /// Press at `at` with modifiers and a click count.
    pub fn press_with(&mut self, at: impl Aim, mods: Mods, clicks: u32) -> &mut Self {
        let p = at.point(self);
        if self.pointer != Some(p) && !self.pressed {
            self.act(Act::Hover(p));
        }
        self.act(Act::Press { at: p, mods, clicks });
        self
    }

    /// Release the button where it is.
    pub fn release(&mut self) -> &mut Self {
        self.act(Act::Release);
        if let Some(p) = self.pointer {
            self.act(Act::Hover(p));
        }
        self
    }

    /// Press and release at `at`.
    pub fn click(&mut self, at: impl Aim) -> &mut Self {
        self.click_with(at, Mods::default())
    }

    /// Click with modifiers (`Mods { reset: true, .. }` is Option-click).
    pub fn click_with(&mut self, at: impl Aim, mods: Mods) -> &mut Self {
        let p = at.point(self);
        self.press_with(p, mods, 1).release()
    }

    /// Two clicks, as the platform reports them: the second with a
    /// click count of 2.
    pub fn double_click(&mut self, at: impl Aim) -> &mut Self {
        let p = at.point(self);
        self.press_with(p, Mods::default(), 1).release();
        self.press_with(p, Mods::default(), 2).release()
    }

    /// Press at `from`, move to `to` through intermediate points, release.
    pub fn drag(&mut self, from: impl Aim, to: impl Toward) -> &mut Self {
        self.drag_with(from, to, Mods::default())
    }

    /// [`drag`](Self::drag) with modifiers held (`Mods { fine: true, .. }`
    /// is Shift).
    pub fn drag_with(&mut self, from: impl Aim, to: impl Toward, mods: Mods) -> &mut Self {
        let a = from.point(self);
        let b = to.end(self, a);
        self.press_with(a, mods, 1);
        self.move_to_with(b, mods);
        self.release()
    }

    /// With the button down, move to `to` through intermediate points.
    pub fn move_to(&mut self, to: impl Toward) -> &mut Self {
        self.move_to_with(to, Mods::default())
    }

    /// [`move_to`](Self::move_to) with modifiers held.
    pub fn move_to_with(&mut self, to: impl Toward, mods: Mods) -> &mut Self {
        let a = self.pointer.unwrap_or(pt(0.0, 0.0));
        let b = to.end(self, a);
        for i in 1..=DRAG_STEPS {
            let t = i as f64 / DRAG_STEPS as f64;
            self.act(Act::Move { at: pt(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t), mods });
        }
        self
    }

    /// Scroll `lines` (positive = up) over `at`.
    pub fn wheel(&mut self, at: impl Aim, lines: f64) -> &mut Self {
        self.wheel_with(at, lines, Mods::default())
    }

    pub fn wheel_with(&mut self, at: impl Aim, lines: f64, mods: Mods) -> &mut Self {
        let p = at.point(self);
        self.act(Act::Wheel { at: p, lines, mods });
        self
    }

    /// Press a key (the focused control takes arrows, Home, End, Escape).
    pub fn key(&mut self, key: Key) -> &mut Self {
        self.key_with(key, Mods::default())
    }

    pub fn key_with(&mut self, key: Key, mods: Mods) -> &mut Self {
        self.act(Act::Key { key, mods });
        self
    }

    /// Set a parameter directly (as automation would), in its own units.
    pub fn set(&mut self, param: impl ParamRef, value: f64) -> &mut Self {
        let id = param.id(self);
        self.param(id).set_value(value);
        self
    }

    /// Show one page or step (pages flip; steps are edits).
    pub fn show(&mut self, state: &[Visibility]) -> &mut Self {
        self.runtime.show(&self.controller, state);
        self
    }

    /// Advance the face's clock (animations).
    pub fn advance(&mut self, seconds: f64) -> &mut Self {
        self.time += seconds;
        self
    }

    // --------------------------------------------------------- queries

    /// The value of a parameter, in its own units.
    pub fn value(&self, param: impl ParamRef) -> f64 {
        self.param(param.id(self)).value()
    }

    /// The normalized (0–1) value of a parameter.
    pub fn normalized(&self, param: impl ParamRef) -> f64 {
        self.param(param.id(self)).normalized()
    }

    /// A parameter's value as the face prints it ("+3.0 dB").
    pub fn value_text(&self, param: impl ParamRef) -> String {
        let id = param.id(self);
        (&self.controller as &dyn Controller).value_text(id)
    }

    /// Every parameter of the table: (name, id), in declaration order.
    pub fn table_params(&self) -> Vec<(&'static str, ParamId)> {
        (0..self.table.count()).filter_map(|i| self.table.by_index(i)).map(|p| (p.meta().name, p.meta().id)).collect()
    }

    fn param(&self, id: ParamId) -> &dyn AnyParam {
        self.table.by_id(id).unwrap_or_else(|| panic!("no parameter {}", id.0))
    }

    /// The part a name names (a node id, a role, or a bound parameter's
    /// name, any case).
    pub fn part(&self, name: &str) -> PartId {
        self.runtime
            .find(&self.controller, name)
            .unwrap_or_else(|| panic!("no part named {name:?} on this face; it has {}", self.part_names()))
    }

    fn part_names(&self) -> String {
        let names: Vec<String> = self
            .runtime
            .parts()
            .into_iter()
            .filter_map(|(id, _)| id)
            .map(|id| match id {
                PartId::Name(n) => format!("{n:?}"),
                PartId::Param(p) => self.table.by_id(p).map_or(format!("param {}", p.0), |p| format!("{:?}", p.meta().name)),
            })
            .collect();
        names.join(", ")
    }

    /// Where `at` aims, as a point.
    pub fn at(&self, at: impl Aim) -> Pt {
        at.point(self)
    }

    /// A part's solved rect.
    pub fn rect(&self, part: impl Into<PartName>) -> Rect {
        let id = part.into().resolve(self);
        self.runtime.rect(id).unwrap_or_else(|| panic!("no part {id:?}"))
    }

    /// A part's grabs as it shows now (none while it is hidden).
    pub fn grabs(&self, part: impl Into<PartName>) -> Vec<Grab> {
        let id = part.into().resolve(self);
        self.runtime.grabs(&self.controller, id)
    }

    /// What a press at `at` would grab: the part and the gesture.
    pub fn grab_at(&self, at: impl Aim) -> Option<(Option<PartId>, Gesture)> {
        self.runtime.grab_at(&self.controller, at.point(self))
    }

    /// The part under the pointer.
    pub fn hovered(&self) -> Option<PartId> {
        self.runtime.hovered().and_then(|(id, _)| id)
    }

    /// The part being dragged (or held).
    pub fn active(&self) -> Option<PartId> {
        self.runtime.dragging().and_then(|(id, _)| id)
    }

    /// The part with keyboard focus.
    pub fn focus(&self) -> Option<PartId> {
        self.runtime.focused().and_then(|(id, _)| id)
    }

    /// The pointer shape the face asks for.
    pub fn cursor(&self) -> Cursor {
        self.runtime.cursor()
    }

    /// The page shown for page set `key`.
    pub fn page(&self, key: u32) -> u32 {
        self.runtime.page(key)
    }

    /// The face's size in points.
    pub fn size(&self) -> (f64, f64) {
        self.runtime.size()
    }

    /// Every edit sent to the host so far.
    pub fn edits(&self) -> Vec<Edit> {
        self.log.all()
    }

    /// The edits since the last call (and forget them).
    pub fn take_edits(&mut self) -> Vec<Edit> {
        self.log.take()
    }

    /// Every edit so far is inside a begin … end of its parameter, and
    /// nothing is left open. Panics with the edits otherwise.
    pub fn assert_bracketed(&self) {
        if let Err(e) = bracketed(&self.edits()) {
            panic!("{e}\nedits: {:?}\ntrace:\n{}", self.edits(), self.trace);
        }
    }

    /// Every event sent so far, replayable.
    pub fn trace(&self) -> &Trace {
        &self.trace
    }

    /// The live picture of each shown part, in paint order (what is on
    /// top paints last).
    pub fn layers(&self) -> Vec<Layer> {
        self.runtime.layers(&self.controller, self.time)
    }

    /// Every line of text the face shows now: baked legends, then live
    /// readouts in paint order.
    pub fn texts(&self) -> Vec<Shown> {
        let mut out = Vec::new();
        // Text sits in points at any scale; a tiny bake keeps it cheap.
        let baked = self.runtime.static_layers(&self.controller, 0.0625);
        for (layers, live) in [(baked, false), (self.layers(), true)] {
            for layer in layers {
                for op in &layer.scene.ops {
                    if let Op::Text { at, text, size, font, .. } = op {
                        let (x, y, w, h) = BoxText::bounds(*at, text, *size, font);
                        out.push(Shown { part: layer.id, text: text.clone(), bounds: Rect { x, y, w, h }, live });
                    }
                }
            }
        }
        out
    }

    /// True if some line of text on the face reads exactly `text`.
    pub fn shows(&self, text: &str) -> bool {
        self.texts().iter().any(|t| t.text == text)
    }

    /// The face drawn at `scale` px/pt, text in [`BoxText`] (the same on
    /// every machine: what golden images compare).
    pub fn render(&mut self, scale: f64) -> Canvas {
        let mut canvas = Canvas::new(0, 0);
        self.runtime.render(&self.controller, scale, self.time, &mut canvas, &mut BoxText);
        canvas
    }

    /// The face's runtime and controller (for checks the driver does not
    /// wrap).
    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    pub fn controller(&self) -> &dyn Controller {
        &self.controller
    }

    /// Every visibility state of the face (see [`Runtime::states`]).
    pub fn states(&self) -> Vec<Vec<Visibility>> {
        self.runtime.states()
    }
}

/// A part by name or id, for queries.
pub enum PartName {
    Id(PartId),
    Name(String),
}

impl PartName {
    fn resolve(self, ui: &FaceDriver) -> PartId {
        match self {
            PartName::Id(id) => id,
            PartName::Name(n) => ui.part(&n),
        }
    }
}

impl From<&str> for PartName {
    fn from(n: &str) -> Self {
        PartName::Name(n.to_string())
    }
}

impl From<PartId> for PartName {
    fn from(id: PartId) -> Self {
        PartName::Id(id)
    }
}

impl From<ParamId> for PartName {
    fn from(id: ParamId) -> Self {
        PartName::Id(PartId::Param(id))
    }
}

/// Every `Perform` is inside a `Begin … End` of its own parameter, every
/// `End` closes a `Begin`, and nothing is left open.
pub fn bracketed(edits: &[Edit]) -> Result<(), String> {
    let mut open: Vec<ParamId> = Vec::new();
    for (i, e) in edits.iter().enumerate() {
        match *e {
            Edit::Begin(id) => {
                if open.contains(&id) {
                    return Err(format!("edit {i}: begin of param {} while its gesture is open", id.0));
                }
                open.push(id);
            }
            Edit::Perform(id, v) => {
                if !open.contains(&id) {
                    return Err(format!("edit {i}: param {} set to {v} outside a begin … end", id.0));
                }
            }
            Edit::End(id) => {
                let Some(at) = open.iter().position(|&o| o == id) else {
                    return Err(format!("edit {i}: end of param {} with no begin", id.0));
                };
                open.remove(at);
            }
        }
    }
    match open.first() {
        Some(id) => Err(format!("param {}'s gesture never ended", id.0)),
        None => Ok(()),
    }
}
