//! `Instance` — the seam between hosting and analysis. Everything that
//! measures or validates a plugin (bench phases, validation checks, the
//! renderer) is written against this trait, so it runs the same on a
//! loaded VST3 ([`LoadedPlugin`](crate::plugin::LoadedPlugin)) and on a
//! pure-Rust test double.

use crate::control::{Control, ControlError};
use crate::note::NoteEvent;
use stave_core::diagnostics::Severity;

/// The fixed shape an instance was set up with.
#[derive(Copy, Clone, Debug, PartialEq, serde::Serialize)]
pub struct Layout {
    pub inputs: usize,
    pub outputs: usize,
    pub sample_rate: f64,
    /// The largest block one `process` call may carry.
    pub max_block: usize,
}

impl Layout {
    /// No audio input: driven by notes (an instrument).
    pub fn is_instrument(&self) -> bool {
        self.inputs == 0
    }
}

/// How long output continues after input stops (VST3 `getTailSamples`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tail {
    Samples(u32),
    Infinite,
}

/// A message the plugin logged (stave's debug bus), with its position in
/// the plugin's message stream.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct PluginMessage {
    pub index: u64,
    #[serde(serialize_with = "severity_name")]
    pub severity: Severity,
    pub text: String,
}

fn severity_name<S: serde::Serializer>(severity: &Severity, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(severity.as_str())
}

/// A plugin-side measurement (stave's debug-bus probes).
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Probe {
    pub name: String,
    pub latest: f64,
    pub min: f64,
    pub max: f64,
}

impl Probe {
    pub const NEWTON_SOLVES: &'static str = "newton.solves";
    pub const NEWTON_MEAN_ITERATIONS: &'static str = "newton.mean_iterations";
    pub const NEWTON_CAPPED: &'static str = "newton.capped";
    pub const NEWTON_MAX_FINAL_STEP: &'static str = "newton.max_final_step";
}

/// Why a state save/restore failed.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum StateError {
    #[error("instance has no state")]
    Unsupported,
    /// The plugin's own save or load call failed with `result`.
    #[error("{call} failed: result=0x{:08x}", *result as u32)]
    Refused { call: &'static str, result: i32 },
    #[error("the plugin cannot read a {bytes}-byte state")]
    Unreadable { bytes: usize },
}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum PresetError {
    #[error("no preset named '{name}'; available: {available:?}")]
    NotFound {
        name: String,
        available: Vec<String>,
    },
    #[error(transparent)]
    Control(#[from] ControlError),
}

pub trait Instance {
    fn layout(&self) -> Layout;

    /// Reported processing latency (plugin delay compensation), frames.
    fn latency(&self) -> u32 {
        0
    }

    fn tail(&self) -> Tail {
        Tail::Samples(0)
    }

    /// Process one block. `outputs` holds `layout().outputs` channels
    /// of equal length `n` (≤ `max_block`); `inputs` holds
    /// `layout().inputs` channels of length `n`. Note offsets are
    /// relative to the block start.
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]], notes: &[NoteEvent]);

    /// The current control surface (values read live).
    fn controls(&self) -> Vec<Control> {
        Vec::new()
    }

    /// Set a control by name (see [`control::find`](crate::control::find)),
    /// PLAIN value. Takes effect from the next `process` call.
    fn set_control(&mut self, name: &str, _plain: f64) -> Result<(), ControlError> {
        Err(ControlError::NotFound(name.to_string()))
    }

    fn save_state(&mut self) -> Result<Vec<u8>, StateError> {
        Err(StateError::Unsupported)
    }

    fn load_state(&mut self, _bytes: &[u8]) -> Result<(), StateError> {
        Err(StateError::Unsupported)
    }

    fn probes(&self) -> Vec<Probe> {
        Vec::new()
    }

    /// Messages logged since index `from` that the plugin still holds.
    fn messages(&self, _from: u64) -> Vec<PluginMessage> {
        Vec::new()
    }
}

/// The program-change control (VST3 `kIsProgramChange`): its positions
/// are the instance's presets.
fn program(controls: Vec<Control>) -> Option<Control> {
    controls.into_iter().find(|c| c.flags.program_change)
}

/// Preset names, in program order.
pub fn presets(inst: &dyn Instance) -> Vec<String> {
    program(inst.controls()).map_or_else(Vec::new, |c| c.step_names)
}

/// Select a preset by name through the program-change control.
pub fn apply_preset(inst: &mut dyn Instance, name: &str) -> Result<(), PresetError> {
    let not_found = |available| PresetError::NotFound {
        name: name.to_string(),
        available,
    };
    let Some(c) = program(inst.controls()) else {
        return Err(not_found(Vec::new()));
    };
    let Some(index) = c
        .step_names
        .iter()
        .position(|s| s.eq_ignore_ascii_case(name))
    else {
        return Err(not_found(c.step_names));
    };
    Ok(inst.set_control(&c.name, index as f64)?)
}

impl<T: Instance + ?Sized> Instance for Box<T> {
    fn layout(&self) -> Layout {
        (**self).layout()
    }
    fn latency(&self) -> u32 {
        (**self).latency()
    }
    fn tail(&self) -> Tail {
        (**self).tail()
    }
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]], notes: &[NoteEvent]) {
        (**self).process(inputs, outputs, notes)
    }
    fn controls(&self) -> Vec<Control> {
        (**self).controls()
    }
    fn set_control(&mut self, name: &str, plain: f64) -> Result<(), ControlError> {
        (**self).set_control(name, plain)
    }
    fn save_state(&mut self) -> Result<Vec<u8>, StateError> {
        (**self).save_state()
    }
    fn load_state(&mut self, bytes: &[u8]) -> Result<(), StateError> {
        (**self).load_state(bytes)
    }
    fn probes(&self) -> Vec<Probe> {
        (**self).probes()
    }
    fn messages(&self, from: u64) -> Vec<PluginMessage> {
        (**self).messages(from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Gain;

    #[test]
    fn presets_are_the_program_controls_positions() {
        let mut g = Gain::new(1, 1)
            .with_preset("quiet", &[("gain", 0.5)])
            .with_preset("loud", &[("gain", 2.0)]);
        assert_eq!(presets(&g), ["quiet", "loud"]);
        assert_eq!(apply_preset(&mut g, "LOUD"), Ok(()));
        assert_eq!(
            crate::control::find(&g.controls(), "gain").unwrap().value,
            2.0
        );
    }

    #[test]
    fn unknown_preset_lists_alternatives() {
        let mut g = Gain::new(1, 1).with_preset("loud", &[]);
        assert_eq!(
            apply_preset(&mut g, "quiet"),
            Err(PresetError::NotFound {
                name: "quiet".into(),
                available: vec!["loud".into()]
            })
        );
    }
}
