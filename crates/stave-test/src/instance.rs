//! The harness as a stave-host [`Instance`]: everything stave-host
//! measures, drives and validates — `drive::Driver`, the frequency
//! response, the bench phases, the validation checks — runs on a linked
//! plugin in-process, through its Rust interface, with no bundle.
//! Notes and parameters convert here, at the edge.

use stave::params::AnyParam;
use stave::{Event, EventKind, NoteEvent, NoteKind, Plugin};
use stave_host::control::{Control, ControlError, ControlFlags};
use stave_host::instance::{Instance, Layout, StateError, Tail};
use stave_host::note;

use crate::harness::ParamName;
use crate::PluginHarness;

/// A param as stave-host's [`Control`] (plain values, steps minus one).
pub fn control_of(p: &dyn AnyParam) -> Control {
    let m = p.meta();
    let steps = p.step_count();
    Control {
        id: m.id.0,
        name: m.name.to_string(),
        group: m.group.to_string(),
        unit: m.unit.label().to_string(),
        min: p.denormalize(0.0),
        max: p.denormalize(1.0),
        default: p.denormalize(p.default_normalized()),
        value: p.value(),
        normalized: p.normalized(),
        steps: steps.saturating_sub(1),
        step_names: (0..steps).filter_map(|i| p.step_name(i)).map(str::to_string).collect(),
        flags: ControlFlags {
            automatable: !m.flags.non_automatable,
            read_only: false,
            hidden: m.flags.hidden,
            bypass: m.flags.bypass,
            program_change: false,
        },
    }
}

/// A stave-host note as the plugin's [`Event`].
pub fn event_of(n: &note::NoteEvent, id: i32) -> Event {
    Event {
        frame: n.offset as usize,
        kind: EventKind::Note(NoteEvent {
            kind: match n.kind {
                note::NoteKind::On => NoteKind::On,
                note::NoteKind::Off => NoteKind::Off,
            },
            key: n.pitch,
            channel: 0,
            velocity: f64::from(n.velocity),
            note_id: id,
        }),
    }
}

impl<P: Plugin> Instance for PluginHarness<P> {
    fn layout(&self) -> Layout {
        Layout {
            inputs: self.input_channels(),
            outputs: self.output_channels(),
            sample_rate: f64::from(self.sample_rate()),
            max_block: self.block_size(),
        }
    }

    fn latency(&self) -> u32 {
        PluginHarness::latency(self)
    }

    fn tail(&self) -> Tail {
        match PluginHarness::tail(self) {
            stave::Tail::None => Tail::Samples(0),
            stave::Tail::Samples(n) => Tail::Samples(n),
            stave::Tail::Infinite => Tail::Infinite,
        }
    }

    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]], notes: &[note::NoteEvent]) {
        let frames = outputs.first().map_or(0, |o| o.len()).min(self.block_size());
        for (dst, src) in self.inputs_mut().zip(inputs) {
            dst[..frames].copy_from_slice(&src[..frames]);
        }
        let mut events = self.take_events();
        events.clear();
        events.extend(notes.iter().map(|n| event_of(n, -1)));
        self.process_frames(frames, &[], &events);
        self.put_events(events);
        for (dst, src) in outputs.iter_mut().zip(self.outputs()) {
            dst[..frames].copy_from_slice(&src[..frames]);
        }
    }

    fn controls(&self) -> Vec<Control> {
        let table = self.table();
        (0..table.count()).filter_map(|i| table.by_index(i)).map(control_of).collect()
    }

    fn set_control(&mut self, name: &str, plain: f64) -> Result<(), ControlError> {
        let table = self.table();
        let p = ParamName::lookup(&name, &*table).ok_or_else(|| ControlError::NotFound(name.to_string()))?;
        let (lo, hi) = (p.denormalize(0.0), p.denormalize(1.0));
        p.set_value(plain.clamp(lo.min(hi), lo.max(hi)));
        Ok(())
    }

    fn save_state(&mut self) -> Result<Vec<u8>, StateError> {
        Ok(self.save())
    }

    fn load_state(&mut self, bytes: &[u8]) -> Result<(), StateError> {
        self.load(bytes).map_err(|_| StateError::Unreadable { bytes: bytes.len() })
    }
}
