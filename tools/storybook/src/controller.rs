//! A stand-in plugin for the storybook: a real parameter table the face
//! edits in place, and taps that move with time.

use std::cell::Cell;
use std::sync::Arc;
use std::time::Instant;

use stave_core::params::{AnyParam, BoolParam, EnumParam, FloatParam, ParamId, Params, Range, Taper, Unit};
use stave_core::{param_enum, Controller};
use stave_face::TapId;

param_enum! {
    pub const SHAPES: Shape = ["SINE", "SQUARE", "DRIFT"];
}

param_enum! {
    pub const SYNCS: Sync = ["FREE", "TEMPO"];
}

/// The taps the stories read.
pub mod taps {
    use stave_face::TapId;

    /// A level swinging between about 0.05 and 0.95.
    pub const LEVEL: TapId = TapId::new("level");
    /// Seconds since the story opened.
    pub const PHASE: TapId = TapId::new("phase");
}

/// The demo parameters.
pub struct StoryParams {
    pub drive: FloatParam,
    pub tone: FloatParam,
    pub mix: FloatParam,
    pub time: FloatParam,
    pub rate: FloatParam,
    pub depth: FloatParam,
    pub shape: EnumParam<Shape>,
    pub sync: EnumParam<Sync>,
    pub level: FloatParam,
    pub bypass: BoolParam,
    pub hold: BoolParam,
    pub width: FloatParam,
}

impl StoryParams {
    fn new() -> Self {
        let percent = |id, name, v| FloatParam::new(id, name, v, Range::new(0.0, 1.0, Taper::Linear)).with_unit(Unit::Percent);
        let db = |id, name| {
            FloatParam::new(id, name, 0.0, Range::new(-24.0, 24.0, Taper::Linear)).with_unit(Unit::Db { floor_db: None })
        };
        StoryParams {
            drive: percent(1, "DRIVE", 0.62),
            tone: db(2, "TONE"),
            mix: percent(3, "MIX", 0.4),
            time: FloatParam::new(4, "TIME", 180.0, Range::new(10.0, 2000.0, Taper::Log)).with_unit(Unit::Ms),
            rate: FloatParam::new(5, "RATE", 0.4, Range::new(0.05, 20.0, Taper::Log)).with_unit(Unit::Hz),
            depth: percent(6, "DEPTH", 0.7),
            shape: EnumParam::new(7, "SHAPE", Shape(1)),
            sync: EnumParam::new(8, "SYNC", Sync(0)),
            level: db(9, "LEVEL"),
            bypass: BoolParam::new(10, "BYPASS", false).with_names("ON", "OFF"),
            hold: BoolParam::new(11, "HOLD", false).with_names("OFF", "ON"),
            width: percent(12, "WIDTH", 0.66),
        }
    }
}

impl Params for StoryParams {
    fn count(&self) -> usize {
        12
    }
    fn by_index(&self, index: usize) -> Option<&dyn AnyParam> {
        let p: &dyn AnyParam = match index {
            0 => &self.drive,
            1 => &self.tone,
            2 => &self.mix,
            3 => &self.time,
            4 => &self.rate,
            5 => &self.depth,
            6 => &self.shape,
            7 => &self.sync,
            8 => &self.level,
            9 => &self.bypass,
            10 => &self.hold,
            11 => &self.width,
            _ => return None,
        };
        Some(p)
    }
}

/// The storybook's controller: edits land in the table directly (there
/// is no host to tell), taps are functions of the story's clock.
pub struct StoryController {
    pub params: Arc<StoryParams>,
    opened: Instant,
    /// A fixed clock (screenshots), instead of the wall clock.
    frozen: Cell<Option<f64>>,
}

impl StoryController {
    pub fn new() -> Self {
        StoryController {
            params: Arc::new(StoryParams::new()),
            opened: Instant::now(),
            frozen: Cell::new(None),
        }
    }

    /// Stop the clock at `t` seconds.
    pub fn freeze(&self, t: f64) {
        self.frozen.set(Some(t));
    }

    pub fn time(&self) -> f64 {
        self.frozen.get().unwrap_or_else(|| self.opened.elapsed().as_secs_f64())
    }
}

impl Controller for StoryController {
    fn param(&self, id: ParamId) -> Option<&dyn AnyParam> {
        self.params.by_id(id)
    }
    fn begin(&self, _id: ParamId) {}
    fn set(&self, id: ParamId, normalized: f64) {
        if let Some(p) = self.params.by_id(id) {
            p.set_normalized(normalized.clamp(0.0, 1.0));
        }
    }
    fn end(&self, _id: ParamId) {}
    fn tap(&self, id: TapId) -> f64 {
        let t = self.time();
        if id == taps::LEVEL {
            0.5 + 0.45 * (t * 1.3).sin()
        } else if id == taps::PHASE {
            t
        } else {
            0.0
        }
    }
}
