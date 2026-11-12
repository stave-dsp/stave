//! In-process plugin harness: builds a [`Block`] over owned buffers and
//! drives `Plugin::process`. No FFI, no bundle load — the fastest
//! iteration for unit tests. (`validation` exercises the full ABI path.)

use std::sync::Arc;

use stave::__export::Shell;
use stave::params::Params;
use stave::{Block, Environment, Event, InputBuffer, OutputBuffer, Plugin, Telemetry};
use stave_core::runtime::Erased;

/// Owns a plugin, its telemetry, and fixed-size input/output buffers.
/// Built prepared: [`new`](Self::new) runs `Plugin::prepare`.
pub struct PluginHarness<P: Plugin> {
    shell: Shell<P>,
    telemetry: Arc<Telemetry>,
    sample_rate: f32,
    block_size: usize,
    /// Fixed-length channels: `Box<[f32]>`, never `Vec`, so no caller can
    /// resize one — channels are only ever lent out as slices.
    input_buffers: Vec<Box<[f32]>>,
    output_buffers: Vec<Box<[f32]>>,
    /// The pointer lists the block's buffers read, one slot per channel,
    /// refilled from the buffers every block (never allocates).
    in_ptrs: Box<[*mut f32]>,
    out_ptrs: Box<[*mut f32]>,
}

impl<P: Plugin> PluginHarness<P> {
    /// Build the plugin on its preferred layout, allocate buffers, and
    /// prepare it at `sample_rate` for blocks of `block_size`.
    pub fn new(sample_rate: f32, block_size: usize) -> Self {
        let layout = P::LAYOUTS[0];
        let (input_chans, output_chans) = (layout.input_channels(), layout.output_channels());
        let telemetry = Telemetry::new();
        let mut shell = <Shell<P> as Erased>::new(&telemetry);
        shell.prepare(&Environment::new(sample_rate, block_size).with_layout(layout));
        Self {
            shell,
            telemetry,
            sample_rate,
            block_size,
            input_buffers: vec![vec![0.0; block_size].into_boxed_slice(); input_chans],
            output_buffers: vec![vec![0.0; block_size].into_boxed_slice(); output_chans],
            in_ptrs: vec![core::ptr::null_mut(); input_chans].into_boxed_slice(),
            out_ptrs: vec![core::ptr::null_mut(); output_chans].into_boxed_slice(),
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }
    pub fn block_size(&self) -> usize {
        self.block_size
    }
    pub fn output_channels(&self) -> usize {
        self.output_buffers.len()
    }
    /// Number of input channels the plugin's preferred layout declares.
    pub fn input_channels(&self) -> usize {
        self.input_buffers.len()
    }
    pub fn plugin(&self) -> &P {
        self.shell.plugin()
    }
    pub fn plugin_mut(&mut self) -> &mut P {
        self.shell.plugin_mut()
    }
    /// The typed params.
    pub fn params(&self) -> &Arc<P::Params> {
        self.shell.params()
    }
    /// The params as a table (by name, by id).
    pub fn table(&self) -> Arc<dyn Params> {
        Erased::params(&self.shell)
    }
    /// The plugin's telemetry.
    pub fn telemetry(&self) -> &Arc<Telemetry> {
        &self.telemetry
    }

    /// Fill input channel `idx` with `samples` (exactly `block_size`).
    pub fn set_input(&mut self, idx: usize, samples: &[f32]) {
        assert_eq!(samples.len(), self.block_size);
        self.input_buffers[idx].copy_from_slice(samples);
    }

    /// Each input channel, writable, never resizable.
    pub fn inputs_mut(&mut self) -> impl Iterator<Item = &mut [f32]> + '_ {
        self.input_buffers.iter_mut().map(|c| &mut **c)
    }

    /// Input channel `idx`.
    pub fn input_mut(&mut self, idx: usize) -> &mut [f32] {
        &mut self.input_buffers[idx]
    }

    /// The last block's output, one slice per channel.
    pub fn outputs(&self) -> &[Box<[f32]>] {
        &self.output_buffers
    }

    /// Process one block in place, allocation free (the no-alloc and perf
    /// checks run it under their guard and timer); read it with
    /// [`outputs`](Self::outputs).
    pub fn run_block(&mut self, events: &[Event]) {
        let frames = self.block_size;
        let input = InputBuffer::from_slices(
            &mut self.in_ptrs,
            self.input_buffers.iter().map(|c| &**c),
            frames,
        );
        let output = OutputBuffer::from_slices(
            &mut self.out_ptrs,
            self.output_buffers.iter_mut().map(|c| &mut **c),
            frames,
        );
        let mut block = Block::new(self.sample_rate, frames, input, output).with_events(events);
        // Zeroed every block: instruments overwrite, effects may not.
        block.output().silence();
        self.shell.process(&mut block);
    }

    /// [`run_block`](Self::run_block), returning a copy of the output.
    pub fn process_block(&mut self, events: &[Event]) -> Vec<Vec<f32>> {
        self.run_block(events);
        self.output_buffers.iter().map(|c| c.to_vec()).collect()
    }

    /// Render `frames` samples in successive blocks. `events` arrive in
    /// the first block; later blocks see none. For sample-accurate
    /// timing, call `process_block` and assemble blocks yourself.
    pub fn render(&mut self, frames: usize, events: &[Event]) -> Vec<Vec<f32>> {
        let mut acc: Vec<Vec<f32>> = vec![Vec::with_capacity(frames); self.output_channels()];
        let mut written = 0;
        let mut first = true;
        while written < frames {
            let take = (frames - written).min(self.block_size);
            let block_events: &[Event] = if first { events } else { &[] };
            first = false;
            let out = self.process_block(block_events);
            for (ch, block) in out.iter().enumerate() {
                acc[ch].extend_from_slice(&block[..take]);
            }
            written += take;
        }
        acc
    }
}

#[cfg(test)]
mod tests {
    use super::PluginHarness;
    use stave::prelude::*;

    #[derive(Params)]
    struct NoKnobs {
        level: FloatParam,
    }

    impl Default for NoKnobs {
        fn default() -> Self {
            NoKnobs {
                level: FloatParam::new(ParamId(1), "Level", Range::linear(0.0, 1.0)),
            }
        }
    }

    /// out[c] = 2 × in[c].
    struct Double;
    impl Plugin for Double {
        const INFO: Info = Info {
            id: "com.stave.test.harness-double",
            name: "Double",
            vendor: "stave tests",
            ..Info::EFFECT
        };
        type Params = NoKnobs;
        fn new(_: &Arc<NoKnobs>, _: &Telemetry) -> Self {
            Double
        }
        fn process(&mut self, block: &mut Block<'_>) {
            let (input, output) = block.io();
            for (x, y) in input.channels().zip(output.channels_mut()) {
                for (x, y) in x.iter().zip(y) {
                    *y = 2.0 * x;
                }
            }
        }
    }

    /// S2: inputs are lent as fixed-length slices and the pointer list
    /// is refilled per block, so writes between blocks — through every
    /// accessor — are what the plugin reads (Miri: no stale pointer use).
    #[test]
    fn inputs_written_between_blocks_reach_the_plugin() {
        let mut h = PluginHarness::<Double>::new(48_000.0, 16);
        assert_eq!(h.input_channels(), 2);
        for block in 0..4 {
            for (c, ch) in h.inputs_mut().enumerate() {
                assert_eq!(ch.len(), 16);
                ch.fill((block * 10 + c) as f32);
            }
            h.run_block(&[]);
            for c in 0..2 {
                assert!(h.outputs()[c]
                    .iter()
                    .all(|&y| y == 2.0 * (block * 10 + c) as f32));
            }
            h.input_mut(1).fill(-1.0);
            h.set_input(0, &[0.5; 16]);
            let out = h.process_block(&[]);
            assert!(out[0].iter().all(|&y| y == 1.0));
            assert!(out[1].iter().all(|&y| y == -2.0));
        }
    }
}
