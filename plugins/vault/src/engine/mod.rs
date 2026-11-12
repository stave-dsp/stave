//! The vault ENGINE — a place, not an algorithm. One machine in
//! four components, signal order:
//!
//! * PREDELAY  — per-channel glided delay (the wet field's onset);
//! * THE ROOM  — the fidelity half: a live image-source geometry
//!   whose reflections, interaural cues, and size scaling are all
//!   one description (room.rs);
//! * DIFFUSER  — four detuned allpasses per ear (the transition
//!   bloom between the room's answer and the late field);
//! * TANK      — the 8-line Hadamard FDN (the nonlinear half: T60
//!   by declared per-trip loss, absorption, DC-blocked loops,
//!   passive drive, chorused + wowed read heads). The tank is fed
//!   BY the room's answer, never the raw input — early and late
//!   cannot disagree about where they are.
//!
//! The wet field leaves through a mid/side width stage (kernel
//! ms_encode/ms_decode — the eddy lesson: allocate the field, never
//! mirror it) and crossfades with the UNTOUCHED dry path (mix 0 is
//! bit-dry, the parallel-chain contract).
//!
//! Block discipline: all voicing (sizes, T60 gains, chorus/wow
//! targets) is block-rate; every read head glides per sample toward
//! its target under one slew cap — knob jumps bend like tape.

extern crate alloc;
use alloc::sync::Arc;
use stave_core::params::ContinuousControl;

use stave_core::{Environment, ProcessCtx, Tail};
use stave_dsp::{db_to_amp, ms_decode, ms_encode, DelayLine};

mod detonate;
mod diffuser;
mod memory;
mod midfield;
mod modes;
mod plate;
mod rerad;
mod room;
mod scene;
mod tank;
mod void_field;
use detonate::Detonate;
use diffuser::Diffuser;
use memory::Memory;
use midfield::Midfield;
use modes::Modes;
use plate::Plate;
use rerad::Rerad;
use room::Room;
use tank::Tank;
use void_field::Void;

use crate::params::VaultParams;

/// Dial-glide slots: size, t60, damp, tone, bass-dial, stand,
/// distance, diffuse, stiff, wall-gain, wall-cutoff, bass-mult,
/// hi-mult, damp-add, width, duck.
const NGLIDE: usize = 16;

/// The plate's loudness calibration relative to the shared
/// WET_MAKEUP path (measured against the room wet on program).
const PLATE_MAKEUP: f32 = 0.44;

/// The memory's loudness calibration (measured on program).
const MEMORY_MAKEUP: f32 = 0.9;

/// The chamber's loudness calibration (measured on hits).
const DET_MAKEUP: f32 = 1.6;

/// The void's loudness calibration.
const VOID_MAKEUP: f32 = 0.55;

/// Tank injection level (diffused ROOM answer → lines; the room's
/// image gains run ~10 dB below raw input, compensated here —
/// measured, the late field fell from −48 to −58 dB when the tank
/// moved onto the room's feed).
const INJECT: f32 = 1.8;
/// Wet makeup: the summed wet ran −11.5 dB under dry at FULL wet
/// (measured — the field heard the deficit as missing fidelity).
/// 3.4 matched RMS exactly but a diffuse field's Gaussian crest
/// peaks ~2.5x above a matched-RMS dry (field: clipping) — 2.7
/// lands ~-2 dB RMS with civilized peaks.
const WET_MAKEUP: f32 = 2.7;
/// Chorus ceiling in samples at 48k (scaled by rate).
const MOTION_SPAN: f32 = 26.0;

pub struct VaultEngine {
    params: Arc<VaultParams>,
    output_control: ContinuousControl,
    early_control: ContinuousControl,
    mix_control: ContinuousControl,
    predelay_control: ContinuousControl,
    sr: f32,
    predelay: [DelayLine; 2],
    /// The fidelity half: the image-source geometry.
    room: Room,
    /// THE PLATE TOPOLOGY — a dedicated skeleton (plate.rs); the
    /// room machinery is not tuned and not ticked while it plays.
    plate: Plate,
    /// THE MEMORY TOPOLOGY — the remembering field (memory.rs).
    memory: Memory,
    /// THE DETONATE TOPOLOGY — the event chamber (detonate.rs).
    det: Detonate,
    /// THE VOID TOPOLOGY — the wall-less field (void_field.rs).
    void: Void,
    /// Active topology index (0 = ROOM, 1 = PLATE) and the wet
    /// fade that makes the machine swap click-free.
    topo: usize,
    topo_fade: f32,
    rerad: Rerad,
    diffusers: [Diffuser; 2],
    /// The region between the room's discrete answer and the
    /// tank's statistics (mean-free-path allpasses per ear).
    mid: [Midfield; 4],
    tank: Tank,
    /// The room below its Schroeder frequency: standing waves.
    modes: Modes,
    /// The side channel's low-fold (real rooms are interaurally
    /// coherent below ~250 Hz; also the mono-translation contract).
    /// TWO poles: one left the 89-250 band's top barely folded
    /// (late low IACC 0.71 vs real rooms' ~0.9, measured by the
    /// depth suite). Feed-forward side processing — the two-pole
    /// in-loop hazard does not apply here.
    side_lp: f64,
    side_lp2: f64,
    k_side: f64,
    k_side2: f64,
    /// THE DIAL GLIDE (worklist 7): every tune-time scalar walks
    /// to its target over ~40 ms instead of stepping per block —
    /// all six swept dials measured clicks 2.3-39x over both
    /// steady regimes (block-step coefficients, the audit
    /// ledger's prediction). Gliding the VALUES means every
    /// derived coefficient inherits the cure. Snapped on reset.
    glide: [f32; NGLIDE],
    glide_cold: bool,
    /// Per-sample glides for the width machinery (worklist-7
    /// doctrine): the ER fold scale and the coherence-corner
    /// coefficients step per block otherwise — width automation
    /// measured a 2.85x click without them.
    er_side_v: f32,
    k_side_v: f64,
    k_side2_v: f64,
    glide_just_snapped: bool,
    /// THE SUPERSTIMULUS STATE. LAUNCH: presence-tilt one-poles on
    /// the ER (the early-only impossible head lives in the tune
    /// call). BLOOM: its own dry-envelope follower (release = the
    /// dial). LIFT: the tail-time ramp + height-shelf one-poles.
    launch_lp: [f32; 2],
    /// The shared TRANSIENT detector (fast-minus-slow dry
    /// envelope): bloom folds on ONSETS and lift resets on them.
    /// Level-gating was the v1 bug — dense program never releases
    /// a level gate (bloom stayed folded = permanent narrowing;
    /// lift waited for silence music never provides).
    tr_fast: f32,
    tr_slow: f32,
    bloom_env: f32,
    lift_ramp: f32,
    lift_lp: [f32; 2],
    lift_lp2: [f32; 2],
    /// THE DUCK's dry-envelope follower + its coefficients.
    duck_env: f32,
    k_duck_a: f32,
    k_duck_r: f32,
    /// THE WET LIMITER, LOOKAHEAD: the wet runs 3 ms late (just
    /// predelay, inaudible) while the envelope reads the present —
    /// gain reaches its target BEFORE the peak arrives, so nothing
    /// is ever waveshaped. (The slow-attack + safety-shave pair
    /// was a contradiction: transients passed the limiter INTO a
    /// waveshaper — every kick tip bent; the field heard it as
    /// mid-bass distortion once the modal LF stacked peaks past
    /// the knee.)
    wet_la: [DelayLine; 2],
    la_samples: usize,
    lim_env: f32,
    k_lim_r: f32,
    /// Output-trim powf cache (audit: a powf per sample otherwise).
    last_db: f32,
    out_g: f32,
    /// Equal-power mix cache (two sqrts per sample otherwise).
    last_mix: f32,
    dry_g: f32,
    wet_g: f32,
}

impl stave_core::Processor for VaultEngine {
    type Params = VaultParams;

    fn new(params: Arc<VaultParams>) -> Self {
        Self {
            output_control: ContinuousControl::new(&params.output),
            early_control: ContinuousControl::new(&params.early),
            mix_control: ContinuousControl::new(&params.mix),
            predelay_control: ContinuousControl::new(&params.predelay),
            params,
            sr: 48_000.0,
            predelay: [DelayLine::new(), DelayLine::new()],
            room: Room::new(),
            plate: Plate::new(),
            memory: Memory::new(),
            det: Detonate::new(),
            void: Void::new(),
            topo: 0,
            topo_fade: 1.0,
            rerad: Rerad::new(),
            diffusers: [Diffuser::new(1.0), Diffuser::new(1.07)],
            mid: [
                Midfield::new(1.0),
                Midfield::new(1.031),
                Midfield::new(1.013),
                Midfield::new(1.043),
            ],
            tank: Tank::new(),
            modes: Modes::new(),
            side_lp: 0.0,
            side_lp2: 0.0,
            k_side: 0.03,
            k_side2: 0.02,
            glide: [0.0; NGLIDE],
            glide_cold: true,
            er_side_v: 0.45,
            k_side_v: 0.03,
            k_side2_v: 0.02,
            glide_just_snapped: true,
            launch_lp: [0.0; 2],
            tr_fast: 0.0,
            tr_slow: 0.0,
            bloom_env: 0.0,
            lift_ramp: 0.0,
            lift_lp: [0.0; 2],
            lift_lp2: [0.0; 2],
            duck_env: 0.0,
            k_duck_a: 0.01,
            k_duck_r: 0.0001,
            wet_la: [DelayLine::new(), DelayLine::new()],
            la_samples: 144,
            lim_env: 0.0,
            k_lim_r: 0.0001,
            last_db: f32::NAN,
            out_g: 1.0,
            last_mix: f32::NAN,
            dry_g: 1.0,
            wet_g: 0.0,
        }
    }

    fn activate(&mut self, env: Environment) {
        let rate = f64::from(env.sample_rate);
        self.output_control.prepare(rate, &self.params.output);
        self.early_control.prepare(rate, &self.params.early);
        self.mix_control.prepare(rate, &self.params.mix);
        self.predelay_control.prepare(rate, &self.params.predelay);
        self.sr = env.sample_rate;
        let cap = (0.25 * self.sr) as usize + 16;
        self.predelay[0].prepare(cap);
        self.predelay[1].prepare(cap);
        self.room.prepare(self.sr);
        self.plate.prepare(self.sr);
        self.memory.prepare(self.sr);
        self.det.prepare(self.sr);
        self.void.prepare(self.sr);
        self.rerad.prepare(self.sr);
        self.diffusers[0].prepare(self.sr);
        self.diffusers[1].prepare(self.sr);
        for m in self.mid.iter_mut() {
            m.prepare(self.sr);
        }
        self.tank.prepare(self.sr);
        self.modes.prepare(self.sr);
        self.k_side = 1.0 - (-core::f64::consts::TAU * 380.0 / f64::from(self.sr)).exp();
        self.k_side2 = 1.0 - (-core::f64::consts::TAU * 220.0 / f64::from(self.sr)).exp();
        self.side_lp = 0.0;
        self.side_lp2 = 0.0;
        self.k_duck_a = 1.0 - (-1.0 / (0.005 * self.sr)).exp();
        self.k_duck_r = 1.0 - (-1.0 / (0.220 * self.sr)).exp();
        self.duck_env = 0.0;
        // LOOKAHEAD limiting: 3 ms of wet delay, attack ~1 ms —
        // the gain settles before the peak arrives; no pumping
        // (release 120 ms) and no waveshaping, ever.
        self.la_samples = (0.003 * self.sr) as usize;
        self.wet_la[0].prepare(self.la_samples + 8);
        self.wet_la[1].prepare(self.la_samples + 8);
        self.k_lim_r = 1.0 - (-1.0 / (0.120 * self.sr)).exp();
        self.lim_env = 0.0;
        // A cold start is NOT a machine swap (see reset).
        self.topo = self.params.topology.index();
        self.topo_fade = 1.0;
    }

    fn reset(&mut self) {
        self.predelay[0].reset();
        self.predelay[1].reset();
        self.room.reset();
        self.plate.reset();
        self.memory.reset();
        self.det.reset();
        self.void.reset();
        // A cold start is NOT a machine swap: adopt the dialed
        // topology open (the swap fade is for LIVE switches only —
        // the scoreboard's sample-0 impulse measured 25 ms "slow"
        // density that was just the fade).
        self.topo = self.params.topology.index();
        self.topo_fade = 1.0;
        self.rerad.reset();
        self.diffusers[0].reset();
        self.diffusers[1].reset();
        for m in self.mid.iter_mut() {
            m.reset();
        }
        self.tank.reset();
        self.modes.reset();
        self.side_lp = 0.0;
        self.side_lp2 = 0.0;
        self.glide_cold = true;
        self.duck_env = 0.0;
        self.launch_lp = [0.0; 2];
        self.tr_fast = 0.0;
        self.tr_slow = 0.0;
        self.bloom_env = 0.0;
        self.lift_ramp = 0.0;
        self.lift_lp = [0.0; 2];
        self.lift_lp2 = [0.0; 2];
        self.lim_env = 0.0;
        self.wet_la[0].reset();
        self.wet_la[1].reset();
    }

    fn process(&mut self, ctx: &mut ProcessCtx<'_>) {
        let p = self.params.clone();
        let frames = ctx.frames;
        let sr = self.sr;

        // Block-rate voicing: aim the machine, then run it —
        // through the DIAL GLIDE (~40 ms): targets from the
        // params, walked values into every consumer.
        let mat_t = scene::material(p.material.index());
        let targets: [f32; NGLIDE] = [
            p.size.value(),
            p.decay.value() * 0.001,
            p.damp.value(),
            p.tone.value(),
            p.bass.value(),
            p.stand.value(),
            p.distance.value(),
            p.diffuse.value(),
            p.stiff.value(),
            mat_t.wall.0,
            mat_t.wall.1,
            mat_t.bass_mult,
            mat_t.hi_mult,
            mat_t.damp_add,
            p.width.value(),
            p.duck.value(),
        ];
        let snapped = self.glide_cold;
        if self.glide_cold {
            self.glide = targets;
            self.glide_cold = false;
        } else {
            let k_glide = 1.0 - (-(frames as f32 / sr) / 0.040).exp();
            for (g, t) in self.glide.iter_mut().zip(targets) {
                *g += k_glide * (t - *g);
            }
        }
        let [size, t60_s, damp, tone, bass_dial, stand, distance, diffuse, stiff, wall_g, wall_c, bass_mult, hi_mult, damp_add, width_g, duck] =
            self.glide;
        // THE IMPOSSIBLE, FOLDED (the dials died 2026-07-21 — the
        // behaviors live where their names already were):
        // * LAUNCH is what CLOSE means: below distance 0.5 the
        //   projection engages, full at 0 — pull the stage at you
        //   and the image steps out of the speakers.
        // * BLOOM is what DUCK's release does: the wet re-blooms
        //   both louder AND wider into the gaps.
        // * LIFT is the house signature, scaled by SIZE — tall
        //   spaces rise more (H grows with size; the tail's
        //   elevation belongs to the architecture).
        let launch = (1.0 - distance / 0.5).clamp(0.0, 1.0);
        let bloom = duck;
        let lift = 0.45 + 0.15 * size.clamp(0.3, 3.0);
        self.glide_just_snapped = snapped;
        let wall = (wall_g, wall_c);
        let rate_scale = sr / 48_000.0;
        // MOTION is BAKED (the dial died 2026-07-21): its job is
        // anti-metallic hygiene — users only ever notice its absence.
        let motion = 0.35f32;
        let chorus_samples = motion * motion * MOTION_SPAN * rate_scale;
        // THE TOPOLOGY: which machine. A switch fades the wet in
        // from silence (~30 ms) — a machine swap, not a dial.
        let topo = p.topology.index();
        if topo != self.topo {
            self.topo = topo;
            self.topo_fade = 0.0;
        }
        let plate_on = topo == 1;
        let memory_on = topo == 2;
        let det_on = topo == 3;
        let void_on = topo == 4;
        let room_on = topo == 0;
        // Only the ROOM has walls: materials are inert elsewhere —
        // plate/memory take the raw dials.
        let (damp, bass) = if !room_on {
            (damp.clamp(0.0, 1.0), bass_dial.clamp(0.2, 4.5))
        } else {
            (
                (damp + damp_add).clamp(0.0, 1.0),
                (bass_dial * bass_mult).clamp(0.2, 4.5),
            )
        };
        // Absorption cutoff sweeps 18 kHz (open air) → 800 Hz
        // (heavy drapes) on a log ramp.
        let cutoff = 18_000.0 * (800.0f32 / 18_000.0).powf(damp);
        let damp_k = 1.0 - (-core::f32::consts::TAU * cutoff / sr).exp();
        // WIDTH IS THE GEOMETRY (the stereo-honesty slice): above
        // natural the ears move APART in the scene (the impossible
        // head, up to ~4x); below natural they move TOGETHER —
        // coincident ears hear one field, so narrow-to-mono falls
        // out of the same description instead of a post M/S trim.
        let ear_scale = if width_g >= 1.0 {
            1.0 + 1.5 * (width_g - 1.0)
        } else {
            width_g.max(0.04)
        };
        // The ER coherence fold (worklist 1), open above width 1.
        // LAUNCH opens it further — deliberately past nature (the
        // superstimulus premise): the early lateral energy the
        // widened ears create must REACH the ears, and the fold
        // was crushing it (measured: geometry alone moved early
        // side/mid 0.179 -> 0.181; the fold owns that number).
        let er_side =
            ((0.45 + 0.55 * (width_g - 1.0).clamp(0.0, 1.0)) * (1.0 + 0.75 * launch)).min(1.0);
        // THE COHERENCE CORNERS TRACK THE EARS: diffuse-field
        // interaural coherence lives below c/(2·ear distance) —
        // twice the ear span puts the corner an octave lower.
        // The LF side folds ARE the coherence model, so their
        // corners scale with 1/ear_scale: hyper-wide ears open
        // the low field (the LATE width the deleted M/S boost was
        // faking), coincident ears fold nearly everything to mid
        // (narrow-to-mono with no trim anywhere). The plate keeps
        // corners at natural (its width is the pickups' business).
        let corner_scale = if !room_on {
            1.0
        } else {
            (1.0 / ear_scale).clamp(0.25, 24.0)
        };
        // The coincident LIMIT, made exact: over the dial's last
        // 5% the residual side (what the risen corners leave)
        // finishes to zero — width 0 ships BIT-mono wet.
        let side_gate = if !room_on {
            1.0
        } else {
            (width_g / 0.05).clamp(0.0, 1.0)
        };
        let k_side_t =
            1.0 - (-core::f64::consts::TAU * 380.0 * f64::from(corner_scale) / f64::from(sr)).exp();
        let k_side2_t =
            1.0 - (-core::f64::consts::TAU * 220.0 * f64::from(corner_scale) / f64::from(sr)).exp();
        if self.glide_just_snapped {
            self.k_side_v = k_side_t;
            self.k_side2_v = k_side2_t;
            self.er_side_v = er_side;
        }
        let nf = frames.max(1) as f32;
        let k_side_step = (k_side_t - self.k_side_v) / f64::from(nf);
        let k_side2_step = (k_side2_t - self.k_side2_v) / f64::from(nf);
        let er_side_step = (er_side - self.er_side_v) / nf;
        // SUPERSTIMULUS coefficients (all zero-cost at dial 0).
        let launch_tilt = 0.75 * launch;
        let k_launch = 1.0 - (-core::f32::consts::TAU * 3_000.0 / sr).exp();
        // BLOOM v3 is ADDITIVE: the attack stays natural and the
        // tail blooms WIDER than the room after every hit (v2
        // folded side away during hits — deleting side energy IS
        // damping + narrowing, the field heard it exactly).
        let bloom_boost = 2.1 * bloom;
        let k_tr_fast = 1.0 - (-1.0 / (0.008 * sr)).exp();
        let k_tr_slow = 1.0 - (-1.0 / (0.2 * sr)).exp();
        let k_bloom_rise = 1.0 - (-1.0 / ((0.08 + 0.5 * bloom) * sr)).exp();
        let lift_amt = 4.6 * lift;
        let k_lift_rise = 1.0 - (-1.0 / (1.1 * sr)).exp();
        let k_lift_lp = 1.0 - (-core::f32::consts::TAU * 5_200.0 / sr).exp();
        if plate_on {
            self.plate.tune(
                size, t60_s, bass, stiff, diffuse, tone, motion, width_g, frames,
            );
        } else if memory_on {
            self.memory.tune(
                size,
                t60_s,
                bass,
                damp,
                tone,
                diffuse,
                width_g,
                p.ascend.value(),
                frames,
            );
        } else if det_on {
            self.det.tune(
                size,
                t60_s,
                bass,
                damp,
                tone,
                diffuse,
                width_g,
                p.fuse.value(),
                frames,
            );
        } else if void_on {
            // The void's high hold derives from the HIGH ENERGY
            // dial as a T60 multiplier (no walls to absorb).
            let hi_mult = (1.0 - 0.85 * damp).max(0.08);
            self.void.tune(
                size,
                t60_s,
                bass,
                hi_mult,
                tone,
                diffuse,
                width_g,
                p.dawn.value(),
                frames,
            );
        } else {
            // LAUNCH: the EARLY-ONLY impossible head — the ER's
            // ears widen past nature while the late field's
            // coherence model stays honest, so precedence carries
            // the image out of the speakers without blurring it.
            // ITD stays inside the fusion window (~1 ms — beyond
            // it the image SPLITS instead of stepping forward; the
            // first cut used 2.2x ears and the coherence law read
            // IACC 0.13: blur, not precedence). The heavy lifting
            // moves to ILD: launch DEEPENS the head shadow.
            let ear_early = ear_scale * (1.0 + 0.35 * launch);
            self.room
                .tune(size, stand, distance, ear_early, tone, wall, launch, frames);
        }
        let mfp = scene::Scene::describe(size, stand).mean_free_path_s();
        let _ = &mfp;
        let bal = self.room.balance() * 0.35;
        let bal_norm = (2.0 / ((1.0 + bal).powi(2) + (1.0 - bal).powi(2))).sqrt();
        let (lean_l, lean_r) = ((1.0 + bal) * bal_norm, (1.0 - bal) * bal_norm);
        if room_on {
            for m in self.mid.iter_mut() {
                m.tune(mfp, diffuse, size, frames);
            }
            // The wet field LEANS toward the source (geometry-derived,
            // applied to the wet only — the dry direct stays the
            // host's business). Equal-power lean, ±~3 dB at the walls.
            self.tank
                .tune(size, t60_s, bass, hi_mult, diffuse, chorus_samples, frames);

            self.rerad.tune(p.material.index());
            self.diffusers[0].tune(size, diffuse, frames);
            self.diffusers[1].tune(size, diffuse, frames);
            self.modes
                .tune(size, stand, distance, t60_s, bass, frames as f32 / sr);
        }

        let mut out_ramp = self.output_control.begin_block(ctx.params, &p.output);
        let mut early_ramp = self.early_control.begin_block(ctx.params, &p.early);
        let mut mix_ramp = self.mix_control.begin_block(ctx.params, &p.mix);
        let mut pre_ramp = self.predelay_control.begin_block(ctx.params, &p.predelay);

        let k_side_v = &mut self.k_side_v;
        let k_side2_v = &mut self.k_side2_v;
        let er_side_v = &mut self.er_side_v;
        let launch_lp = &mut self.launch_lp;
        let tr_fast = &mut self.tr_fast;
        let tr_slow = &mut self.tr_slow;
        let bloom_env = &mut self.bloom_env;
        let lift_ramp = &mut self.lift_ramp;
        let lift_lp = &mut self.lift_lp;
        let lift_lp2 = &mut self.lift_lp2;
        let side_lp = &mut self.side_lp;
        let side_lp2 = &mut self.side_lp2;
        let (k_duck_a, k_duck_r) = (self.k_duck_a, self.k_duck_r);
        let duck_env = &mut self.duck_env;
        let k_lim_r = self.k_lim_r;
        let lim_env = &mut self.lim_env;
        let wet_la = &mut self.wet_la;
        let la = self.la_samples;
        let last_db = &mut self.last_db;
        let out_g = &mut self.out_g;
        let predelay = &mut self.predelay;
        let room = &mut self.room;
        let plate = &mut self.plate;
        let memory = &mut self.memory;
        let det = &mut self.det;
        let void = &mut self.void;
        let topo_fade = &mut self.topo_fade;
        let k_fade = (frames as f32 / sr / 0.030).min(1.0) / frames.max(1) as f32;
        let rerad = &mut self.rerad;
        let mid = &mut self.mid;
        let diffusers = &mut self.diffusers;
        let tank = &mut self.tank;
        let modes = &mut self.modes;
        let last_mix = &mut self.last_mix;
        let (dry_gc, wet_gc) = (&mut self.dry_g, &mut self.wet_g);
        let lanes = ctx.lanes::<2>();
        lanes.run(|fr| {
            let dry_l = fr.input(0);
            let dry_r = fr.input(1);
            // The wet onset: glided predelay (read before write).
            let pre = (pre_ramp.next() * 0.001 * sr).max(1.0);
            let pd_l = predelay[0].read_cubic(pre);
            let pd_r = predelay[1].read_cubic(pre);
            predelay[0].write(dry_l);
            predelay[0].advance();
            predelay[1].write(dry_r);
            predelay[1].advance();
            // THE ROOM answers first (the source is the mid of
            // the predelayed dry); the late field then grows OUT
            // of the room's answer — diffused ER feeds the tank,
            // never the raw input (early and late can no longer
            // disagree about where they are).
            // The late field grows from the SECOND-ORDER energy
            // (feed_l/feed_r): its onset trails the early field
            // and scales with the room — the C80/EDT arc a
            // same-instant feed could not make.
            // TRUE STEREO: both channels enter the scene as their
            // own sources (review catch: folding to mono at the
            // door discarded the input's spatial field).
            // THE TOPOLOGY BRANCH: one machine plays, the other
            // does not even tick — the CPU answer is architectural.
            let (wl, wr) = if plate_on {
                let (pl, pr) = plate.tick(pd_l, pd_r, damp_k);
                (pl * PLATE_MAKEUP, pr * PLATE_MAKEUP)
            } else if memory_on {
                let (ml, mr) = memory.tick(pd_l, pd_r);
                (ml * MEMORY_MAKEUP, mr * MEMORY_MAKEUP)
            } else if det_on {
                let (dl, dr) = det.tick(pd_l, pd_r, damp_k);
                (dl * DET_MAKEUP, dr * DET_MAKEUP)
            } else if void_on {
                let (vl, vr) = void.tick(pd_l, pd_r);
                (vl * VOID_MAKEUP, vr * VOID_MAKEUP)
            } else {
                let (er_l, er_r, feed_l, feed_r) = room.tick(pd_l, pd_r);
                // THE MID-FIELD: every discrete arrival grows a
                // densifying tail at the room's own bounce interval
                // (size-gated — it fills the order-2 truncation gap,
                // which only opens in big rooms). The tank keeps its
                // second-order feed (slice 4's C80 structure) through
                // its own mid-field pair.
                // MATERIAL RE-RADIATION: the walls answer the field
                // that struck them — the ring joins the ER path and
                // wears its gain, coherence fold and lean.
                let (rr_l, rr_r) = rerad.tick(er_l, er_r);
                let er_l = mid[0].tick(er_l + rr_l);
                let er_r = mid[1].tick(er_r + rr_r);
                let e_l = diffusers[0].tick(mid[2].tick(feed_l));
                let e_r = diffusers[1].tick(mid[3].tick(feed_r));
                let (late_l, late_r) = tank.tick(e_l * INJECT, e_r * INJECT, damp_k);
                // THE MODES: the standing waves charge from the same
                // predelayed source; their voice is mono (the field's
                // lows fold to mid regardless) and un-leaned (a mode
                // fills the room; it does not arrive from a side).
                let lf = modes.tick(pd_l, pd_r);
                let early = early_ramp.next() * 1.5;
                // WORKLIST 1 — EARLY-COHERENCE EVOLUTION: the field
                // must localize first and envelop second (the depth
                // suite read mid/high IACC flat at 0.05-0.14 from
                // window one — the anti-"jumping-out" signature). The
                // ER alone folds partially toward mid; the tank stays
                // fully decorrelated, so coherence EVOLVES early→late
                // by construction. Above WIDTH 1 the fold opens again
                // — the impossible head keeps its hyper-wide attack.
                // LAUNCH's projection: a presence tilt on the ER only
                // (the attack steps forward; the tail stays put).
                let (er_l, er_r) = if launch_tilt > 1e-4 {
                    launch_lp[0] += k_launch * (er_l - launch_lp[0]);
                    launch_lp[1] += k_launch * (er_r - launch_lp[1]);
                    (
                        er_l + launch_tilt * (er_l - launch_lp[0]),
                        er_r + launch_tilt * (er_r - launch_lp[1]),
                    )
                } else {
                    (er_l, er_r)
                };
                let er_m = 0.5 * (er_l + er_r);
                let er_s = 0.5 * (er_l - er_r) * *er_side_v;
                *er_side_v += er_side_step;
                let er_l = er_m + er_s;
                let er_r = er_m - er_s;
                let wl = (er_l * early + late_l) * lean_l + lf;
                let wr = (er_r * early + late_r) * lean_r + lf;
                (wl, wr)
            };
            // The topology fade: a machine swap enters from
            // silence over ~30 ms (the robustness doctrine — a
            // swap is a rebuild, never a click).
            let wl = wl * *topo_fade;
            let wr = wr * *topo_fade;
            *topo_fade = (*topo_fade + k_fade).min(1.0);
            // The field: width allocates side, never mirrors —
            // and the side's LOWS fold to mid (interaural
            // coherence below ~250 Hz is how real rooms behave,
            // and how wide mixes survive mono).
            let (m, s) = ms_encode(wl * WET_MAKEUP, wr * WET_MAKEUP);
            // WORKLIST 2 — the deep anchor done phase-safely: TWO
            // SEQUENTIAL mild one-pole folds (250 then 140 Hz,
            // independent states). Each subtraction stays phase-
            // benign; magnitudes compound. (One deep 2-pole
            // subtraction rotated -54 deg and measured WORSE:
            // 0.71 -> 0.56.)
            *side_lp += *k_side_v * (s - *side_lp);
            let s = s - 0.86 * *side_lp;
            *side_lp2 += *k_side2_v * (s - *side_lp2);
            let s = s - 0.84 * *side_lp2;
            *k_side_v += k_side_step;
            *k_side2_v += k_side2_step;
            // WIDTH IS THE GEOMETRY, fully: no post M/S in either
            // direction. Wide = the impossible head + the ER fold
            // opening + the coherence corners dropping; narrow =
            // ears converging + corners rising until the folds
            // carry the whole spectrum to mid. The plate does all
            // of it with its pickups.
            // BLOOM: coherence choreographed in TIME — each hit
            // folds the field to the center and it OPENS around
            // the listener as the dry releases (a trajectory no
            // room can have; at 0 the fold is bit-absent).
            let s = if bloom_boost > 1e-4 || lift_amt > 1e-4 {
                // The shared transient detector, RATIO form (a
                // difference detector fires on steady tones'
                // waveform ripple — measured): fast/slow rides
                // ~1.1 on sustains, spikes 2-4x on hits.
                let x = 0.5 * (dry_l.abs() + dry_r.abs());
                *tr_fast += k_tr_fast * (x - *tr_fast);
                *tr_slow += k_tr_slow * (x - *tr_slow);
                let onset = ((*tr_fast / (*tr_slow).max(1e-6) - 1.35) * 1.6).clamp(0.0, 1.0);
                if bloom_boost > 1e-4 {
                    // ADDITIVE: the ramp rises through each tail
                    // (rise time = the dial) and every hit pulls
                    // it back — the field blooms OUTWARD from the
                    // natural width; nothing is ever removed.
                    *bloom_env += k_bloom_rise * (1.0 - *bloom_env);
                    *bloom_env *= 1.0 - 0.08 * onset;
                    s * f64::from(1.0 + bloom_boost * *bloom_env)
                } else {
                    s
                }
            } else {
                s
            };
            let (mut wl, mut wr) = ms_decode(m, s * f64::from(side_gate));
            // THE DUCK: the wet compresses under the dry's envelope
            // (5 ms attack / 220 ms release) and blooms into the
            // gaps — loud AND clear, the companding answer to
            // "reverb loudness".
            if duck > 1e-3 {
                let x = 0.5 * (dry_l.abs() + dry_r.abs());
                let k = if x > *duck_env { k_duck_a } else { k_duck_r };
                *duck_env += k * (x - *duck_env);
                let gr = 1.0 - duck * 0.8 * (*duck_env * 2.5).min(1.0);
                wl *= gr;
                wr *= gr;
            }
            // LIFT: the tail RISES — Blauert "above" energy grows
            // while the dry rests and resets on each new hit (an
            // elevation TRAJECTORY, not a filter; bit-absent at 0).
            if lift_amt > 1e-4 {
                // Onset-reset (v1 gated on SILENCE, which music
                // never provides — the dial did nothing on
                // program): every hit pulls the ramp down, every
                // tail lets it rise.
                let onset = ((*tr_fast - *tr_slow) * 6.0).clamp(0.0, 1.0);
                *lift_ramp += k_lift_rise * (1.0 - *lift_ramp);
                *lift_ramp *= 1.0 - 0.05 * onset;
                let g = lift_amt * *lift_ramp;
                // TWO poles under the complement: the one-pole
                // skirt leaked the boost into the mids and diluted
                // the trajectory (measured).
                lift_lp[0] += k_lift_lp * (wl - lift_lp[0]);
                lift_lp2[0] += k_lift_lp * (lift_lp[0] - lift_lp2[0]);
                lift_lp[1] += k_lift_lp * (wr - lift_lp[1]);
                lift_lp2[1] += k_lift_lp * (lift_lp[1] - lift_lp2[1]);
                wl += g * (wl - lift_lp2[0]);
                wr += g * (wr - lift_lp2[1]);
            }
            // THE 0 dB GUARD, LOOKAHEAD: the envelope reads the
            // PRESENT while the wet plays 3 ms late — gain settles
            // before each peak arrives. No waveshaping (the shave
            // bent every kick tip once the modal LF stacked peaks
            // past its knee — the field's mid-bass distortion,
            // measured 0.36 share), no pumping (1 ms attack only
            // ever moves gain in the inaudible pre-peak window).
            let a = wl.abs().max(wr.abs());
            // Instantaneous-max envelope (a one-pole attack missed
            // single-sample crests — peak 1.53, measured): env is
            // >= every peak by construction, decays at the release,
            // and the 3 ms delay guarantees the gain is already in
            // place when the peak plays.
            *lim_env = a.max(*lim_env * (1.0 - k_lim_r));
            // THE AIRBAG, not a ceiling (design decision, on the
            // record): honesty over normalization — a longer, hotter
            // room IS louder, and near-0 dB sources may breathe
            // past full scale in float. 1.35 (+2.6 dB) catches only
            // runaway (parked resonances, pathological corners);
            // the idle-limiter law FORBIDS it from working on
            // ordinary program.
            const LIMIT: f32 = 1.35;
            wet_la[0].write(wl);
            wet_la[0].advance();
            wet_la[1].write(wr);
            wet_la[1].advance();
            let mut wl = wet_la[0].read_int(la);
            let mut wr = wet_la[1].read_int(la);
            if *lim_env > LIMIT {
                let g_lim = LIMIT / *lim_env;
                wl *= g_lim;
                wr *= g_lim;
            }
            // EQUAL-POWER mix (the linear crossfade under-weighted
            // the wet another ~10 dB at working settings): mix 0
            // stays BIT-dry (sqrt(1) = 1 exactly, wet x0).
            let mix = mix_ramp.next();
            if mix != *last_mix {
                *last_mix = mix;
                *dry_gc = (1.0 - mix).max(0.0).sqrt();
                *wet_gc = mix.max(0.0).sqrt();
            }
            let (dry_g, wet_g) = (*dry_gc, *wet_gc);
            // powf only when the trim MOVES (audit: this was a
            // powf per sample; the ramp is constant almost always).
            let db = out_ramp.next();
            if db != *last_db {
                *last_db = db;
                *out_g = db_to_amp(db);
            }
            let g = *out_g;
            fr.write(0, (dry_l * dry_g + wl * wet_g) * g);
            fr.write(1, (dry_r * dry_g + wr * wet_g) * g);
        });
    }

    fn latency_samples(&self) -> u32 {
        0
    }

    /// Honest from the knobs: the LONGEST band's T60 — the low
    /// shelf reaches decay x bass x 1.45 (review catch: the
    /// broadband-only figure under-reported the 30 s x 4 corner
    /// ~4x, inviting hosts to truncate the tail) — plus a park
    /// margin and the predelay.
    fn tail(&self) -> Tail {
        let base = self.params.decay.value() * 0.001;
        let longest = base * (self.params.bass.value() * 1.45).max(1.0);
        let secs = longest * 1.4 + 0.35;
        Tail::Samples((secs * self.sr) as u32)
    }
}
