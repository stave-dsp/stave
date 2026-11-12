//! stave-dsp — the DSP vocabulary stave plugins are built from.
//!
//! One contract: every processor is a [`Stage`] — `prepare(sample_rate)`
//! caches rate-derived coefficients, `reset` clears state, `tick` advances
//! one sample, `process` runs a block, and `latency`/[`Tail`] report delay
//! and ring-out. Setters take musical units (`_hz`, `_ms`, `_db`, `q`);
//! phase is in cycles; sample rates are `f32`.
//!
//! Five layers, each depending only on the ones before it
//! (`scripts/check-view-bindings.sh` enforces it):
//!
//! 0. [`kernel`] — the contract and the forms graphs are built from:
//!    [`Serial`], [`Par2`], [`Loop`], [`Oversampled2x`], [`PerChannel`],
//!    control routing ([`Router`]) and voices ([`VoiceRack`]).
//! 1. [`primitives`] — linear building blocks: filters, delays, control
//!    generators and followers, oscillators.
//! 2. [`behavioral`] — models fitted to measurement or chosen by ear:
//!    tabulated curves with ADAA, saturators, slew, hysteresis.
//! 3. [`circuit`] — models derived from schematics: devices, solvers and
//!    topologies verified against ngspice.
//! 4. [`structures`] — reusable compositions: [`Chain`], [`ModMatrix`].
//!
//! Constraints: no allocation on the audio path, no panics in `tick` or
//! `process`. The crate root re-exports the public vocabulary flat.

#![forbid(unsafe_code)]

pub mod behavioral;
pub mod circuit;
pub mod kernel;
pub mod primitives;
pub mod structures;

pub use behavioral::shapes;

// Layer 0 — kernel.
pub use kernel::control::{Full, Route, Router, Source, StageSource};
pub use kernel::feedback::Loop;
pub use kernel::lanes::{downmix_51, ms_decode, ms_encode, ChannelLayout, PerChannel};
pub use kernel::node::{Stage, Tail};
pub use kernel::oversampled::Oversampled2x;
pub use kernel::parallel::{Compensator, Par2};
pub use kernel::sequence::Serial;
pub use kernel::voices::{Steal, Voice, VoiceEvent, VoiceRack};

// Layer 1 — primitives.
pub use primitives::control::adsr::{Adsr, AdsrConfig};
pub use primitives::control::envelope::{EnvelopeFollower, EnvelopeTimes};
pub use primitives::control::lag::Lag;
pub use primitives::control::lfo::Lfo;
pub use primitives::control::mod_lfo::{mod_lfo_hz, rise, ModLfo, ModLfoShape, SYNC_BEATS};
pub use primitives::control::note_sources::{
    key_position, standard_sources, sympathy, Ensemble, NoteSources,
};
pub use primitives::control::random_walk::RandomWalk;
pub use primitives::control::slot::RouteSlot;
pub use primitives::control::smoothed::SmoothedParam;
pub use primitives::control::sub::SubOctave;
pub use primitives::filters::allpass::Allpass1;
pub use primitives::filters::biquad::{Biquad, FilterSpec};
pub use primitives::filters::linkwitz_riley::{Crossover3, CrossoverSide, LinkwitzRiley4};
pub use primitives::filters::one_pole::{tau_coeff, DcBlocker, OnePole, OnePoleMode};
pub use primitives::filters::svf::{Svf, SvfTaps};
pub use primitives::gain::{amp_to_db, db_to_amp, t60_gain};
pub use primitives::osc::{Osc, Waveform};
pub use primitives::pitch::hz_of_key;
pub use primitives::random::Xorshift32;
pub use primitives::time::comb::{Comb, DelayAllpass};
pub use primitives::time::delay::DelayLine;

// Layer 2 — behavioral.
pub use behavioral::adaa::{Adaa, AdaaOrder};
pub use behavioral::curve::{AdaaShape, Curve, CurveStage, Frame, Framed};
pub use behavioral::hysteresis::Hysteresis;
pub use behavioral::rc::{NonlinearRc, RcClipperConfig, RcTopology};
pub use behavioral::shapes::soft_clip;
pub use behavioral::slew::Slew;
pub use behavioral::smooth_sat::SmoothSat;

// Layer 3 — circuit.
pub use circuit::audio::{AudioAdapter, AudioAdapterConfig, Circuit};
pub use circuit::devices::diode::Shockley;
pub use circuit::devices::magnetic::{JilesAtherton, JilesAthertonCore};
pub use circuit::devices::triode::dempwolf::Dempwolf;
pub use circuit::devices::triode::koren::Koren;
pub use circuit::devices::triode::{TriodeCurrents, TriodeModel, Tube};
pub use circuit::methods::newton::{NewtonSettings, NewtonStats, Solved};
pub use circuit::topologies::common_cathode::{CommonCathode, CommonCathodeConfig};
pub use circuit::topologies::coupler::Coupler;
pub use circuit::topologies::diode_clipper::{DiodeClipper, DiodeClipperConfig, PairSolver};
pub use circuit::topologies::opamp_clipper::{OpAmpClipper, OpAmpClipperConfig};
pub use circuit::topologies::power_supply::{PowerSupply, PowerSupplyConfig, SupplyFilter};
pub use circuit::topologies::push_pull::PushPull;
pub use circuit::topologies::tone_stack::{
    PassiveToneControlPositions, PassiveToneStack, PassiveToneStackComponents,
};
pub use circuit::topologies::transformer::{Transformer, TransformerConfig};
pub use circuit::topologies::triode_stage::{Bias, LevelPolicy, Triode, TriodeParams};

// Layer 4 — structures.
pub use structures::chain::{Chain, StageId, StageKind, SwapPoint};
pub use structures::modmatrix::{Driver, ModMatrix, StageTarget, Target};
