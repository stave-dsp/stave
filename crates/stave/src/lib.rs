//! # stave — write an audio plugin
//!
//! One dependency, one import, one trait, one macro:
//!
//! ```
//! use stave::prelude::*;
//!
//! #[derive(Params)]
//! pub struct GainParams {
//!     pub gain: FloatParam,
//!     pub bypass: BoolParam,
//! }
//!
//! impl Default for GainParams {
//!     fn default() -> Self {
//!         GainParams {
//!             gain: FloatParam::new(ParamId(1), "Gain", Range::linear(-24.0, 24.0))
//!                 .unit(Unit::Db)
//!                 .smooth(Smooth::ms(10.0)),
//!             bypass: BoolParam::bypass(ParamId(2), "Bypass"),
//!         }
//!     }
//! }
//!
//! pub struct Gain {
//!     params: Arc<GainParams>,
//!     gain: Smoother,
//! }
//!
//! impl Plugin for Gain {
//!     const INFO: Info = Info { id: "com.example.gain", name: "Gain", vendor: "Example", ..Info::EFFECT };
//!     type Params = GainParams;
//!
//!     fn new(params: &Arc<GainParams>, _telemetry: &Telemetry) -> Self {
//!         Gain { params: params.clone(), gain: Smoother::new(&params.gain) }
//!     }
//!
//!     fn prepare(&mut self, env: &Environment) {
//!         self.gain.prepare(env);
//!     }
//!
//!     fn process(&mut self, block: &mut Block<'_>) {
//!         let mut gain = self.gain.ramp(block, &self.params.gain);
//!         block.lanes::<2>().run(|frame| {
//!             let g = db_to_amp(gain.next());
//!             for lane in 0..frame.lanes() {
//!                 frame.write(lane, frame.input(lane) * g);
//!             }
//!         });
//!     }
//! }
//!
//! stave::export!(Gain);
//! ```
//!
//! A plugin with no parameters names [`NoParams`](params::NoParams):
//!
//! ```
//! use stave::prelude::*;
//!
//! pub struct Invert;
//!
//! impl Plugin for Invert {
//!     const INFO: Info = Info { id: "com.example.invert", name: "Invert", vendor: "Example", ..Info::EFFECT };
//!     type Params = NoParams;
//!
//!     fn new(_params: &Arc<NoParams>, _telemetry: &Telemetry) -> Self {
//!         Invert
//!     }
//!
//!     fn process(&mut self, block: &mut Block<'_>) {
//!         let (input, output) = block.io();
//!         for (x, y) in input.channels().zip(output.channels_mut()) {
//!             for (x, y) in x.iter().zip(y) {
//!                 *y = -x;
//!             }
//!         }
//!     }
//! }
//!
//! stave::export!(Invert);
//! ```
//!
//! `stave::export!` makes the crate (a `cdylib`) a VST3, a CLAP plugin
//! and (on macOS) an Audio Unit at once. The framework owns everything around `process`: bypass
//! (click-free, latency matched), parameter automation and state,
//! presets, the face's window, denormals, panics at the host boundary.
//!
//! The pieces, when you need more than the prelude:
//! [`params`], [`dsp`] (the DSP library), [`face`] (the design system),
//! [`telemetry`] (plugin → face values).

#![forbid(unsafe_code)]
#![deny(rust_2018_idioms)]

use std::sync::Arc;

// The derives name `::stave::__core`; inside this crate that is `crate`.
extern crate self as stave;

/// The model crate, for the derives' generated paths.
#[doc(hidden)]
pub use stave_core as __core;

pub mod route;
mod voice;

pub use stave_core::{
    block, budget, buffer, controller, cost_taps, diagnostics, environment, event, info, params, preset,
    process, solver_taps, state, telemetry, units,
};
pub use stave_core::{
    AuCodes, Aux, Block, Budget, Channels, Controller, CostTapIds, CostTaps, Environment, Event, EventCursor,
    EventKind, Feature, Info,
    InputBuffer, LaneFrame, Lanes, Layout, NoteEvent, NoteKind, Normalized, OutputBuffer, ParamId,
    Peaks, Preset, SolverTapIds, SolverTaps, Tail, Tap, TapId, Telemetry, Transport,
};
pub use stave_core::{brk_if, cost_tap_ids, param_enum, probe, solver_tap_ids};
/// `#[derive(Params)]`: the parameter table of a struct of params.
pub use stave_macros::StaveParams as Params;
/// `#[derive(Smoothers)]`: a group of [`Smoother`](params::Smoother)s
/// built, prepared and ramped together, one per param of the same name.
///
/// ```
/// use stave::prelude::*;
///
/// #[derive(Params)]
/// pub struct DelayParams {
///     pub time: FloatParam,
///     pub mix: FloatParam,
/// }
///
/// /// One frame of the knobs, as the DSP reads them.
/// pub struct Knobs {
///     pub time: f32,
///     pub mix: f32,
/// }
///
/// #[derive(Smoothers)]
/// #[smoothers(params = DelayParams, frame = Knobs)]
/// pub struct Glides {
///     time: Smoother,
///     mix: Smoother,
/// }
///
/// let params = DelayParams {
///     time: FloatParam::new(ParamId(1), "time", Range::linear(1.0, 500.0)).smooth(Smooth::ms(40.0)),
///     mix: FloatParam::new(ParamId(2), "mix", Range::linear(0.0, 1.0)).smooth(Smooth::ms(10.0)),
/// };
/// let mut glides = Glides::new(&params);
/// glides.prepare(&Environment::new(48_000.0, 64));
/// // In `process`: `let mut ramps = glides.ramp(block, &params);`, then
/// // per frame `ramps.next()` (a `Knobs`) or `ramps.time.next()`.
/// ```
pub use stave_macros::StaveSmoothers as Smoothers;
/// `#[derive(Taps)]`: a telemetry table of a struct of taps.
pub use stave_macros::StaveTaps as Taps;
pub use voice::ToVoiceEvent;

/// The DSP library: stages, filters, delays, oscillators, circuits.
pub use stave_dsp as dsp;
/// The design system: parts, themes, layout.
pub use stave_face as face;
pub use stave_face::Face;

use stave_core::params::Params as ParamTable;
use stave_core::state::Migrations;

/// An audio plugin: the one trait an author implements.
///
/// The framework builds `Self::Params` (its `Default`), then the plugin
/// with [`new`](Self::new); [`prepare`](Self::prepare)s it for the host's
/// conditions; and calls [`process`](Self::process) once per block on
/// the audio thread. Everything else has a default.
pub trait Plugin: Sized + Send + 'static {
    /// Identity: id (forever), name, vendor, kind.
    const INFO: Info;
    /// The supported bus layouts, preferred first; never empty.
    const LAYOUTS: &'static [Layout] = &[Layout::STEREO];
    /// Bumped when saved state changes shape; `migrations` maps old to new.
    const STATE_VERSION: u32 = 0;
    /// What one channel may cost at 48 kHz in the worst parameter state
    /// ([`Budget`]); `plugin_checks!`'s release perf check fails over it.
    /// Undeclared, the check measures and reports and never fails.
    const CPU_BUDGET: Budget = Budget::UNDECLARED;
    /// The parameter table (`#[derive(Params)]`), built by its `Default`.
    type Params: ParamTable + Default + 'static;

    /// Build the plugin: its smoothers and steps from `params`, its taps
    /// on `telemetry`. May allocate.
    fn new(params: &Arc<Self::Params>, telemetry: &Telemetry) -> Self;

    /// Derive everything that depends on the sample rate, block size or
    /// layout. May allocate; called again whenever they change.
    fn prepare(&mut self, _env: &Environment) {}

    /// Clear DSP state (delay lines, filters, envelopes), not params.
    /// Realtime.
    fn reset(&mut self) {}

    /// One block. Realtime: no allocation, no locks, no blocking.
    fn process(&mut self, block: &mut Block<'_>);

    /// Samples of delay the host compensates for.
    fn latency(&self) -> u32 {
        0
    }

    /// How long output rings after input stops.
    fn tail(&self) -> Tail {
        Tail::None
    }

    /// The editor: a pure function of the params.
    fn face(_params: &Self::Params) -> Option<Face> {
        None
    }

    /// Factory presets, declared against the params.
    fn presets(_params: &Self::Params) -> Vec<Preset> {
        Vec::new()
    }

    /// How to load state saved at an older `STATE_VERSION`.
    fn migrations() -> Migrations {
        Migrations::default()
    }

    /// State beyond params (rare): bytes for the session.
    fn save(&self) -> Vec<u8> {
        Vec::new()
    }

    /// Restore `save`'s bytes, written at state `version`;
    /// params are already restored.
    fn load(&mut self, _bytes: &[u8], _version: u32) {}
}

/// Everything a plugin's source needs, in one import.
pub mod prelude {
    pub use std::sync::Arc;

    pub use crate::{
        Aux, Block, Budget, Channels, CostTapIds, CostTaps, Environment, Event, EventCursor, EventKind, Face, Feature, Info, Layout,
        NoteEvent, NoteKind, Params, ParamId, Plugin, Preset, Smoothers, Tail, Tap, TapId, Taps,
        Telemetry, ToVoiceEvent, Transport,
    };
    /// The table trait (`table.by_name(..)`), beside the derive.
    pub use stave_core::params::Params;
    pub use stave_core::{cost_tap_ids, param_enum};
    pub use stave_core::params::{
        AnyParam, BoolParam, EnumParam, FloatParam, ModalWatch, NoParams, ParamEnum, Quantity, Ramp,
        Range, Smooth, Smoother, Step, Steps, Unit, Watch,
    };
    pub use stave_core::state::Migrations;
    /// Quantities as types: every stave-dsp setter takes one, and a
    /// param converts into one with `value_as` (DSP.md U4).
    pub use stave_dsp::units::{db, hz, khz, ms, seconds, Db, Gain, Hz, Ratio, Seconds};
    pub use stave_dsp::{
        amp_to_db, db_to_amp, soft_clip, Biquad, DcBlocker, DelayLine, FilterSpec, Generator, Lfo,
        OnePole, Oversampled2x, PerChannel, Stage, Svf,
    };
    pub use stave_face::prelude::*;
}

/// A [`Plugin`] [`export!`] made a VST3, CLAP and Audio Unit plugin: the facts only
/// the exporting crate knows. `export!` implements it; an author never
/// does.
pub trait Exported: Plugin {
    /// The exporting crate's `CARGO_PKG_VERSION`.
    const CRATE_VERSION: &'static str;
    /// VST3 class ids `[component, controller]`, when they must stay
    /// what an earlier build shipped; `None` derives them from
    /// `INFO.id`.
    const VST3_CLASS_IDS: Option<[[u32; 4]; 2]> = None;
}

/// What hosts key a plugin on: saved sessions, presets and automation
/// find it again by these, so a shipped plugin's identity never
/// changes. Read it with [`identity`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    /// The CLAP plugin id (`Info::id`).
    pub clap: &'static str,
    /// The VST3 component class id, as the 16 bytes hosts print in hex
    /// (the same on every platform).
    pub vst3_component: [u8; 16],
    /// The VST3 edit-controller class id.
    pub vst3_controller: [u8; 16],
    /// The Audio Unit's type, subtype and manufacturer codes (`aufx`,
    /// `aumu` or `aumf` first), as `auval -v` takes them.
    pub au: [[u8; 4]; 3],
}

/// The identity [`export!`] gives plugin `P`: its CLAP id, VST3 class
/// ids (derived from `INFO.id` or pinned by `export!`'s
/// `vst3_class_ids`) and Audio Unit codes (derived from `INFO.vendor`
/// and `INFO.id`, or pinned by `INFO.au`). Pin it in a test, so a change that would orphan
/// every saved session fails the build:
///
/// ```ignore
/// let id = stave::identity::<Tilt>();
/// assert_eq!(id.clap, "com.example.tilt");
/// assert_eq!(id.vst3_component, [0x5A, 0x7F, /* … */]);
/// ```
pub const fn identity<P: Exported>() -> Identity {
    use __export::vst3::sys::canonical_tuid;
    Identity {
        clap: P::INFO.id,
        vst3_component: canonical_tuid(__export::class_id::<P>(0, "component")),
        vst3_controller: canonical_tuid(__export::class_id::<P>(1, "controller")),
        au: {
            let id = __export::au::component::Identity::from_info(&P::INFO, P::LAYOUTS);
            [id.kind, id.subtype, id.manufacturer]
        },
    }
}

/// The bridge from [`Plugin`] to the format wrappers. Not author API:
/// [`export!`] names it.
#[doc(hidden)]
pub mod __export {
    use super::*;
    use stave_core::params::IndexedParams;
    use stave_core::runtime::Erased;

    pub use stave_au as au;
    pub use stave_clap as clap;
    pub use stave_vst3 as vst3;

    pub use crate::Exported as Export;

    /// A [`Plugin`] with its params, table and presets: what the wrappers
    /// instantiate.
    pub struct Shell<P: Plugin> {
        plugin: P,
        params: Arc<P::Params>,
        table: Arc<dyn ParamTable>,
        presets: Arc<[Preset]>,
    }

    impl<P: Plugin> Shell<P> {
        pub fn plugin(&self) -> &P {
            &self.plugin
        }
        pub fn plugin_mut(&mut self) -> &mut P {
            &mut self.plugin
        }
        pub fn params(&self) -> &Arc<P::Params> {
            &self.params
        }
    }

    impl<P: Plugin> Erased for Shell<P> {
        const INFO: Info = P::INFO;
        const LAYOUTS: &'static [Layout] = P::LAYOUTS;
        const STATE_VERSION: u32 = P::STATE_VERSION;

        fn new(telemetry: &Telemetry) -> Self {
            let params = Arc::new(P::Params::default());
            let plugin = P::new(&params, telemetry);
            let table = Arc::new(IndexedParams::new(params.clone()));
            let presets = P::presets(&params).into();
            Shell {
                plugin,
                params,
                table,
                presets,
            }
        }
        fn params(&self) -> Arc<dyn ParamTable> {
            self.table.clone()
        }
        fn presets(&self) -> Arc<[Preset]> {
            self.presets.clone()
        }
        fn prepare(&mut self, env: &Environment) {
            self.plugin.prepare(env);
        }
        fn reset(&mut self) {
            self.plugin.reset();
        }
        fn process(&mut self, block: &mut Block<'_>) {
            self.plugin.process(block);
        }
        fn latency(&self) -> u32 {
            self.plugin.latency()
        }
        fn tail(&self) -> Tail {
            self.plugin.tail()
        }
        fn migrations() -> Migrations {
            P::migrations()
        }
        fn save(&self) -> Vec<u8> {
            self.plugin.save()
        }
        fn load(&mut self, bytes: &[u8], version: u32) {
            self.plugin.load(bytes, version);
        }
    }

    /// `INFO.version`, or the crate's when it is empty.
    const fn version<P: Export>() -> &'static str {
        if P::INFO.version.is_empty() {
            P::CRATE_VERSION
        } else {
            P::INFO.version
        }
    }

    pub(crate) const fn class_id<P: Export>(role: usize, name: &str) -> vst3::sys::Tuid {
        match P::VST3_CLASS_IDS {
            Some(ids) => {
                let [a, b, c, d] = ids[role];
                vst3::sys::make_tuid(a, b, c, d)
            }
            None => vst3::ids::derive_cid(P::INFO.id, name),
        }
    }

    impl<P: Export> vst3::wrapper::Vst3Export for Shell<P> {
        const COMPONENT_CID: vst3::sys::Tuid = class_id::<P>(0, "component");
        const CONTROLLER_CID: vst3::sys::Tuid = class_id::<P>(1, "controller");
        const VERSION: &'static str = version::<P>();
        fn face(&self) -> Option<Face> {
            P::face(&self.params)
        }
    }

    impl<P: Export> clap::wrapper::ClapExport for Shell<P> {
        const VERSION: &'static str = version::<P>();
        fn face(&self) -> Option<Face> {
            P::face(&self.params)
        }
    }

    impl<P: Export> au::AuExport for Shell<P> {
        const VERSION: &'static str = version::<P>();
        fn face(&self) -> Option<Face> {
            P::face(&self.params)
        }
    }

    #[cfg(feature = "standalone")]
    pub use stave_standalone as standalone;

    #[cfg(feature = "standalone")]
    impl<P: Plugin> standalone::Standalone for Shell<P> {
        fn face(&self) -> Option<Face> {
            P::face(&self.params)
        }
    }
}

/// Run `$plugin` as its own app (the `standalone` feature): the face is
/// the window, the audio device the host. The whole of a plugin crate's
/// `src/main.rs`, beside its `src/lib.rs`:
///
/// ```ignore
/// stave::standalone!(stave_tilt_vst3::Tilt);
/// ```
///
/// with, in its `Cargo.toml`,
///
/// ```toml
/// [features]
/// standalone = ["stave/standalone"]
///
/// [[bin]]
/// name = "stave-tilt-vst3-standalone"
/// path = "src/main.rs"
/// required-features = ["standalone"]
/// ```
///
/// `scripts/package.sh` builds it into a macOS `.app`, a Windows `.exe`
/// and a Linux binary beside the plugin bundles.
#[cfg(feature = "standalone")]
#[macro_export]
macro_rules! standalone {
    ($plugin:ty $(,)?) => {
        fn main() {
            $crate::__export::standalone::run::<$crate::__export::Shell<$plugin>>();
        }
    };
}

/// Make this crate a VST3, a CLAP plugin and (on macOS) an Audio Unit of
/// `$plugin` (a [`Plugin`]). Once per `cdylib` crate.
///
/// ```ignore
/// stave::export!(Tilt);
/// ```
///
/// The VST3 class ids derive from `INFO.id`. A plugin that shipped with
/// other ids keeps them (each id four `u32`s, as in the VST3 SDK's
/// `DECLARE_CLASS_IID`):
///
/// ```ignore
/// stave::export!(Saga, vst3_class_ids: {
///     component: [0x7C31A2E4, 0x88B14D02, 0xA5E0C9D3, 0x11700001],
///     controller: [0x7C31A2E4, 0x88B14D02, 0xA5E0C9D3, 0x11700002],
/// });
/// ```
#[macro_export]
macro_rules! export {
    ($plugin:ty $(,)?) => {
        impl $crate::Exported for $plugin {
            const CRATE_VERSION: &'static str = ::core::env!("CARGO_PKG_VERSION");
        }
        $crate::export!(@entries $plugin);
    };
    ($plugin:ty, vst3_class_ids: {
        component: [$c1:expr, $c2:expr, $c3:expr, $c4:expr $(,)?],
        controller: [$e1:expr, $e2:expr, $e3:expr, $e4:expr $(,)?] $(,)?
    } $(,)?) => {
        impl $crate::Exported for $plugin {
            const CRATE_VERSION: &'static str = ::core::env!("CARGO_PKG_VERSION");
            const VST3_CLASS_IDS: ::core::option::Option<[[u32; 4]; 2]> =
                ::core::option::Option::Some([[$c1, $c2, $c3, $c4], [$e1, $e2, $e3, $e4]]);
        }
        $crate::export!(@entries $plugin);
    };
    (@entries $plugin:ty) => {
        $crate::__export::vst3::export_vst3!($crate::__export::Shell<$plugin>);
        $crate::__export::clap::export_clap!($crate::__export::Shell<$plugin>);
        $crate::__export::au::export_au!($crate::__export::Shell<$plugin>);
    };
}
