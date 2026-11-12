//! In-process plugin harness. Constructs a `ProcessCtx`, drives the
//! `Plugin::process` method, returns the output as owned buffers. No FFI,
//! no bundle load — the fastest possible iteration for unit testing.
//!
//! For tests against the bundled .vst3 / .clap dylib, see the
//! `bundle_host` module (which exercises the full ABI path).

use stave_core::{AudioBuffer, Event, Plugin, ProcessCtx};

/// Owns input/output buffers around a single `Plugin` instance. Each call
/// to `render` produces a fresh output snapshot (owned, allocates).
pub struct PluginHarness<P: Plugin> {
    plugin: P,
    sample_rate: f32,
    block_size: usize,
    input_chans: usize,
    output_chans: usize,
    input_buffers: Vec<Vec<f32>>,
    output_buffers: Vec<Vec<f32>>,
    /// Into the buffers above; their heap storage never moves.
    in_ptrs: Vec<*mut f32>,
    out_ptrs: Vec<*mut f32>,
}

impl<P: Plugin> PluginHarness<P> {
    /// Build a harness, allocate buffers, and activate the plugin.
    pub fn new(sample_rate: f32, block_size: usize) -> Self {
        let ports = P::CONFIGS[0];
        let input_chans = ports.in_channels() as usize;
        let output_chans = ports.out_channels() as usize;
        let mut plugin = P::new();
        plugin.activate(sample_rate, block_size);
        let mut input_buffers = vec![vec![0.0; block_size]; input_chans];
        let mut output_buffers = vec![vec![0.0; block_size]; output_chans];
        let in_ptrs = input_buffers.iter_mut().map(|c| c.as_mut_ptr()).collect();
        let out_ptrs = output_buffers.iter_mut().map(|c| c.as_mut_ptr()).collect();
        Self {
            plugin,
            sample_rate,
            block_size,
            input_chans,
            output_chans,
            input_buffers,
            output_buffers,
            in_ptrs,
            out_ptrs,
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }
    pub fn block_size(&self) -> usize {
        self.block_size
    }
    pub fn output_channels(&self) -> usize {
        self.output_chans
    }
    pub fn plugin(&self) -> &P {
        &self.plugin
    }
    pub fn plugin_mut(&mut self) -> &mut P {
        &mut self.plugin
    }

    /// Fill the input buffer for channel `idx` with `samples`. Length must
    /// equal `block_size`. Use this for effect-style tests.
    pub fn set_input(&mut self, idx: usize, samples: &[f32]) {
        assert_eq!(samples.len(), self.block_size);
        self.input_buffers[idx].copy_from_slice(samples);
    }

    pub fn inputs_mut(&mut self) -> &mut [Vec<f32>] {
        &mut self.input_buffers
    }

    pub fn outputs(&self) -> &[Vec<f32>] {
        &self.output_buffers
    }

    /// Process one block in place, allocation-free (the no-alloc and perf
    /// checks run it under their guard and timer); read it with `outputs`.
    pub fn run_block(&mut self, events: &[Event]) {
        // SAFETY: each pointer is into a buffer of exactly block_size,
        // owned by self and alive for this call; channels are distinct
        // allocations.
        let (input, mut output) = unsafe {
            (
                AudioBuffer::from_raw(&self.in_ptrs, self.block_size),
                AudioBuffer::from_raw(&self.out_ptrs, self.block_size),
            )
        };
        // Zero outputs each block — synth plugins overwrite, effects may
        // not. Through the pointers, not `output_buffers`: a fresh `&mut`
        // to the buffers would invalidate them.
        for c in 0..output.channel_count() {
            output.channel(c).fill(0.0);
        }

        let mut ctx = ProcessCtx {
            sample_rate: self.sample_rate,
            frames: self.block_size,
            transport: None,
            input,
            output,
            aux: &[],
            params: stave_core::ParamBlock::empty(),
            events,
        };
        self.plugin.process(&mut ctx);
    }

    /// [`run_block`](Self::run_block), returning a copy of the output.
    pub fn process_block(&mut self, events: &[Event]) -> Vec<Vec<f32>> {
        self.run_block(events);
        self.output_buffers.clone()
    }

    /// Render `frames` total samples by running successive blocks. `events`
    /// is delivered at frame 0 of the first block; subsequent blocks see no
    /// events. For sample-accurate event timing, call `process_block`
    /// directly and assemble blocks yourself.
    pub fn render(&mut self, frames: usize, events: &[Event]) -> Vec<Vec<f32>> {
        let mut acc: Vec<Vec<f32>> = vec![Vec::with_capacity(frames); self.output_chans];
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

    /// Number of input channels declared by the plugin.
    pub fn input_channels(&self) -> usize {
        self.input_chans
    }
}
