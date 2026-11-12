//! Forge's typed parameter table.
//!
//! Ids are FOREVER (docs/architecture.md): 1–19 in original declaration
//! order. Drive, bias, width and the trims glide per sample. Edge and
//! tone are `Smooth::Off`: they re-voice the stage tables and filters,
//! at block rate.
//! `bypass` carries the flag: the framework gate owns click-free bypass.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::params::{
    BoolParam, EnumParam, FloatParam, ParamEnum, ParamFlags, Params, Range, Smooth, Taper, Unit,
};

pub const CHARACTERS: &[&str] = &["SILK", "TRIODE", "CLASS AB", "TAPE", "DIODE", "FUZZ"];
pub const STACKS: &[&str] = &["SINGLE", "CASCADE", "SCREAMER", "PUSH-PULL"];
pub const VOICES: &[&str] = &["FLAT", "SCOOP", "BUMP", "BRIGHT", "DARK", "HONK"];
pub const MEMORIES: &[&str] = &["NONE", "SAG", "BLOCKING", "SQUEEZE", "RIPPLE"];
pub const POSTS: &[&str] = &["FLAT", "AIR", "LOW CUT", "WARM", "HI CUT", "DARK"];

/// Index-selector param enums: the engine consumes positions as
/// `usize`; hosts and editors see named steps.
macro_rules! selector {
    ($name:ident, $names:expr) => {
        #[derive(Copy, Clone, Debug, PartialEq, Eq)]
        pub struct $name(pub u32);
        impl ParamEnum for $name {
            const COUNT: u32 = $names.len() as u32;
            fn from_index(i: u32) -> Self {
                Self(i.min(Self::COUNT - 1))
            }
            fn to_index(self) -> u32 {
                self.0
            }
            fn name(i: u32) -> &'static str {
                $names[(i as usize).min($names.len() - 1)]
            }
        }
    };
}

selector!(Character, CHARACTERS);
selector!(Stack, STACKS);

/// Stack positions by NAME — the engine's mode gates read these
/// beside the selector that defines them.
pub(crate) mod stack {
    pub(crate) const SCREAMER: usize = 2;
    pub(crate) const PUSH_PULL: usize = 3;
}
selector!(Voice, VOICES);
selector!(Memory, MEMORIES);
selector!(PostVoice, POSTS);

#[derive(Params)]
pub struct ForgeParams {
    pub drive: FloatParam,
    pub character: EnumParam<Character>,
    pub edge: FloatParam,
    pub bias: FloatParam,
    pub stack: EnumParam<Stack>,
    pub filter: EnumParam<Voice>,
    pub post: EnumParam<PostVoice>,
    pub tone: FloatParam,
    pub memory: EnumParam<Memory>,
    pub depth: FloatParam,
    pub grit: FloatParam,
    pub iron: FloatParam,
    pub sub_mix: FloatParam,
    pub sub_mod: FloatParam,
    pub width: FloatParam,
    pub input: FloatParam,
    pub mix: FloatParam,
    pub output: FloatParam,
    pub bypass: BoolParam,
}

/// The stage frame's glide: drive and bias move the operating point of
/// a nonlinearity, where a step is a click.
const FRAME_GLIDE: Smooth = Smooth::Exp { ms: 20.0 };

impl ForgeParams {
    pub fn new() -> Arc<Self> {
        let unit = |id: u32, name: &'static str, default: f64, group: &'static str| {
            FloatParam::new(id, name, default, Range::new(0.0, 1.0, Taper::Linear))
                .with_group(group)
                .with_smooth(Smooth::Off)
        };
        Arc::new(Self {
            drive: FloatParam::new(1, "drive", 4.0, Range::new(0.0, 10.0, Taper::Linear))
                .with_group("Stage 1")
                .with_smooth(FRAME_GLIDE),
            character: EnumParam::new(2, "character", Character(1)).with_group("Stage 1"),
            edge: unit(3, "edge", 0.3, "Stage 1"),
            bias: unit(4, "bias", 0.0, "Stage 1").with_smooth(FRAME_GLIDE),
            stack: EnumParam::new(5, "stack", Stack(1)).with_group("Topology"),
            filter: EnumParam::new(6, "filter", Voice(0)).with_group("Topology"),
            post: EnumParam::new(7, "post filter", PostVoice(0)).with_group("Topology"),
            tone: FloatParam::new(8, "tone", -0.2, Range::new(-1.0, 1.0, Taper::Linear))
                .with_group("Topology")
                .with_smooth(Smooth::Off),
            memory: EnumParam::new(9, "memory", Memory(1)).with_group("Texture"),
            depth: unit(10, "depth", 0.25, "Texture"),
            grit: unit(11, "grit", 0.0, "Texture"),
            iron: unit(12, "iron", 0.0, "Texture"),
            sub_mix: unit(13, "sub mix", 0.0, "Sub"),
            sub_mod: unit(14, "sub mod", 0.0, "Sub"),
            width: unit(15, "width", 0.5, "Stereo").with_smooth(FRAME_GLIDE),
            // The output trims are CONTINUOUS BINDINGS: smoothing is
            // declared here and rendered by the engine's
            // `ContinuousControl`s, not hand-rolled.
            input: FloatParam::new(16, "input", 0.0, Range::new(-24.0, 24.0, Taper::Linear))
                .with_unit(Unit::Db { floor_db: None })
                .with_group("Output")
                .with_smooth(Smooth::Exp { ms: 5.0 }),
            mix: FloatParam::new(17, "mix", 1.0, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Output")
                .with_smooth(Smooth::Exp { ms: 5.0 }),
            output: FloatParam::new(18, "output", 0.0, Range::new(-24.0, 24.0, Taper::Linear))
                .with_unit(Unit::Db { floor_db: None })
                .with_group("Output")
                .with_smooth(Smooth::Exp { ms: 5.0 }),
            bypass: BoolParam::new(19, "bypass", false)
                .with_group("Output")
                .with_flags(ParamFlags {
                    bypass: true,
                    ..Default::default()
                }),
        })
    }
}
