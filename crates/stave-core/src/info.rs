//! What a plugin declares before any instance exists: its identity
//! ([`Info`]) and the bus configurations it supports ([`Layout`]).

use core::ffi::CStr;

/// A plugin's identity, read by hosts before any instance exists.
///
/// Start from [`Info::EFFECT`] or [`Info::INSTRUMENT`] and name what
/// differs:
///
/// ```
/// use stave_core::Info;
///
/// const INFO: Info = Info { id: "com.example.tilt", name: "Tilt", vendor: "Example", ..Info::EFFECT };
/// assert!(INFO.version.is_empty()); // the exporting crate's version
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Info {
    /// Reverse-DNS identifier, unique and FOREVER: hosts key sessions on
    /// it, and the VST3 class ids derive from it.
    pub id: &'static str,
    pub name: &'static str,
    pub vendor: &'static str,
    /// Empty: the version of the crate that exports the plugin
    /// (`CARGO_PKG_VERSION`).
    pub version: &'static str,
    pub url: &'static str,
    pub description: &'static str,
    /// What kind of plugin this is, for hosts' browsers.
    pub features: &'static [Feature],
}

impl Info {
    /// An audio effect. Fill in `id`, `name` and `vendor`.
    pub const EFFECT: Info = Info {
        id: "",
        name: "",
        vendor: "",
        version: "",
        url: "",
        description: "",
        features: &[Feature::AudioEffect],
    };

    /// An instrument (pair it with [`Layout::INSTRUMENT`]).
    pub const INSTRUMENT: Info = Info {
        features: &[Feature::Instrument],
        ..Info::EFFECT
    };
}

/// A host-browser category.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Feature {
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

impl Feature {
    /// The CLAP feature string.
    pub const fn clap_name(self) -> &'static CStr {
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

/// The channels of one bus, named by their speaker meaning (never a bare
/// count): what hosts negotiate. Plugins lift them into their own lane
/// semantics (stereo → mid/side is a plugin's adapter, not a bus).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Channels {
    Mono,
    Stereo,
    /// L R C LFE Ls Rs, in that order (the VST3/ITU order).
    Surround51,
}

impl Channels {
    pub const fn count(self) -> usize {
        match self {
            Channels::Mono => 1,
            Channels::Stereo => 2,
            Channels::Surround51 => 6,
        }
    }
}

/// An auxiliary input bus (a sidechain): named, with its channels. Aux
/// buses are always optional to connect — a plugin must work with every
/// aux silent or absent.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Aux {
    pub name: &'static str,
    pub channels: Channels,
}

/// One supported bus configuration: every bus's channels at once.
///
/// The unit of negotiation is the whole plugin — coupling between buses
/// (main in matches main out, a sidechain's presence) lives inside one
/// layout, so there is no "is this combination supported" predicate for
/// hosts to probe. `Plugin::LAYOUTS` lists the supported set in
/// preference order; the host picks one.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    /// The main input; `None` for an instrument.
    pub input: Option<Channels>,
    pub output: Channels,
    /// Sidechains and other secondary inputs, in order.
    pub aux: &'static [Aux],
}

impl Layout {
    /// Stereo in, stereo out.
    pub const STEREO: Layout = Layout {
        input: Some(Channels::Stereo),
        output: Channels::Stereo,
        aux: &[],
    };
    /// Mono in, mono out.
    pub const MONO: Layout = Layout {
        input: Some(Channels::Mono),
        output: Channels::Mono,
        aux: &[],
    };
    /// No audio in, stereo out.
    pub const INSTRUMENT: Layout = Layout {
        input: None,
        output: Channels::Stereo,
        aux: &[],
    };

    /// Main input channels (0 for an instrument).
    pub const fn input_channels(&self) -> usize {
        match self.input {
            Some(c) => c.count(),
            None => 0,
        }
    }

    /// Main output channels.
    pub const fn output_channels(&self) -> usize {
        self.output.count()
    }
}
