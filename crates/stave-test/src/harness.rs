//! The one in-process driver for a `stave::Plugin`: builds a [`Block`]
//! over owned buffers and calls `process`, with everything a host
//! supplies — a chosen layout, sidechain (aux) inputs, a transport,
//! sample-accurate parameter changes, events, telemetry and state. No
//! FFI and no bundle: the fastest loop for a plugin's own tests, and an
//! `stave_host::instance::Instance` so stave-host's driver, measurements and validation
//! checks run on it too (`validation` runs the same suite through the
//! plugin's VST3 entry points, so the ABI is under test there).
//!
//! ```
//! # use stave_test::PluginHarness;
//! # use stave_tilt_vst3::Tilt;
//! let mut h = PluginHarness::<Tilt>::builder().sample_rate(44_100.0).block_size(64).set("Tilt", 6.0).build();
//! h.inputs_mut().for_each(|c| c.fill(0.1));
//! h.process(&[]);
//! assert!(h.outputs()[0].iter().all(|y| y.is_finite()));
//! let out = h.run(1_000, &[]);                  // 1000 frames over successive blocks
//! assert_eq!(out[0].len(), 1_000);
//! ```

use std::sync::{Arc, Mutex};

use stave::__export::Shell;
use stave::params::{AnyParam, ParamId, Params};
use stave::process::{ParamBlock, ParamChange, ParamChangeKind};
use stave::{Block, Environment, Event, InputBuffer, Layout, OutputBuffer, Plugin, Telemetry, Transport};
use stave_core::runtime::Erased;
use stave_core::state::{self, StateStore};

/// Aux buses a harness feeds at most (a block's aux list lives on the
/// stack, so processing never allocates).
pub const MAX_AUX: usize = 4;

/// Owns a plugin, its telemetry, and fixed-size input, aux and output
/// buffers. Built prepared: `Plugin::prepare` has run.
pub struct PluginHarness<P: Plugin> {
    shell: Shell<P>,
    telemetry: Arc<Telemetry>,
    sample_rate: f32,
    block_size: usize,
    layout: Layout,
    transport: Option<Transport>,
    /// Fixed-length channels: `Box<[f32]>`, never `Vec`, so no caller can
    /// resize one — channels are only ever lent out as slices.
    input_buffers: Vec<Box<[f32]>>,
    output_buffers: Vec<Box<[f32]>>,
    /// One list of channels per aux bus of the layout.
    aux_buffers: Vec<Vec<Box<[f32]>>>,
    /// The pointer lists the block's buffers read, one slot per channel,
    /// refilled from the buffers every block (never allocates).
    in_ptrs: Box<[*mut f32]>,
    out_ptrs: Box<[*mut f32]>,
    aux_ptrs: Box<[*mut f32]>,
    /// Retained unknown state, as a wrapper keeps it.
    store: Mutex<StateStore>,
    /// Reused for converted events (the `Instance` path never allocates
    /// once warm).
    events: Vec<Event>,
}

/// How to build a [`PluginHarness`]: rate, block size, layout and the
/// parameter values to start from. `PluginHarness::new(rate, block)` is
/// the defaults with those two set.
pub struct HarnessBuilder<P: Plugin> {
    sample_rate: f32,
    block_size: usize,
    layout: Layout,
    values: Vec<(String, f64)>,
    _plugin: core::marker::PhantomData<fn() -> P>,
}

impl<P: Plugin> HarnessBuilder<P> {
    /// Hz (default 48 000).
    pub fn sample_rate(mut self, sample_rate: f32) -> Self {
        self.sample_rate = sample_rate;
        self
    }
    /// Frames per block (default 256).
    pub fn block_size(mut self, frames: usize) -> Self {
        self.block_size = frames;
        self
    }
    /// One of `P::LAYOUTS` (default the first).
    ///
    /// # Panics
    /// At [`build`](Self::build), if `P` does not declare it.
    pub fn layout(mut self, layout: Layout) -> Self {
        self.layout = layout;
        self
    }
    /// Start parameter `name` (any case) at `value`, in its own units,
    /// restored before `prepare` (so nothing glides in).
    pub fn set(mut self, name: &str, value: f64) -> Self {
        self.values.push((name.to_string(), value));
        self
    }
    pub fn build(self) -> PluginHarness<P> {
        assert!(P::LAYOUTS.contains(&self.layout), "{} does not declare the layout {:?}", P::INFO.name, self.layout);
        let telemetry = Telemetry::new();
        let shell = <Shell<P> as Erased>::new(&telemetry);
        let mut h = PluginHarness {
            shell,
            telemetry,
            sample_rate: self.sample_rate,
            block_size: 0,
            layout: self.layout,
            transport: None,
            input_buffers: Vec::new(),
            output_buffers: Vec::new(),
            aux_buffers: Vec::new(),
            in_ptrs: Box::new([]),
            out_ptrs: Box::new([]),
            aux_ptrs: Box::new([]),
            store: Mutex::new(StateStore::default()),
            events: Vec::with_capacity(256),
        };
        for (name, value) in &self.values {
            h.param(name.as_str()).restore(*value);
        }
        h.prepare(self.sample_rate, self.block_size);
        h
    }
}

/// A parameter a test names: by [`ParamId`] or by name (any case).
pub trait ParamName {
    fn lookup<'t>(&self, table: &'t dyn Params) -> Option<&'t dyn AnyParam>;
}

impl ParamName for ParamId {
    fn lookup<'t>(&self, table: &'t dyn Params) -> Option<&'t dyn AnyParam> {
        table.by_id(*self)
    }
}

impl ParamName for &str {
    fn lookup<'t>(&self, table: &'t dyn Params) -> Option<&'t dyn AnyParam> {
        (0..table.count()).filter_map(|i| table.by_index(i)).find(|p| p.meta().name.eq_ignore_ascii_case(self))
    }
}

impl<P: Plugin> PluginHarness<P> {
    /// Build the plugin on its preferred layout, allocate buffers, and
    /// prepare it at `sample_rate` for blocks of `block_size`.
    pub fn new(sample_rate: f32, block_size: usize) -> Self {
        Self::builder().sample_rate(sample_rate).block_size(block_size).build()
    }

    /// Choose the rate, block size, layout and starting values.
    pub fn builder() -> HarnessBuilder<P> {
        HarnessBuilder {
            sample_rate: 48_000.0,
            block_size: 256,
            layout: P::LAYOUTS[0],
            values: Vec::new(),
            _plugin: core::marker::PhantomData,
        }
    }

    /// Prepare again at a new rate and block size (buffers resized, the
    /// plugin's `prepare` called), as a host does between sessions.
    pub fn prepare(&mut self, sample_rate: f32, block_size: usize) {
        let l = self.layout;
        let chans = |n: usize| vec![vec![0.0; block_size].into_boxed_slice(); n];
        self.sample_rate = sample_rate;
        self.block_size = block_size;
        self.input_buffers = chans(l.input_channels());
        self.output_buffers = chans(l.output_channels());
        self.aux_buffers = l.aux.iter().take(MAX_AUX).map(|a| chans(a.channels.count())).collect();
        self.in_ptrs = vec![core::ptr::null_mut(); l.input_channels()].into_boxed_slice();
        self.out_ptrs = vec![core::ptr::null_mut(); l.output_channels()].into_boxed_slice();
        let aux_chans = self.aux_buffers.iter().map(Vec::len).sum();
        self.aux_ptrs = vec![core::ptr::null_mut(); aux_chans].into_boxed_slice();
        self.shell.prepare(&Environment::new(sample_rate, block_size).with_layout(l));
    }

    /// The plugin's `reset` (a host's transport jump or re-activation).
    pub fn reset(&mut self) {
        self.shell.reset();
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }
    pub fn block_size(&self) -> usize {
        self.block_size
    }
    pub fn layout(&self) -> Layout {
        self.layout
    }
    pub fn output_channels(&self) -> usize {
        self.output_buffers.len()
    }
    /// Number of input channels the layout declares.
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
    /// The latency the plugin reports, in samples.
    pub fn latency(&self) -> u32 {
        self.shell.latency()
    }
    pub fn tail(&self) -> stave::Tail {
        self.shell.tail()
    }

    fn param(&self, name: impl ParamName) -> &dyn AnyParam {
        let table: &dyn Params = &**self.shell.params();
        name.lookup(table).unwrap_or_else(|| panic!("{}: no such parameter", P::INFO.name))
    }

    /// Set a parameter as automation does (smoothed), in its own units.
    pub fn set(&mut self, name: impl ParamName, value: f64) {
        self.param(name).set_value(value);
    }

    /// A parameter's value, in its own units.
    pub fn value(&self, name: impl ParamName) -> f64 {
        self.param(name).value()
    }

    /// The transport every following block sees (`None`: no host clock).
    pub fn set_transport(&mut self, transport: Option<Transport>) {
        self.transport = transport;
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

    /// Channel `channel` of aux bus `bus` (a sidechain), writable.
    pub fn aux_mut(&mut self, bus: usize, channel: usize) -> &mut [f32] {
        &mut self.aux_buffers[bus][channel]
    }

    /// Aux buses the layout declares (up to [`MAX_AUX`]).
    pub fn aux_buses(&self) -> usize {
        self.aux_buffers.len()
    }

    /// The last block's output, one slice per channel.
    pub fn outputs(&self) -> &[Box<[f32]>] {
        &self.output_buffers
    }

    /// Process one block of `block_size` frames in place, allocation free
    /// (the no-alloc and perf checks run it under their guard and timer);
    /// read it with [`outputs`](Self::outputs).
    pub fn process(&mut self, events: &[Event]) {
        self.process_with(&[], events);
    }

    /// [`process`](Self::process) with sample-accurate parameter changes
    /// (plain values, ordered by frame): each change's last value is also
    /// written to the table, as the wrappers do.
    pub fn process_with(&mut self, changes: &[ParamChange], events: &[Event]) {
        self.process_frames(self.block_size, changes, events);
    }

    /// Process the first `frames` (≤ `block_size`, 0 allowed) of the
    /// buffers: hosts send short and empty blocks.
    pub fn process_frames(&mut self, frames: usize, changes: &[ParamChange], events: &[Event]) {
        assert!(frames <= self.block_size, "{frames} frames in blocks of {}", self.block_size);
        for c in changes.iter().filter(|c| c.kind == ParamChangeKind::Value) {
            if let Some(p) = self.shell.params().by_id(c.id) {
                p.set_value(c.value);
            }
        }
        let input = InputBuffer::from_slices(&mut self.in_ptrs, self.input_buffers.iter().map(|c| &**c), frames);
        let output = OutputBuffer::from_slices(&mut self.out_ptrs, self.output_buffers.iter_mut().map(|c| &mut **c), frames);
        let mut aux: [InputBuffer<'_>; MAX_AUX] = core::array::from_fn(|_| InputBuffer::empty());
        let mut slots: &mut [*mut f32] = &mut self.aux_ptrs;
        for (bus, chans) in self.aux_buffers.iter().enumerate() {
            let (head, rest) = core::mem::take(&mut slots).split_at_mut(chans.len());
            slots = rest;
            aux[bus] = InputBuffer::from_slices(head, chans.iter().map(|c| &**c), frames);
        }
        let mut block = Block::new(self.sample_rate, frames, input, output)
            .with_aux(&aux[..self.aux_buffers.len()])
            .with_transport(self.transport)
            .with_params(ParamBlock::new(changes))
            .with_events(events);
        // Zeroed every block: instruments overwrite, effects may not.
        block.output().silence();
        self.shell.process(&mut block);
    }

    /// Process `frames` frames in successive blocks, returning every
    /// output channel. `events` arrive in the first block; later blocks
    /// see none. Inputs keep what they hold. For sample-accurate timing
    /// use [`process_with`](Self::process_with).
    pub fn run(&mut self, frames: usize, events: &[Event]) -> Vec<Vec<f32>> {
        let mut acc: Vec<Vec<f32>> = vec![Vec::with_capacity(frames); self.output_channels()];
        let mut written = 0;
        let mut first = true;
        while written < frames {
            let take = (frames - written).min(self.block_size);
            let block_events: &[Event] = if first { events } else { &[] };
            first = false;
            self.process_frames(take, &[], block_events);
            for (ch, out) in self.output_buffers.iter().enumerate() {
                acc[ch].extend_from_slice(&out[..take]);
            }
            written += take;
        }
        acc
    }

    /// The plugin's state as a host saves it: parameters and the
    /// plugin's own bytes.
    pub fn save(&self) -> Vec<u8> {
        let extra = self.shell.save();
        let table = self.table();
        state::save(&*table, P::STATE_VERSION, &extra, &self.store.lock().unwrap_or_else(|p| p.into_inner()))
    }

    /// Load a saved state; on an error nothing changes.
    pub fn load(&mut self, bytes: &[u8]) -> Result<(), stave_core::state::StateError> {
        let table = self.table();
        let (report, extra) = state::load(
            bytes,
            &*table,
            &<Shell<P> as Erased>::migrations(),
            &mut self.store.lock().unwrap_or_else(|p| p.into_inner()),
        )?;
        self.shell.load(&extra, report.plugin_version);
        Ok(())
    }
}

impl<P: Plugin> PluginHarness<P> {
    pub(crate) fn take_events(&mut self) -> Vec<Event> {
        core::mem::take(&mut self.events)
    }
    pub(crate) fn put_events(&mut self, events: Vec<Event>) {
        self.events = events;
    }
}

/// A change of parameter `id` to `value` (plain) at `frame` of a block.
pub fn change(id: ParamId, frame: usize, value: f64) -> ParamChange {
    ParamChange { id, frame, value, kind: ParamChangeKind::Value }
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
            h.process(&[]);
            for c in 0..2 {
                assert!(h.outputs()[c]
                    .iter()
                    .all(|&y| y == 2.0 * (block * 10 + c) as f32));
            }
            h.input_mut(1).fill(-1.0);
            h.set_input(0, &[0.5; 16]);
            h.process(&[]);
            assert!(h.outputs()[0].iter().all(|&y| y == 1.0));
            assert!(h.outputs()[1].iter().all(|&y| y == -2.0));
        }
    }
}
