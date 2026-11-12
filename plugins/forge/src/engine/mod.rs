//! The Forge ENGINE — the CANONICAL Stave engine: every piece is a
//! named kernel form or a declared plane, composed. Testable without
//! any plugin ABI; `lib.rs` is the declarations half.
//!
//! The map, form by form:
//! * Audio path = `Oversampled2x::front(Oversampled2x::nested(Chain<6>))` per
//!   lane — Region owns the rate contract and the 66+18 latency
//!   arithmetic; Chain is the Sequence structure whose interiors are
//!   sense endpoints.
//! * Lanes = `PerChannel<Strip>` in the M/S domain; the lift is the
//!   kernel adapter pair (`ms_encode`/`ms_decode`), width scales the
//!   side lane.
//! * Control = `ModMatrix` (a kernel `Router` instance): detectors +
//!   LFO + divider as Sources, routes as data, displace-from-base at
//!   every inlet. Param bindings are `Watch`/`ModalWatch` sets.
//! * Structure changes = `SwapPoint` benches (moves, never allocs);
//!   dry path = `Compensator` on the latency laws.
//! * Observation = declared `ForgeTaps` + the framework `Scope`; the
//!   topology strip derives from `Chain::topology()`.
//! * `run()` is bind → render → observe, seven lines.
//!
//! Files are named for the interfaces they define — the types make
//! the composition obvious; the file names just say where to look:
//! * `strip.rs` — `Strip`, one M/S lane: topology + region + dry pad.
//! * `bindings.rs` — `Bindings::apply_changes`: params → structure.
//! * `modulation.rs` — `Modulation`: drivers, routes, displacement.
//! * `process.rs` — `process_block`, the audio block (the
//!   `Processor::process` contract — not a UI concern).
//! * `telemetry.rs` — `Telemetry::publish`: taps, scope, editor feeds.
//! * here — the component struct and `process`, the orchestration.

extern crate alloc;
use alloc::sync::Arc;
use std::time::Instant;

use stave_core::{DebugBus, ProcessCtx};
use stave_dsp::{Oversampler2xFir, Oversampler2xFirInner, PerChannel, Stage};

use crate::params::ForgeParams;

mod bindings;
mod modulation;
mod process;
mod strip;
mod telemetry;
pub(crate) mod voicing;

use bindings::Bindings;
use modulation::Modulation;
use strip::Strip;
pub use telemetry::ForgeTaps;
use telemetry::Telemetry;

const FORGE_LATENCY: usize =
    Oversampler2xFir::latency_samples() + Oversampler2xFirInner::latency_samples_base(); // 66 + 18 = 84

/// The engine is its COMPONENTS — read the fields, know the design;
/// `process` below is the whole per-block dance through them.
pub struct ForgeEngine {
    params: alloc::sync::Arc<ForgeParams>,
    debug: Option<Arc<DebugBus>>,
    sr: f32,
    /// The audio path: one Strip per M/S lane.
    strips: PerChannel<Strip>,
    /// Params bound to structure — watches + rebuild rules.
    bindings: Bindings,
    /// Drivers, routes, and the state they displace.
    modulation: Modulation,
    /// Everything reported out: taps, scope capture, editor feeds.
    telemetry: Telemetry,
    controls: process::Controls,
}

impl ForgeEngine {
    /// Prepare at rate/block (Plugin::activate delegates).
    fn boot(&mut self, sample_rate: f32, max_frames: usize) {
        self.sr = sample_rate;
        for strip in self.strips.iter_mut() {
            // The Region form owns the rate contract: one call, the
            // chain lands prepared at 4×.
            strip.region.prepare(sample_rate);
            strip.dry.prepare(FORGE_LATENCY as u32);
        }
        self.modulation.prepare(sample_rate);
        self.telemetry.prepare(max_frames);
        // Force full re-application after (re)activation.
        self.bindings.reset();
    }
}

impl stave_core::Processor for ForgeEngine {
    type Params = ForgeParams;

    fn new(params: Arc<ForgeParams>) -> Self {
        Self {
            controls: process::Controls::new(&params),
            params,
            debug: None,
            sr: 48_000.0,
            strips: PerChannel::new(|_| Strip::new()),
            bindings: Bindings::new(),
            modulation: Modulation::new(),
            telemetry: Telemetry::new(),
        }
    }

    fn activate(&mut self, env: stave_core::Environment) {
        self.boot(env.sample_rate, env.max_block);
        self.controls
            .prepare(f64::from(env.sample_rate), &self.params);
    }

    /// The per-block orchestration — the whole engine in three
    /// phases, each a named component call:
    fn process(&mut self, ctx: &mut ProcessCtx<'_>) {
        let t_block = Instant::now();
        let p = self.params.clone();
        // 1. Param changes become structure (only where knobs moved).
        self.bindings.apply_changes(
            &p,
            self.sr,
            &mut self.strips,
            &mut self.modulation,
            &mut self.telemetry,
            &self.debug,
        );
        // 2. The audio block: strips tick under the modulation plane.
        let (peak_in, peak_out) = process::process_block(
            ctx,
            &p,
            &mut self.controls,
            &mut self.strips,
            &mut self.modulation,
            &mut self.telemetry,
        );
        // 3. What happened, reported out (bus-gated; free unattached).
        self.telemetry.publish(
            &self.debug,
            &p,
            self.sr,
            telemetry::BlockReport {
                frames: ctx.frames,
                peak_in,
                peak_out,
                started: t_block,
            },
        );
    }

    fn latency_samples(&self) -> u32 {
        FORGE_LATENCY as u32
    }

    fn attach_telemetry(&mut self, bus: Arc<DebugBus>) {
        // The scope wants transfer PAIRS, not output L/R: ch0 = shaper-
        // domain input, ch1 = chain output (plugin-owned capture).
        bus.claim_capture();
        self.debug = Some(bus);
    }
}

impl ForgeEngine {
    /// The declared observation points — instruments attach here
    /// (handle in, transport at the wire only).
    pub fn taps(&self) -> &ForgeTaps {
        self.telemetry.taps()
    }
}

#[cfg(test)]
mod topology_tests {
    use super::strip::AmpChain;
    use super::*;

    /// law tie: the Region form computes the pinned latency.
    #[test]
    fn region_latency_matches_the_pinned_const() {
        let strip = Strip::new();
        assert_eq!(strip.region.latency_samples() as usize, FORGE_LATENCY);
    }

    /// Sidecar snapshot: the emitted topology data IS the built
    /// structure — this pin can only move when build_chain moves.
    #[test]
    fn declared_topology_matches_the_build() {
        let amp = AmpChain::new();
        let got: alloc::vec::Vec<_> = amp.as_chain().topology().collect();
        assert_eq!(
            got,
            alloc::vec![
                (amp.primary, "curve"),
                (amp.voicing, "filter"),
                (amp.second, "curve"),
                (amp.post, "filter"),
                (amp.grit, "slew"),
                (amp.iron, "hysteresis"),
            ]
        );
    }
}
