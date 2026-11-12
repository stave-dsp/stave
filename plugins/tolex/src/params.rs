//! Tolex's typed parameter table. Ids are FOREVER — pick once,
//! never reuse or renumber. Ids 6–8, 13–14 and 16–18 are reserved.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::param_enum;
use stave_core::params::{
    BoolParam, EnumParam, FloatParam, ParamFlags, Params, Range, Smooth, Taper, Unit,
};

param_enum! {
    /// Driver diameter. Sets the cone area Sd and the moving-mass
    /// default the motor derives everything else from (engine/motor.rs
    /// SIZES table is the physics-side twin of this menu).
    pub const SIZES: SpeakerSize = ["8\"", "10\"", "12\"", "15\""];
}

param_enum! {
    /// Cabinet material — each is a panel-mode SET in the interior
    /// (engine/interior.rs PANELS is the physics twin): birch ply is
    /// stiff and ringy, pine loud and low, MDF dead.
    pub const MATERIALS: CabMaterial = ["BIRCH", "PINE", "MDF"];
}

param_enum! {
    /// How many cones share the baffle. Pure geometry at the mic
    /// (engine/drivers.rs): close-mic'd they collapse to one; pulled
    /// back they weave.
    pub const SPEAKERS: SpeakerCount = ["1x12", "2x12", "4x12"];
}

param_enum! {
    /// Mic models — each a signature DISTILLED from the reference
    /// corpus (median difference vs the SM57 over matched
    /// speaker/position groups; engine/microphone.rs). 57 = flat.
    pub const MICS: MicModel = ["57", "421", "58", "906", "SM7", "121", "160", "U47"];
}

param_enum! {
    /// The cabinet's back. CLOSED is the sealed air spring (the
    /// alignment shift is the motor's physics, not an EQ); OPEN
    /// frees the spring and opens the rear wrap path to the mic.
    pub const BACKS: CabBack = ["CLOSED", "OPEN"];
}

// DEFAULTS ARE BREAKDOWN (2026-07-26): load = the metalcore rig.
// The neutral bench lives on as the INIT preset and in every law's
// pinned bench sets.
#[derive(Params)]
pub struct TolexParams {
    /// Output trim. Smoothing is DECLARED here (table-owned ramp),
    /// never hand-rolled in the engine.
    pub output: FloatParam,
    /// The framework gate owns click-free bypass; this carries the flag.
    pub bypass: BoolParam,
    /// Driver diameter — the physical scale of everything.
    pub size: EnumParam<SpeakerSize>,
    /// Free-air resonance fs. Guitar drivers live 55–120 Hz.
    pub resonance: FloatParam,
    /// Total Q (Qts) at the resonance — how springy the suspension
    /// is against the motor's grip. House Qms is fixed; the electrical
    /// side derives from this (engine/motor.rs).
    pub q: FloatParam,
    /// How hard the amp swings the voice coil, in dB around an 8 V
    /// reference. Loudness-compensated by construction (the motor
    /// normalizes its passband by the same swing): this knob buys
    /// EXCURSION — compression and harmonics — not volume.
    pub drive: FloatParam,
    /// The amp's source impedance, 0 = solid-state grip (≈0 Ω),
    /// 1 = no-feedback tube output (≈40 Ω): the speaker's own
    /// impedance curve prints on the response. Real coupling no IR
    /// can render — a headline law.
    pub damping: FloatParam,
    /// The CONE — which fingerprint this speaker wears (the
    /// coordinate axis: anchors are fitted at specific cones; 0 is
    /// the house voice). Same physical law at every step.
    pub cone: FloatParam,
    /// Moving-mass scale (×Mms). Heavier cones break up lower and
    /// later; the whole modal fingerprint slides with it (a law).
    pub mass: FloatParam,
    /// How loud the cone's own voice is — the modal bank's level.
    /// 0 is a rigid piston (the fake); the default sits where the
    /// York population's ripple lives.
    pub breakup: FloatParam,
    /// Edge doping / damping: eats mode Q and prominence from the
    /// top of the band down, the way surround treatment does.
    pub doping: FloatParam,
    /// Closed or open back.
    pub back: EnumParam<CabBack>,
    /// Mic position across the cone, 0 = dust cap … 1 = cone edge.
    /// Macro: the corpus-calibrated −4.6 dB tilt walk. Micro: mode
    /// weights shift and the cap-cavity comb moves (~69% of peaks
    /// persist, a law).
    pub position: FloatParam,
    /// Interior volume in liters — the size of the air spring and
    /// the wrap path's scale.
    pub cab: FloatParam,
    /// What the box is built from — the panel modes' voice.
    pub material: EnumParam<CabMaterial>,
    /// Driver count — the weave.
    pub speakers: EnumParam<SpeakerCount>,
    /// Which microphone hears the cab. Signatures measured in
    /// context (corpus distillation), not from datasheets.
    pub model: EnumParam<MicModel>,
    /// Mic distance from the grille, in cm. Close = the direct field;
    /// pulled back = the open back's comb deepens (the rear path's
    /// level catches up with the front's).
    pub mic: FloatParam,
}

/// The speaker, cab and mic construction rebuilds the model when it
/// changes, so it does not glide; hosts may not automate it.
const CONSTRUCTION: ParamFlags = ParamFlags {
    bypass: false,
    non_automatable: true,
    hidden: false,
};

impl TolexParams {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            output: FloatParam::new(1, "output", 0.0, Range::new(-24.0, 24.0, Taper::Linear))
                .with_unit(Unit::Db { floor_db: None })
                .with_group("Output")
                .with_smooth(Smooth::Exp { ms: 5.0 }),
            bypass: BoolParam::new(2, "bypass", false)
                .with_group("Output")
                .with_flags(ParamFlags {
                    bypass: true,
                    ..Default::default()
                }),
            size: EnumParam::new(3, "size", SpeakerSize(2)).with_group("Speaker"),
            resonance: FloatParam::new(4, "resonance", 92.0, Range::new(55.0, 120.0, Taper::Log))
                .with_unit(Unit::Hz)
                .with_group("Speaker")
                .with_flags(CONSTRUCTION),
            q: FloatParam::new(5, "q", 0.7, Range::new(0.3, 1.2, Taper::Linear))
                .with_group("Speaker")
                .with_flags(CONSTRUCTION),
            drive: FloatParam::new(9, "drive", 6.0, Range::new(-12.0, 24.0, Taper::Linear))
                .with_unit(Unit::Db { floor_db: None })
                .with_group("Amp")
                .with_smooth(Smooth::Exp { ms: 20.0 }),
            damping: FloatParam::new(10, "damping", 0.3, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Amp")
                .with_flags(CONSTRUCTION),
            // Range extended 31 -> 32 the same day id 19 was minted
            // (pre-release; never in a shipped build): slot 32+ are
            // MEASURED anchors, 0-31 the invented space.
            cone: FloatParam::new(19, "cone", 21.0, Range::new(0.0, 32.0, Taper::Linear))
                .with_group("Speaker")
                .with_flags(CONSTRUCTION),
            mass: FloatParam::new(6, "mass", 1.05, Range::new(0.5, 2.0, Taper::Log))
                .with_group("Speaker")
                .with_flags(CONSTRUCTION),
            breakup: FloatParam::new(7, "breakup", 0.6, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Speaker")
                .with_flags(CONSTRUCTION),
            doping: FloatParam::new(8, "doping", 0.38, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Speaker")
                .with_flags(CONSTRUCTION),
            back: EnumParam::new(11, "back", CabBack(0)).with_group("Cab"),
            position: FloatParam::new(16, "position", 0.15, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Mic")
                .with_flags(CONSTRUCTION),
            cab: FloatParam::new(12, "cab", 170.0, Range::new(25.0, 200.0, Taper::Log))
                .with_group("Cab")
                .with_flags(CONSTRUCTION),
            material: EnumParam::new(13, "material", CabMaterial(0)).with_group("Cab"),
            speakers: EnumParam::new(14, "speakers", SpeakerCount(2)).with_group("Cab"),
            model: EnumParam::new(17, "model", MicModel(1)).with_group("Mic"),
            mic: FloatParam::new(15, "mic", 2.5, Range::new(0.5, 60.0, Taper::Log))
                .with_group("Mic")
                .with_smooth(Smooth::Exp { ms: 30.0 }),
        })
    }
}
