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
    /// Fixed-length channels: `Box<[f32]>`, never `Vec`, so no caller
    /// can resize one — channels are only ever lent out as `&mut [f32]`.
    input_buffers: Vec<Box<[f32]>>,
    output_buffers: Vec<Box<[f32]>>,
    /// Pointer lists for `AudioBuffer::from_raw`, REBUILT from the
    /// buffers at the top of every `run_block` (capacity is reserved
    /// here, so the rebuild never allocates). Never cached across
    /// calls: a `&mut` lent out in between would invalidate them.
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
        let input_buffers = vec![vec![0.0; block_size].into_boxed_slice(); input_chans];
        let output_buffers = vec![vec![0.0; block_size].into_boxed_slice(); output_chans];
        let in_ptrs = Vec::with_capacity(input_chans);
        let out_ptrs = Vec::with_capacity(output_chans);
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

    /// Each input channel as a fixed-length `&mut [f32]` (length
    /// `block_size`) — writable, never resizable.
    pub fn inputs_mut(&mut self) -> impl Iterator<Item = &mut [f32]> + '_ {
        self.input_buffers.iter_mut().map(|c| &mut **c)
    }

    /// Input channel `idx` as a fixed-length `&mut [f32]`.
    pub fn input_mut(&mut self, idx: usize) -> &mut [f32] {
        &mut self.input_buffers[idx]
    }

    /// The last block's output, one fixed-length slice per channel.
    pub fn outputs(&self) -> &[Box<[f32]>] {
        &self.output_buffers
    }

    /// Process one block in place, allocation-free (the no-alloc and perf
    /// checks run it under their guard and timer); read it with `outputs`.
    pub fn run_block(&mut self, events: &[Event]) {
        // Fresh pointers every block, derived from the buffers' current
        // storage (within reserved capacity: no allocation).
        self.in_ptrs.clear();
        self.in_ptrs
            .extend(self.input_buffers.iter_mut().map(|c| c.as_mut_ptr()));
        self.out_ptrs.clear();
        self.out_ptrs
            .extend(self.output_buffers.iter_mut().map(|c| c.as_mut_ptr()));
        // SAFETY: each pointer was just derived from a live `Box<[f32]>`
        // of exactly block_size owned by self (fixed length — no API can
        // resize it); nothing else touches the buffers until `process`
        // returns; channels are distinct allocations.
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
        self.output_buffers.iter().map(|c| c.to_vec()).collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use stave_core::PluginDescriptor;

    /// out[c] = 2 × in[c].
    struct Double;
    impl Plugin for Double {
        const DESCRIPTOR: PluginDescriptor = PluginDescriptor {
            id: c"com.stave.test.harness-double",
            name: c"Double",
            vendor: c"stave tests",
            version: c"0.0.0",
            url: c"",
            description: c"harness fixture",
            features: &[],
        };
        fn new() -> Self {
            Self
        }
        fn process(&mut self, ctx: &mut ProcessCtx<'_>) {
            for c in 0..ctx.output.channel_count().min(ctx.input.channel_count()) {
                for i in 0..ctx.frames {
                    let x = ctx.input.channel_ref(c)[i];
                    ctx.output.channel(c)[i] = 2.0 * x;
                }
            }
        }
    }

    /// S2: inputs are lent as fixed-length `&mut [f32]` and the pointer
    /// list is rebuilt per block, so writes between blocks — through
    /// every accessor — are what the plugin reads (Miri: no stale
    /// pointer use).
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
