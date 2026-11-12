//! Shared DSP primitives for stave plugins.
//!
//! Anything reused across more than one plugin in the suite lives here.
//! Constraints: no allocation in the audio path, no panics in `process()`.
//! (We could go no_std + libm later, but plugin backends pull in std and
//! the f32 inherent math methods anyway, so the dep isn't worth it.)

#![forbid(unsafe_code)] // frozen guarantee: the DSP layer is 100% safe Rust
                        //
                        // Layout: `kernel/` = mathematical forms,
                        // `primitives/` = stateful building blocks, and `structures/` =
                        // reusable compositions. Product topologies remain in their
                        // project crates. Unit tests are colocated per module; the
                        // crate root re-exports the public vocabulary flat.
pub mod behavioral;
pub mod circuit;
pub mod kernel;
pub mod primitives;
pub mod structures;

pub use behavioral::shapes;

pub use behavioral::adaa::{Adaa, AdaaOrder};
pub use behavioral::curve::{AdaaShape, Curve, Framed};
pub use behavioral::curve::{CurveStage, Frame};
pub use behavioral::hysteresis::Hysteresis;
pub use behavioral::rc::{NonlinearRc, RcClipperConfig, RcTopology};
pub use behavioral::slew::Slew;
pub use behavioral::smooth_sat::SmoothSat;
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
pub use circuit::topologies::power_supply::{Filter, PowerSupply, PowerSupplyConfig};
pub use circuit::topologies::push_pull::PushPull;
pub use circuit::topologies::tone_stack::{
    PassiveToneControlPositions, PassiveToneStack, PassiveToneStackComponents,
};
pub use circuit::topologies::transformer::{Transformer, TransformerConfig};
pub use circuit::topologies::triode_stage::{Bias, LevelPolicy, Triode, TriodeParams};
pub use kernel::control::{Route as ControlRoute, Router, Source, StageSource};
pub use kernel::feedback::Loop;
pub use kernel::lanes::{downmix_51, ms_decode, ms_encode, ChannelLayout, PerChannel};
pub use kernel::node::Stage;
pub use kernel::oversampled::Oversampled2x;
pub use kernel::parallel::{Compensator, Par2};
pub use kernel::resample::{HalfbandFir, Oversampler2xFir, Oversampler2xFirInner};
pub use kernel::sequence::Serial;
pub use kernel::voices::{hz_of_key, Steal, Voice, VoiceEvent, VoiceRack};
pub use primitives::control::adsr::{Adsr, AdsrConfig};
pub use primitives::control::envelope::{EnvelopeFollower, EnvelopeTimes};
pub use primitives::control::lag::Lag;
pub use primitives::control::lfo::{Lfo, LfoShape};
pub use primitives::control::mod_lfo::{mod_lfo_hz, rise, ModLfo, ModLfoShape, SYNC_BEATS};
pub use primitives::control::note_sources::{
    key_position, standard_sources, sympathy, Ensemble, NoteSources,
};
pub use primitives::control::random_walk::RandomWalk;
pub use primitives::control::slot::RouteSlot;
pub use primitives::control::smoothed::SmoothedParam;
pub use primitives::control::sub::SubOctave;
pub use primitives::filters::allpass::Allpass1;
pub use primitives::filters::biquad::Biquad;
pub use primitives::filters::biquad::{FilterSpec, FilterStage};
pub use primitives::filters::linkwitz_riley::LinkwitzRiley4;
pub use primitives::filters::svf::{Svf, SvfTaps};
pub use primitives::gain::{amp_to_db, db_to_amp, t60_gain};
pub use primitives::osc::{Osc, OscShape};
pub use primitives::random::Xorshift32;
pub use primitives::resample::Oversampler2x;
pub use primitives::time::delay::DelayLine;
pub use structures::chain::StageKind;
pub use structures::chain::SwapPoint;
pub use structures::chain::{Chain, StageId};
pub use structures::modmatrix::{Driver, ModMatrix, Route, Target};
