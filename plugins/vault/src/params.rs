//! Vault's typed parameter table. Ids are FOREVER — pick once,
//! never reuse or renumber. RETIRED ids (removed with the tape
//! purge, 2026-07-21 — never reuse): 9 (wow), 10 (drive);
//! 8 (motion — baked into the engines, 2026-07-21: an anti-
//! metallic health parameter, not a musical decision); 23/24/25
//! (launch/bloom/lift — folded into DISTANCE's low end, DUCK, and
//! a SIZE-scaled bake, 2026-07-21: the behaviors live where their
//! names already were).
//!
//! The PRISTINE↔LOFI axis is deliberately not one knob: DAMP is
//! the room's absorption, MOTION is the tank's chorus, WOW is slow
//! tape-style drift, DRIVE is loop saturation. All four at zero =
//! the pristine hall; dialed together they walk the machine down
//! to lofi without ever leaving the same physical tank.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::param_enum;
use stave_core::params::{
    BoolParam, EnumParam, FloatParam, ParamFlags, Params, Range, Smooth, Taper, Unit,
};

param_enum! {
    /// THE TOPOLOGY — which MACHINE this is, not a flavor of one
    /// machine (the arc's ratifying critique: one skeleton, many
    /// shaders = every preset is the same room). ROOM is the
    /// image-source + FDN + modes machine; PLATE is a dedicated
    /// 2D-membrane skeleton (short dispersive lines, instant
    /// density, damper decay — no geometry stages at all).
    pub const TOPOLOGIES: Topology = ["ROOM", "PLATE", "MEMORY", "DETONATE", "VOID"];
}

param_enum! {
    /// What the WALLS are made of — the room's construction, not a
    /// post-EQ: per-bounce absorption and brightness in the early
    /// field, bass ring and damping in the late (scene.rs owns the
    /// table). PLASTER is the pre-materials voicing exactly.
    pub const MATERIALS: WallMaterial =
        ["WOOD", "PLASTER", "STONE", "GLASS", "DRAPE", "METAL"];
}

#[derive(Params)]
pub struct VaultParams {
    /// Output trim. Smoothing is DECLARED here (table-owned ramp),
    /// never hand-rolled in the engine.
    pub output: FloatParam,
    /// The framework gate owns click-free bypass; this carries the flag.
    pub bypass: BoolParam,
    /// Dry/wet. mix 0 is BIT-dry (the parallel-chain contract).
    pub mix: FloatParam,
    /// Room scale: multiplies every tank line's length. The engine
    /// GLIDES length changes (slew-capped read heads — turning the
    /// room bends pitch like tape, never clicks).
    pub size: FloatParam,
    /// Decay time (T60) — a MEASURED promise, not a flavor name:
    /// the t60 law pins the dialed value against the rendered tail.
    pub decay: FloatParam,
    /// Wet onset delay. Glided by declaration (tape-bend, no click).
    pub predelay: FloatParam,
    /// The room's absorption: HF loss per trip through the tank.
    pub damp: FloatParam,
    /// Wet stereo width, 0..2: narrow → natural (1) → artificially
    /// wide (side boost + slight mid dip). Id 11 (the old 0..1
    /// range) is RETIRED — ranges are frozen, never bent.
    pub width: FloatParam,
    /// WHERE YOU STAND across the room's width. Off-center, the
    /// near wall answers earlier and stronger and each ear wears
    /// its own coloration — L/R balance EMERGES from the geometry,
    /// it is not panned on.
    pub stand: FloatParam,
    /// THE ROOM's voice in the wet field: the image-source early
    /// reflections' level. The late tank always grows out of the
    /// room's answer; this dial is how much of the answer you hear
    /// raw.
    pub early: FloatParam,
    /// Low-band decay MULTIPLIER (the bass ratio — real halls ring
    /// their bass 1.1-1.4x longer; tight rooms cut it): below
    /// ~350 Hz the tail decays at decay x bass.
    pub bass: FloatParam,
    /// TONE is the WALLS now, not a post-EQ: it scales the room's
    /// material cutoffs, darkening/brightening the early taps AND
    /// the late field through the tank's feed (the tank eats what
    /// the walls serve). DAMP remains the decay's darkening over
    /// time.
    pub tone: FloatParam,
    /// Scattering density: input diffusers AND the in-loop
    /// diffusion together. Low = discrete vintage echoes (a
    /// CHARACTER, not a defect); high = smooth wash.
    pub diffuse: FloatParam,
    /// DISPERSION — a stiff medium's frequency-dependent wave
    /// speed (metal plates and springs chirp; air does not): a
    /// first-order allpass cascade in every tank loop. 0 = a room.
    pub stiff: FloatParam,
    /// THE DUCK — the wet compresses under the DRY's envelope and
    /// blooms into the gaps (the companding trick that lets a
    /// reverb run LOUD and stay clear: reactiveness, not wash).
    pub duck: FloatParam,
    /// The walls' construction (see MATERIALS).
    pub material: EnumParam<WallMaterial>,
    /// Which machine (see TOPOLOGIES). Id 22.
    pub topology: EnumParam<Topology>,
    /// THE MEMORY's ladder (MEMORY machine only): energy leaks UP
    /// the resonator grid one octave at a time — the remembered
    /// chord climbs as it sustains. Shimmer with no pitch shifter,
    /// unconditionally stable (the ladder only goes up and out).
    /// Id 26.
    pub ascend: FloatParam,
    /// THE FUSE (DETONATE machine only): the burst's envelope
    /// shape — 0 = instant detonation that decays, 1 = a reverse
    /// swell that builds INTO the cut. Id 27.
    pub fuse: FloatParam,
    /// THE DAWN (VOID machine only): the send swells IN — sources
    /// rise into the field instead of splashing against it. Id 28.
    pub dawn: FloatParam,
    /// WHERE THE STAGE STANDS, front to back: 0 = right in front
    /// of you (intimate — long first-reflection gap, bright,
    /// early-heavy), 1 = the far wall (deep — short gap, dark,
    /// wash-heavy). One geometry moves every cue together.
    pub distance: FloatParam,
}

impl VaultParams {
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
            mix: FloatParam::new(3, "mix", 0.32, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Field")
                .with_smooth(Smooth::Exp { ms: 20.0 }),
            size: FloatParam::new(4, "size", 1.0, Range::new(0.3, 3.0, Taper::Log))
                .with_group("Space")
                .with_smooth(Smooth::Off),
            decay: FloatParam::new(5, "decay", 2_400.0, Range::new(100.0, 30_000.0, Taper::Log))
                .with_unit(Unit::Ms)
                .with_group("Space")
                .with_smooth(Smooth::Off),
            predelay: FloatParam::new(6, "predelay", 12.0, Range::new(0.0, 250.0, Taper::Linear))
                .with_unit(Unit::Ms)
                .with_group("Space")
                .with_smooth(Smooth::Exp { ms: 80.0 }),
            damp: FloatParam::new(7, "high energy", 0.35, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Color")
                .with_smooth(Smooth::Off),
            width: FloatParam::new(19, "width", 1.0, Range::new(0.0, 2.0, Taper::Linear))
                .with_group("Field")
                .with_smooth(Smooth::Exp { ms: 20.0 }),
            stand: FloatParam::new(18, "stand", 0.2, Range::new(-1.0, 1.0, Taper::Linear))
                .with_group("Field")
                .with_smooth(Smooth::Off),
            early: FloatParam::new(12, "early", 0.55, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Space")
                .with_smooth(Smooth::Exp { ms: 20.0 }),
            bass: FloatParam::new(13, "low energy", 1.0, Range::new(0.25, 4.0, Taper::Log))
                .with_group("Color")
                .with_smooth(Smooth::Off),
            tone: FloatParam::new(14, "tone", 0.0, Range::new(-1.0, 1.0, Taper::Linear))
                .with_group("Color")
                .with_smooth(Smooth::Off),
            diffuse: FloatParam::new(15, "diffuse", 0.7, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Color")
                .with_smooth(Smooth::Off),
            stiff: FloatParam::new(16, "whip", 0.0, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Color")
                .with_smooth(Smooth::Off),
            duck: FloatParam::new(17, "duck", 0.0, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Field")
                .with_smooth(Smooth::Off),
            material: EnumParam::new(20, "material", WallMaterial(1)).with_group("Color"),
            topology: EnumParam::new(22, "topology", Topology(0)).with_group("Space"),
            ascend: FloatParam::new(26, "ascend", 0.0, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Space"),
            fuse: FloatParam::new(27, "fuse", 0.25, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Space"),
            dawn: FloatParam::new(28, "dawn", 0.4, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Space"),
            distance: FloatParam::new(21, "distance", 0.7, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Space")
                .with_smooth(Smooth::Off),
        })
    }
}
