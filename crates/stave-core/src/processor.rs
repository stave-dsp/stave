//! The top-level plugin shape (user-specced): a stave plugin is TWO
//! configurable pieces — a [`Processor`] and an editor — plus static
//! declarations. Projects implement the narrow traits here;
//! [`StavePlugin`] is the framework shell that composes them into the
//! wide [`Plugin`] surface the ABI wrappers consume.
//!
//! Seams (JUCE's processor/editor split, improved):
//!  - `Processor` is REALTIME-ONLY: construct from the typed params,
//!    activate at a rate, process blocks. Nothing about descriptors,
//!    editors, or state formats.
//!  - The editor is a PURE FUNCTION of params (`Spec::face`) — no
//!    JUCE-style mutable pointer from the editor back into the
//!    processor; gestures flow through the framework.
//!  - `Spec` carries the declarations (descriptor, editor, presets,
//!    state versioning) as associated items — data, not state.
//!
//! The shell is also the optimization seam: it owns construction and
//! lifecycle, so allocation pooling, layout tricks, or processor
//! chains land HERE without touching any project.

extern crate alloc;
use alloc::sync::Arc;
use alloc::vec::Vec;

use stave_debug::DebugBus;

use crate::environment::Environment;
use crate::params::{IndexedParams, Params};
use crate::plugin::{Plugin, PluginDescriptor, PortConfig, Tail};
use crate::preset::Preset;
use crate::process::ProcessCtx;
use crate::state::Migrations;

/// The realtime half. Everything here may run on the audio thread —
/// no allocation, locks, or blocking in `process`/`reset`.
pub trait Processor: Send + 'static {
    /// The project's typed param struct (`#[derive(Params)]`).
    type Params: Params + Send + Sync + 'static;

    /// Build the engine. The params Arc is the ONLY input — every
    /// internal (oversamplers, delay lines, benches, watches) is the
    /// engine's own business, constructed here, never passed in.
    fn new(params: Arc<Self::Params>) -> Self;

    /// Prepare under the host's conditions (may allocate — message
    /// thread). The prepare cascade convention: derive EVERY
    /// coefficient from `env` here, and pass `env` (or
    /// [`Environment::region`] of it) down to every stage — a
    /// re-activate at a new rate must fully re-derive.
    fn activate(&mut self, _env: Environment) {}
    fn deactivate(&mut self) {}

    /// Realtime.
    fn process(&mut self, ctx: &mut ProcessCtx<'_>);

    /// Clear DSP state without touching params.
    fn reset(&mut self) {}

    fn latency_samples(&self) -> u32 {
        0
    }
    fn tail(&self) -> Tail {
        Tail::None
    }

    /// Extra non-param state for the session chunk (rare).
    fn save_extra(&self) -> Vec<u8> {
        Vec::new()
    }
    fn load_extra(&mut self, _bytes: &[u8], _from_version: u32) {}

    /// Optional telemetry seam: the host-owned bus arrives after
    /// construction; engines that publish probes/captures stash it.
    fn attach_telemetry(&mut self, _bus: Arc<DebugBus>) {}
}

/// The declarations half — associated data, no instance state.
pub trait Spec: Processor {
    const DESCRIPTOR: PluginDescriptor;
    const CONFIGS: &'static [PortConfig] = &[PortConfig::STEREO_FX];

    /// Construct the param table (identity lives in params.rs).
    fn params() -> Arc<Self::Params>;

    /// The editor as a design-system face — a pure function of params.
    fn face(_params: &Self::Params) -> Option<stave_face::Face> {
        None
    }
    /// Factory presets — pure function of params (presets.rs).
    fn presets(_params: &Self::Params) -> Vec<Preset> {
        Vec::new()
    }

    fn state_version() -> u32 {
        0
    }
    fn migrations() -> Migrations {
        Migrations::default()
    }
}

/// The framework shell: `Spec + Processor` in, [`Plugin`] out.
/// Projects export `StavePlugin<TheirEngine>` and write no glue.
pub struct StavePlugin<E: Spec> {
    params: Arc<E::Params>,
    /// `params` behind an id → index table, built once here (instance
    /// creation) so the wrappers' audio-thread lookups never scan.
    table: Arc<dyn Params>,
    engine: E,
}

impl<E: Spec> StavePlugin<E> {
    /// The composed engine — for tests and advanced hosts. The shell
    /// composes; it does not hide.
    pub fn engine(&self) -> &E {
        &self.engine
    }
    pub fn engine_mut(&mut self) -> &mut E {
        &mut self.engine
    }
}

impl<E: Spec> Plugin for StavePlugin<E> {
    const DESCRIPTOR: PluginDescriptor = E::DESCRIPTOR;
    const CONFIGS: &'static [PortConfig] = E::CONFIGS;

    fn new() -> Self {
        let params = E::params();
        let engine = E::new(params.clone());
        let table = Arc::new(IndexedParams::new(params.clone()));
        Self {
            params,
            table,
            engine,
        }
    }

    fn param_table(&self) -> Arc<dyn Params> {
        self.table.clone()
    }
    fn editor_face(&self) -> Option<stave_face::Face> {
        E::face(&self.params)
    }
    fn presets(&self) -> Vec<Preset> {
        E::presets(&self.params)
    }

    fn activate(&mut self, sample_rate: f32, max_frames: usize) {
        self.engine
            .activate(Environment::new(sample_rate, max_frames));
    }
    fn deactivate(&mut self) {
        self.engine.deactivate();
    }
    fn process(&mut self, ctx: &mut ProcessCtx<'_>) {
        self.engine.process(ctx);
    }
    fn reset(&mut self) {
        self.engine.reset();
    }
    fn latency_samples(&self) -> u32 {
        self.engine.latency_samples()
    }
    fn tail(&self) -> Tail {
        self.engine.tail()
    }

    fn state_version() -> u32 {
        E::state_version()
    }
    fn save_extra(&self) -> Vec<u8> {
        self.engine.save_extra()
    }
    fn load_extra(&mut self, bytes: &[u8], from_version: u32) {
        self.engine.load_extra(bytes, from_version);
    }
    fn migrations() -> Migrations {
        E::migrations()
    }

    fn debug_attach(&mut self, bus: Arc<DebugBus>) {
        self.engine.attach_telemetry(bus);
    }
}
