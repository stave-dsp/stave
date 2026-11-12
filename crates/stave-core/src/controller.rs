//! The editor contract: what any editor (a face, a test, a tool) reads
//! and writes on its plugin — the VST3 meaning of *controller*.
//!
//! [`Controller`] is the portable surface: parameters by [`ParamId`],
//! taps by [`TapId`], the latest capture and curve, factory presets.
//! [`PluginController`] is the one implementation, over the plugin's
//! table and telemetry; every edit goes through a [`GestureSink`] the
//! format wrapper supplies, so the DAW records and undoes it.

use std::boxed::Box;
use std::string::{String, ToString};
use std::sync::Arc;
use std::vec::Vec;
use core::cell::Cell;

use stave_telemetry::{TapId, Telemetry, CAPTURE_LEN};

use crate::params::{AnyParam, ParamId, Params};
use crate::preset::Preset;
use crate::units::Normalized;

/// What an editor reads and writes on its plugin. Values cross as
/// NORMALIZED (0–1 in the parameter's own taper); an edit is a gesture,
/// `begin`, any number of `set`s, `end`.
pub trait Controller {
    /// The parameter `id`, for its value, format and steps.
    fn param(&self, id: ParamId) -> Option<&dyn AnyParam>;
    /// Open an edit gesture on `id` (the host's automation bracket).
    fn begin(&self, id: ParamId);
    /// Move `id` to `normalized` inside a gesture.
    fn set(&self, id: ParamId, normalized: Normalized);
    /// Close the gesture `begin` opened.
    fn end(&self, id: ParamId);
    /// The latest value of tap `id` (0 until the plugin publishes it).
    fn tap(&self, id: TapId) -> f64;
    /// The latest captured audio of `channel` into `out`; how many
    /// frames were written (0 when nothing is captured).
    fn capture(&self, _channel: usize, _out: &mut [f32]) -> usize {
        0
    }
    /// The plugin's published transfer curve (output over the input
    /// span), empty when it publishes none.
    fn curve(&self) -> Vec<f32> {
        Vec::new()
    }
    /// The factory presets.
    fn presets(&self) -> &[Preset] {
        &[]
    }
    /// The preset last applied, if any.
    fn preset(&self) -> Option<usize> {
        None
    }
    /// Apply factory preset `index`, every value as its own gesture.
    fn apply_preset(&self, _index: usize) {}
}

/// Readers every editor wants, over [`Controller::param`]: an unknown
/// id reads as a continuous parameter at 0 with no text.
impl dyn Controller + '_ {
    pub fn normalized(&self, id: ParamId) -> f64 {
        self.param(id).map_or(0.0, |p| p.normalized())
    }
    pub fn default_normalized(&self, id: ParamId) -> f64 {
        self.param(id).map_or(0.0, |p| p.default_normalized())
    }
    /// The number of discrete steps, 0 for a continuous parameter.
    pub fn steps(&self, id: ParamId) -> u32 {
        self.param(id).map_or(0, |p| p.step_count())
    }
    pub fn step_name(&self, id: ParamId, index: u32) -> Option<&'static str> {
        self.param(id)?.step_name(index)
    }
    /// The current value as the plugin formats it ("1.20 s", "VINYL").
    pub fn value_text(&self, id: ParamId) -> String {
        self.param(id)
            .map_or_else(String::new, |p| p.format(p.value()).as_str().to_string())
    }
}

/// The wrapper side of an edit: where a gesture goes (the host's
/// automation). The value is already in the table when `perform` runs;
/// the sink tells the host.
pub trait GestureSink: Send {
    fn begin(&self, id: ParamId);
    /// `value` in the param's own units.
    fn perform(&self, id: ParamId, value: f64);
    fn end(&self, id: ParamId);
}

/// The [`Controller`] of a running plugin: its parameter table, its
/// telemetry, its factory presets, and the wrapper's [`GestureSink`].
/// Edits write the table (the truth every reader sees) and then the
/// sink; edits to ids the table does not declare are ignored.
pub struct PluginController {
    table: Arc<dyn Params>,
    telemetry: Arc<Telemetry>,
    presets: Arc<[Preset]>,
    current: Cell<Option<usize>>,
    sink: Box<dyn GestureSink>,
}

impl PluginController {
    pub fn new(
        table: Arc<dyn Params>,
        telemetry: Arc<Telemetry>,
        presets: Arc<[Preset]>,
        sink: Box<dyn GestureSink>,
    ) -> Self {
        PluginController {
            table,
            telemetry,
            presets,
            current: Cell::new(None),
            sink,
        }
    }

    /// Write `value` (clamped into range) and tell the sink.
    fn perform(&self, p: &dyn AnyParam, value: f64) {
        let (lo, hi) = (p.denormalize(0.0), p.denormalize(1.0));
        let value = value.clamp(lo.min(hi), lo.max(hi));
        p.set_value(value);
        self.sink.perform(p.meta().id, value);
    }
}

impl Controller for PluginController {
    fn param(&self, id: ParamId) -> Option<&dyn AnyParam> {
        self.table.by_id(id)
    }
    fn begin(&self, id: ParamId) {
        if self.table.by_id(id).is_some() {
            self.sink.begin(id);
        }
    }
    fn set(&self, id: ParamId, normalized: Normalized) {
        if let Some(p) = self.table.by_id(id) {
            self.perform(p, p.denormalize(normalized.get()));
        }
    }
    fn end(&self, id: ParamId) {
        if self.table.by_id(id).is_some() {
            self.sink.end(id);
        }
    }
    fn tap(&self, id: TapId) -> f64 {
        self.telemetry.value(id).unwrap_or(0.0)
    }
    fn capture(&self, channel: usize, out: &mut [f32]) -> usize {
        let mut buf = [0f32; CAPTURE_LEN];
        let (n, _) = self.telemetry.read_capture_channel(channel, &mut buf);
        let n = (n as usize).min(out.len());
        out[..n].copy_from_slice(&buf[..n]);
        n
    }
    fn curve(&self) -> Vec<f32> {
        self.telemetry.curve()
    }
    fn presets(&self) -> &[Preset] {
        &self.presets
    }
    fn preset(&self) -> Option<usize> {
        self.current.get()
    }
    fn apply_preset(&self, index: usize) {
        let Some(preset) = self.presets.get(index) else {
            return;
        };
        for &(id, value) in &preset.values {
            if let Some(p) = self.table.by_id(id) {
                self.sink.begin(id);
                self.perform(p, value);
                self.sink.end(id);
            }
        }
        self.current.set(Some(index));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::{FloatParam, Range};
    use std::sync::Mutex;

    struct Drive(FloatParam);

    impl Params for Drive {
        fn count(&self) -> usize {
            1
        }
        fn by_index(&self, index: usize) -> Option<&dyn AnyParam> {
            (index == 0).then_some(&self.0 as &dyn AnyParam)
        }
    }

    #[derive(Default)]
    struct Log(Arc<Mutex<Vec<String>>>);

    impl GestureSink for Log {
        fn begin(&self, id: ParamId) {
            self.0.lock().unwrap().push(std::format!("begin {}", id.0));
        }
        fn perform(&self, id: ParamId, value: f64) {
            self.0.lock().unwrap().push(std::format!("perform {} {value}", id.0));
        }
        fn end(&self, id: ParamId) {
            self.0.lock().unwrap().push(std::format!("end {}", id.0));
        }
    }

    fn rig(presets: Vec<Preset>) -> (PluginController, Arc<Mutex<Vec<String>>>) {
        let table: Arc<dyn Params> = Arc::new(Drive(FloatParam::new(ParamId(7), "Drive", Range::linear(-12.0, 12.0))));
        let log = Log::default();
        let seen = log.0.clone();
        (PluginController::new(table, Telemetry::new(), presets.into(), Box::new(log)), seen)
    }

    #[test]
    fn an_edit_writes_the_table_then_tells_the_sink_in_plain_units() {
        let (c, log) = rig(Vec::new());
        let c: &dyn Controller = &c;
        c.begin(ParamId(7));
        c.set(ParamId(7), Normalized::new(0.75));
        c.set(ParamId(7), Normalized::new(2.0));
        c.end(ParamId(7));
        assert_eq!(*log.lock().unwrap(), ["begin 7", "perform 7 6", "perform 7 12", "end 7"]);
        assert_eq!(c.normalized(ParamId(7)), 1.0);
        assert_eq!(c.value_text(ParamId(7)), c.param(ParamId(7)).unwrap().format(12.0).as_str());
    }

    #[test]
    fn unknown_ids_are_ignored_and_read_as_zero() {
        let (c, log) = rig(Vec::new());
        let c: &dyn Controller = &c;
        c.begin(ParamId(9));
        c.set(ParamId(9), Normalized::new(0.5));
        c.end(ParamId(9));
        assert!(log.lock().unwrap().is_empty());
        assert_eq!((c.normalized(ParamId(9)), c.steps(ParamId(9))), (0.0, 0));
        assert_eq!(c.tap(TapId::new("nothing")), 0.0);
    }

    #[test]
    fn a_preset_applies_each_value_as_a_clamped_gesture() {
        let preset = Preset {
            name: "Hot",
            values: std::vec![(ParamId(7), 40.0), (ParamId(99), 1.0)],
        };
        let (c, log) = rig(std::vec![preset]);
        assert_eq!(c.preset(), None);
        c.apply_preset(0);
        c.apply_preset(5);
        assert_eq!(*log.lock().unwrap(), ["begin 7", "perform 7 12", "end 7"]);
        assert_eq!(c.preset(), Some(0));
    }
}
