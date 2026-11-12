//! Eddy's typed parameter table. Ids are FOREVER — pick once, never
//! reuse or renumber. Groups: Delay (the line), Motion (the chorus),
//! Algorithm (the tabbed feedback laws + their per-tab knobs).

extern crate alloc;
use alloc::sync::Arc;

use stave_core::param_enum;
use stave_core::params::{
    BoolParam, EnumParam, FloatParam, ParamFlags, Params, Range, Smooth, Taper, Unit,
};

param_enum! {
    /// The LFO character menu. Selector label truth — the editor pills
    /// and the host's step list read from HERE. The variants are
    /// RECIPES over the kernel basis (shape ∘ lag ∘ drift), never new
    /// oscillator kinds — see engine.rs.
    pub const MOTIONS: Motion = ["SINE", "TRI", "JUNO", "VIBE"];
}

param_enum! {
    /// The four delay algorithms — the editor's tabs. Each is a distinct
    /// feedback law over the same M/S delay substrate (engine/echo.rs);
    /// the per-tab knobs below only exist inside their algorithm.
    /// (Slot 2 was TRACK, an octave-divider follower — replaced by SHIFT
    /// 2026-07-05: it couldn't track real program and its per-lap wobble
    /// smeared under feedback.)
    pub const ALGOS: Algo = ["CIRCUIT", "MOTOR", "SHIFT", "ORBIT"];
}

param_enum! {
    /// CIRCUIT's regeneration cell — which physics sits in the loop.
    pub const CIRCUITS: CircuitKind = ["TUBE", "SCREAMER", "TAPE"];
}

param_enum! {
    /// The diffusion stage's four voices — the second tabbed section.
    /// The diffuser sits INSIDE the loop, on the read side: what you
    /// hear is what regenerates, so density compounds per lap and
    /// cranking DIFFUSE grows the delay into one integrated reverberant
    /// system (never early/late reflections bolted around a line).
    pub const DIFFUSIONS: DiffusionKind = ["PLATE", "PRISM", "DUST", "MESH"];
}

#[derive(Params)]
pub struct EddyParams {
    /// Output trim. Smoothing is DECLARED here (table-owned ramp),
    /// never hand-rolled in the engine.
    pub output: FloatParam,
    /// The framework gate owns click-free bypass; this carries the flag.
    pub bypass: BoolParam,
    /// Delay time. The slow declared glide IS the tape bend: moving
    /// the knob sweeps the read head and pitch-bends the tail.
    pub time: FloatParam,
    /// How much of the wet tail is fed back into the line — the echo
    /// train's decay. Capped below unity: this delay never runs away.
    pub feedback: FloatParam,
    /// Dry LEVEL — the untouched program's own knob. At 1 with wet
    /// at 0 the output is BIT-identical dry (a law test).
    /// (id 5 was `mix`, a blend — retired 2026-07-05 for these two
    /// independent levels; the id is never reused.)
    pub dry: FloatParam,
    /// Wet LEVEL — the echo train's own knob, independent of dry.
    pub wet: FloatParam,
    /// Chorus rate — how fast the read head wobbles.
    pub rate: FloatParam,
    /// Chorus depth — how far it wobbles (0 = the delay stands still).
    pub depth: FloatParam,
    /// LFO character: pristine shapes or the analog recipes.
    pub motion: EnumParam<Motion>,
    /// Where the wobble lives: 0 = on the MID (both ears together —
    /// solid center, mono-safe), 1 = on the SIDE (maximal width,
    /// center anchored). The phantom center survives either way.
    pub spread: FloatParam,
    /// How much analog character the recipe applies: 0 = pristine
    /// (JUNO becomes the raw triangle, VIBE the raw sine), 1 = the
    /// lamp glowing at full thermal lag. Pristine shapes (SINE/TRI)
    /// have no character to exaggerate and ignore it by definition.
    pub flavor: FloatParam,
    /// Which feedback law runs — the editor's tab. Host-visible, so
    /// tab choice automates and saves like any knob.
    pub algo: EnumParam<Algo>,
    /// CIRCUIT: the physics cell in the regeneration path.
    pub circuit: EnumParam<CircuitKind>,
    /// CIRCUIT: how hard each repeat drives the cell. 0 BYPASSES the
    /// cell — the loop is the classic law, bit for bit (a law test).
    pub heat: FloatParam,
    /// MOTOR: how much program energy loads the motor — the wet tail
    /// drags its own time (and pitch) flat, then recovers.
    pub sag: FloatParam,
    /// MOTOR: the platter's mass — how slowly the sag leans in and
    /// lets go.
    pub inertia: FloatParam,
    /// SHIFT: semitones each repeat is transposed by — +12 is the
    /// shimmer, negative spirals down. (id 16 was TRACK's amount —
    /// retired with the algorithm; the id is never reused.)
    pub shift: FloatParam,
    /// SHIFT: the transposer's grain window — the splice cadence.
    /// Small = tight flutter, large = washy; turning it bends the
    /// heads through its declared glide (the TIME-knob honesty).
    pub grain: FloatParam,
    /// SHIFT: how hard the machine hides each seam — at 1 every
    /// incoming head is phase-aligned by search (pristine), at 0 it
    /// lands wherever the sweep says (raw tape-splice warble).
    pub splice: FloatParam,
    /// ORBIT: degrees the echo image rotates per repeat (negative =
    /// the other way around). 0 = echoes hold their place.
    pub orbit: FloatParam,
    /// Which diffusion voice runs — the second tab bar.
    pub diffusion: EnumParam<DiffusionKind>,
    /// The diffusion stage's depth. 0 BYPASSES it — the loop is the
    /// undiffused law, bit for bit (a law test); 1 with feedback up
    /// is the full integrated reverb.
    pub diffuse: FloatParam,
    /// PLATE: the lattice's allpass strength — echo softening at low
    /// settings, full smear at the top.
    pub density: FloatParam,
    /// PLATE: how far the lattice delays WANDER (the Lexicon move —
    /// static lattices ring metallic; wandering ones bloom).
    pub wander: FloatParam,
    /// PRISM: where the chirp bends — low is the deep whale-tail
    /// (a first-order dispersive line ALWAYS lags lows; physics
    /// permits no other direction), high is the tight zap.
    pub bend: FloatParam,
    /// PRISM: how deep into the dispersive line each repeat travels
    /// (continuous — the tap slides along the cascade).
    pub smear: FloatParam,
    /// DUST: the cloud's radius — how far the micro-taps stray.
    pub spray: FloatParam,
    /// DUST: how many motes carry the echo — few coarse grains up to
    /// a fine dust.
    pub motes: FloatParam,
    /// MESH: the woven lines' lengths — small rooms to long strands.
    pub size: FloatParam,
    /// MESH: how hard the image-space rotations cross-couple the
    /// lines (and how much they re-circulate inside the mesh).
    pub weave: FloatParam,
    /// The field's OWN feedback — how long it rings after a repeat
    /// feeds it. This is what unchains the diffusion from the delay
    /// time: a 2-second echo can sit inside a continuous wash.
    pub hold: FloatParam,
    /// Field → line: how much of the field re-enters the
    /// regeneration. 0 = the field blooms AROUND an articulate
    /// delay; up = field content recirculates through the algorithm
    /// itself (shimmer feeding reverb feeding shimmer).
    pub bleed: FloatParam,
}

impl EddyParams {
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
            time: FloatParam::new(3, "time", 350.0, Range::new(1.0, 2000.0, Taper::Log))
                .with_unit(Unit::Ms)
                .with_group("Delay")
                .with_smooth(Smooth::Exp { ms: 80.0 }),
            feedback: FloatParam::new(4, "feedback", 0.35, Range::new(0.0, 0.95, Taper::Linear))
                .with_group("Delay")
                .with_smooth(Smooth::Exp { ms: 20.0 }),
            dry: FloatParam::new(18, "dry", 1.0, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Delay")
                .with_smooth(Smooth::Exp { ms: 5.0 }),
            wet: FloatParam::new(19, "wet", 0.35, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Delay")
                .with_smooth(Smooth::Exp { ms: 5.0 }),
            rate: FloatParam::new(6, "rate", 0.8, Range::new(0.05, 10.0, Taper::Log))
                .with_group("Motion")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            depth: FloatParam::new(7, "depth", 0.0, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Motion")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            motion: EnumParam::new(8, "motion", Motion(0)).with_group("Motion"),
            spread: FloatParam::new(9, "spread", 0.5, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Motion")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            flavor: FloatParam::new(10, "flavor", 0.5, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Motion")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            algo: EnumParam::new(11, "algo", Algo(0)).with_group("Algorithm"),
            circuit: EnumParam::new(12, "circuit", CircuitKind(0)).with_group("Algorithm"),
            heat: FloatParam::new(13, "heat", 0.0, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Algorithm")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            sag: FloatParam::new(14, "sag", 0.4, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Algorithm")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            inertia: FloatParam::new(15, "inertia", 0.5, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Algorithm")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            shift: FloatParam::new(20, "shift", 12.0, Range::new(-12.0, 12.0, Taper::Linear))
                .with_group("Algorithm")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            grain: FloatParam::new(21, "grain", 64.0, Range::new(16.0, 160.0, Taper::Log))
                .with_unit(Unit::Ms)
                .with_group("Algorithm")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            splice: FloatParam::new(22, "splice", 1.0, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Algorithm")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            orbit: FloatParam::new(17, "orbit", 60.0, Range::new(-180.0, 180.0, Taper::Linear))
                .with_group("Algorithm")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            diffusion: EnumParam::new(23, "diffusion", DiffusionKind(0)).with_group("Diffusion"),
            diffuse: FloatParam::new(24, "diffuse", 0.0, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Diffusion")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            density: FloatParam::new(25, "density", 0.5, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Diffusion")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            wander: FloatParam::new(26, "wander", 0.5, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Diffusion")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            bend: FloatParam::new(27, "bend", 0.35, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Diffusion")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            smear: FloatParam::new(28, "smear", 0.5, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Diffusion")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            spray: FloatParam::new(29, "spray", 0.5, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Diffusion")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            motes: FloatParam::new(30, "motes", 0.6, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Diffusion")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            size: FloatParam::new(31, "size", 0.5, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Diffusion")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            weave: FloatParam::new(32, "weave", 0.5, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Diffusion")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            hold: FloatParam::new(33, "hold", 0.5, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Diffusion")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
            bleed: FloatParam::new(34, "bleed", 0.0, Range::new(0.0, 1.0, Taper::Linear))
                .with_group("Diffusion")
                .with_smooth(Smooth::Exp { ms: 50.0 }),
        })
    }
}
