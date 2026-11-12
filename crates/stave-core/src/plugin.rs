use core::ffi::CStr;

extern crate alloc;
use alloc::sync::Arc;

use stave_telemetry::Telemetry;

use crate::params::Params;
use crate::process::ProcessCtx;

/// What a plugin advertises to the host: identity + capabilities. All strings
/// are `&'static CStr` because every plugin ABI we target (CLAP, AU, VST3)
/// crosses an FFI boundary — keeping them nul-terminated at the source avoids
/// per-call allocation and lets us pass pointers straight through.
#[derive(Clone, Debug)]
pub struct PluginDescriptor {
    /// Reverse-DNS unique identifier, e.g. `c"com.ryanbahan.stave.sine"`.
    pub id: &'static CStr,
    pub name: &'static CStr,
    pub vendor: &'static CStr,
    pub version: &'static CStr,
    pub url: &'static CStr,
    pub description: &'static CStr,
    pub features: &'static [PluginFeature],
}

#[derive(Copy, Clone, Debug)]
pub enum PluginFeature {
    Instrument,
    AudioEffect,
    NoteEffect,
    Analyzer,
    Synthesizer,
    Sampler,
    Drum,
    Filter,
    Delay,
    Reverb,
    Distortion,
    Eq,
    Mixing,
    Mastering,
    Stereo,
    Mono,
}

impl PluginFeature {
    pub const fn as_clap_cstr(self) -> &'static CStr {
        match self {
            Self::Instrument => c"instrument",
            Self::AudioEffect => c"audio-effect",
            Self::NoteEffect => c"note-effect",
            Self::Analyzer => c"analyzer",
            Self::Synthesizer => c"synthesizer",
            Self::Sampler => c"sampler",
            Self::Drum => c"drum",
            Self::Filter => c"filter",
            Self::Delay => c"delay",
            Self::Reverb => c"reverb",
            Self::Distortion => c"distortion",
            Self::Eq => c"equalizer",
            Self::Mixing => c"mixing",
            Self::Mastering => c"mastering",
            Self::Stereo => c"stereo",
            Self::Mono => c"mono",
        }
    }
}

/// Post-input decay a host must render out.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Tail {
    /// Output stops with input (gain, EQ, saturation).
    None,
    /// Fixed decay (delays, reverbs with finite feedback).
    Samples(u32),
    /// Self-sustaining output (freeze modes, unbounded feedback).
    Infinite,
}

impl Tail {
    /// Wire encoding shared by VST3 `getTailSamples` and `clap.tail`:
    /// 0 = none, u32::MAX = infinite.
    pub fn to_samples(self) -> u32 {
        match self {
            Tail::None => 0,
            Tail::Samples(n) => n,
            Tail::Infinite => u32::MAX,
        }
    }
}

/// A host-negotiable speaker arrangement — what a PORT is, named
/// semantically (never a bare channel count). This
/// is the HOST-FACING vocabulary; the kernel's `ChannelLayout`
/// (stave-dsp) names internal lane SEMANTICS — hosts negotiate
/// speaker arrangements, engines lift them (Stereo → MidSide is an
/// explicit adapter, never a port type).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Arrangement {
    Mono,
    Stereo,
    /// L R C LFE Ls Rs, in that lane order (the VST3/ITU order).
    Surround51,
}

impl Arrangement {
    pub const fn channels(self) -> u32 {
        match self {
            Arrangement::Mono => 1,
            Arrangement::Stereo => 2,
            Arrangement::Surround51 => 6,
        }
    }
}

/// An auxiliary input port (sidechain): named, with its arrangement.
/// Aux ports are always OPTIONAL to connect — a plugin must be fully
/// functional with every aux silent/absent.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AuxIn {
    pub name: &'static str,
    pub arrangement: Arrangement,
}

/// ONE host-negotiable audio configuration: every port's arrangement
/// as a single tuple. The unit of negotiation is the WHOLE plugin —
/// cross-port coupling (main in matches main out, aux presence) is
/// inside the member, so no support-predicate exists for hosts to
/// probe (the JUCE `isBusesLayoutSupported` lesson; CLAP's
/// audio-ports-config got this right). `Plugin::CONFIGS` declares
/// the supported SET in preference order; hosts pick a member.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PortConfig {
    /// `None` = instrument (no audio in).
    pub main_in: Option<Arrangement>,
    pub main_out: Arrangement,
    /// Sidechains and other secondary inputs.
    pub aux_in: &'static [AuxIn],
}

impl PortConfig {
    pub const STEREO_FX: Self = Self {
        main_in: Some(Arrangement::Stereo),
        main_out: Arrangement::Stereo,
        aux_in: &[],
    };
    pub const STEREO_SYNTH: Self = Self {
        main_in: None,
        main_out: Arrangement::Stereo,
        aux_in: &[],
    };

    /// Main input channel count (0 for instruments) — the wrappers'
    /// scratch/bus arithmetic reads these, never raw counts.
    pub const fn in_channels(&self) -> u32 {
        match self.main_in {
            Some(a) => a.channels(),
            None => 0,
        }
    }
    pub const fn out_channels(&self) -> u32 {
        self.main_out.channels()
    }
}

/// Implement this for your plugin. Backends (CLAP, AU, standalone) wrap it.
pub trait Plugin: Send + 'static {
    /// Static identity. Read by the host before any instance exists.
    const DESCRIPTOR: PluginDescriptor;

    /// The declared audio-configuration SET, preference order first.
    /// Never empty.
    const CONFIGS: &'static [PortConfig] = &[PortConfig::STEREO_FX];

    /// Construct a fresh, deactivated instance.
    fn new() -> Self
    where
        Self: Sized;

    /// The typed parameter table. Return
    /// the SAME `Arc` on every call — the wrapper clones it once at
    /// instance creation and serves every host param query from it,
    /// while the plugin reads its own typed clone on the audio thread.
    /// The Arc lives outside the processor cell precisely so host
    /// threads never alias the audio thread's exclusive plugin borrow.
    ///
    /// The default is an empty table: a plugin with no parameters.
    fn param_table(&self) -> Arc<dyn Params> {
        Arc::new(crate::params::NoParams)
    }

    /// Factory presets, declared against typed params. The wrappers
    /// publish them as the host's program list; an editor steps through
    /// them (`Controller::apply_preset`).
    fn presets(&self) -> alloc::vec::Vec<crate::preset::Preset> {
        alloc::vec::Vec::new()
    }

    /// Called once before processing starts (or after sample rate changes).
    /// `max_frames` is the largest block size the host will request.
    fn activate(&mut self, _sample_rate: f32, _max_frames: usize) {}

    fn deactivate(&mut self) {}

    /// Realtime: do not allocate, lock, or block.
    fn process(&mut self, ctx: &mut ProcessCtx<'_>);

    /// Constant processing latency in samples that the host should
    /// compensate for (PDC). Plugins with look-ahead or fixed delay
    /// return that value; default 0 — no compensation needed.
    fn latency_samples(&self) -> u32 {
        0
    }

    /// Clear DSP state (delay lines, filters, envelopes) WITHOUT
    /// touching parameters. Called on host reset and re-activation so
    /// stale audio never rings into a fresh transport start.
    fn reset(&mut self) {}

    /// How long output remains audible after input stops. Hosts keep
    /// calling process for this long on transport stop — a reverb
    /// that reports `None` gets its tail truncated.
    fn tail(&self) -> Tail {
        Tail::None
    }

    /// Host wrote a parameter outside of a process block (e.g. preset load).
    fn set_param(&mut self, _id: crate::params::ParamId, _value: f64) {}

    /// Author-bumped state version, written into every chunk. Bump on
    /// any change `migrate` has to reason about.
    fn state_version() -> u32
    where
        Self: Sized,
    {
        0
    }

    /// Extra non-param state for the session chunk (tables, user
    /// curves). Most plugins have none. Non-realtime context.
    fn save_extra(&self) -> alloc::vec::Vec<u8> {
        alloc::vec::Vec::new()
    }

    /// Restore `save_extra`'s bytes. `from_version` is the chunk
    /// writer's `state_version`. Non-realtime context; called after
    /// param values are already applied.
    fn load_extra(&mut self, _bytes: &[u8], _from_version: u32) {}

    /// Declare param-level migrations (renames, rescales) applied when
    /// loading chunks from older versions. Ids are forever — this is
    /// the ONLY sanctioned way to move one.
    fn migrations() -> crate::state::Migrations
    where
        Self: Sized,
    {
        crate::state::Migrations::default()
    }

    /// Called once per instance, immediately after `new()`, with the
    /// instance's telemetry. Plugins register their taps here
    /// (`MyTaps::new(&telemetry)`) and publish from `process()`.
    ///
    /// Default no-op: a plugin without taps pays nothing.
    fn attach_telemetry(&mut self, _telemetry: Arc<Telemetry>) {}
}
