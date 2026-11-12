//! The harness as a stave-host [`Instance`]: everything stave-host
//! measures, drives and validates — `drive::Driver`, the frequency
//! response, the bench phases, the validation checks — runs on a linked
//! plugin in-process, through its Rust interface, with no bundle.
//! Events and params are stave-core's on both sides: nothing converts.

use stave::Plugin;
use stave_host::instance::{Instance, Layout, StateError, Tail};
use stave_host::param::{Param, ParamError};

use crate::harness::ParamName;
use crate::PluginHarness;

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

    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]], events: &[stave::Event]) {
        let frames = outputs.first().map_or(0, |o| o.len()).min(self.block_size());
        for (dst, src) in self.inputs_mut().zip(inputs) {
            dst[..frames].copy_from_slice(&src[..frames]);
        }
        self.process_frames(frames, &[], events);
        for (dst, src) in outputs.iter_mut().zip(self.outputs()) {
            dst[..frames].copy_from_slice(&src[..frames]);
        }
    }

    fn params(&self) -> Vec<Param> {
        let table = self.table();
        (0..table.count()).filter_map(|i| table.by_index(i)).map(Param::of).collect()
    }

    fn set_param(&mut self, name: &str, value: f64) -> Result<(), ParamError> {
        let table = self.table();
        let p = ParamName::lookup(&name, &*table).ok_or_else(|| ParamError::NotFound(name.to_string()))?;
        let (lo, hi) = (p.denormalize(0.0), p.denormalize(1.0));
        p.set_value(value.clamp(lo.min(hi), lo.max(hi)));
        Ok(())
    }

    fn save_state(&mut self) -> Result<Vec<u8>, StateError> {
        Ok(self.save())
    }

    fn load_state(&mut self, bytes: &[u8]) -> Result<(), StateError> {
        self.load(bytes).map_err(|_| StateError::Unreadable { bytes: bytes.len() })
    }
}
