//! Diffusion — the FIELD: a second resonant system coupled to the
//! delay line by three independent knobs, so the wash has its own
//! clock instead of being slaved to the repeat period:
//! * DIFFUSE — the HEARD morph: repeat ↔ field, for your ears.
//! * BLEED — the REGENERATION morph: what re-enters the loop. On
//!   the diagonal (bleed = diffuse, hold = 0) this reproduces the
//!   in-loop compounding; at bleed 0 the field blooms AROUND an
//!   articulate delay; at diffuse 0 / bleed up, first arrivals stay
//!   sharp while the loop secretly dissolves (ghost mode).
//! * HOLD — the field's OWN recirculation: it rings between
//!   repeats, however far apart they are.
//!
//! The couplings live in `couple` below; the tail-quality laws
//! (density, comb-ness, decay smoothness) and the coupled-system
//! stability laws measure what the voices actually deliver.
//!
//! Four voices:
//! * PLATE — a figure-8 TANK in the Dattorro/Lexicon lineage: input
//!   diffusion per lane, then two cross-coupled branches, each a
//!   MODULATED allpass → delay → damping → allpass → delay. The
//!   single-loop lattice this replaced rang as a 22 ms comb — a
//!   flangy delay tail, not a reverb. The tank's half-lap is
//!   ~150 ms of incommensurate delays, the in-tank modulation is a
//!   few SAMPLES of slow wander (micropitch that dissolves the
//!   remaining modes without spiraling), and the damping darkens
//!   each pass the way air does.
//! * PRISM — dispersion proper: a 96-stage first-order allpass line
//!   (a dispersive transmission line); each pass chirps, and the
//!   field loop makes chirps of chirps.
//! * DUST — diffusion by tap cloud: micro-taps walking inside the
//!   spray radius, gains summing to one. Echoes crumble; the cloud
//!   sheds energy every lap — that IS the voice.
//! * MESH — a 4-line FDN mixed by ROTATIONS IN (mid, side) SPACE
//!   (ORBIT's math grown up), golden-detuned so no rotation axis is
//!   lossless, now DAMPED per line and with a few samples of slow
//!   line motion — energy diffuses through time and the image
//!   together, and the late field breathes instead of ringing.
//!
//! DIFFUSE = 0 with BLEED = 0 bypasses the stage, which keeps every
//! algorithm's law bit-identical (a law test).

use stave_dsp::Xorshift32;
use stave_dsp::{Allpass1, DelayLine, PerChannel};

use crate::params::EddyParams;

/// PLATE input-diffusion lengths (samples at 48 kHz, rescaled) —
/// short coprime pairs, one set per lane.
const PLATE_IN_MID: [f32; 2] = [149.0, 293.0];
const PLATE_IN_SIDE: [f32; 2] = [163.0, 307.0];

/// PLATE tank geometry (samples at 48 kHz, rescaled): per branch, a
/// modulated allpass, the first long delay (with its output tap
/// offsets), the second allpass, the second delay. Incommensurate
/// across branches — the figure-8 lap is ~300 ms with no common
/// period.
const TANK_MOD_AP: [f32; 2] = [672.0, 908.0];
const TANK_D1: [f32; 2] = [4_453.0, 4_217.0];
const TANK_AP2: [f32; 2] = [1_800.0, 2_656.0];
const TANK_D2: [f32; 2] = [3_720.0, 3_163.0];
/// Early tap offsets into each branch's first delay — the tank's
/// output is a SPREAD of points, so a single pass reads as an
/// early-reflection bloom, never one discrete echo.
const TANK_TAPS: [f32; 3] = [271.0, 1_157.0, 2_723.0];
/// In-tank modulation depth at full WANDER, in SAMPLES — absolute,
/// not proportional: a few samples of slow motion is micropitch;
/// tens of samples inside a recirculation is a parametric pump
/// (the doctrine, measured three times).
const TANK_MOD_DEPTH: f32 = 9.0;
/// In-tank damping: one-pole lowpass corner. Each half-lap darkens
/// the way air and iron do — an undamped tail rings statically.
const TANK_DAMP_HZ: f32 = 5_800.0;

/// PRISM's dispersive line length.
const PRISM_STAGES: usize = 96;

/// DUST cloud: tap count and the spray ceiling.
const MOTE_COUNT: usize = 7;
const SPRAY_MAX_MS: f32 = 42.0;

/// MESH line lengths (ms) — coprime-ish strands, scaled by SIZE.
const MESH_MS: [f32; 4] = [29.7, 41.3, 56.9, 78.1];
/// MESH line motion depth (samples) and damping corner.
const MESH_MOD_DEPTH: f32 = 4.0;
const MESH_DAMP_HZ: f32 = 6_500.0;

/// A Schroeder allpass on a cubic-read line: `w = x + g·v;
/// y = v − g·w` (unit-magnitude for any g < 1). The delay EASES
/// toward a target that RENEWS at intervals — renewal is what gives
/// a walk its reach (white noise through a slow one-pole alone has
/// almost no variance; the flutter model taught us). `depth` is in
/// absolute samples; 0 = static.
struct Ap {
    line: DelayLine,
    len: f32,
    drift: f32,
    target: f32,
}

impl Ap {
    fn new() -> Self {
        Self {
            line: DelayLine::new(),
            len: 0.0,
            drift: 0.0,
            target: 0.0,
        }
    }

    fn prepare(&mut self, len: f32, margin: f32) {
        self.len = len;
        self.drift = 0.0;
        self.target = 0.0;
        self.line.prepare((len + margin + 8.0) as usize);
        self.line.reset();
    }

    fn reset(&mut self) {
        self.line.reset();
        self.drift = 0.0;
    }

    #[inline]
    fn tick(&mut self, x: f32, g: f32, depth: f32, ease: f32) -> f32 {
        self.drift += (self.target - self.drift) * ease;
        let v = self.line.read_cubic(self.len + depth * self.drift);
        let w = x + g * v;
        self.line.write(w);
        self.line.advance();
        v - g * w
    }
}

/// One branch of the plate tank: modulated allpass → first delay
/// (tapped) → damping → second allpass → second delay.
struct Branch {
    mod_ap: Ap,
    d1: DelayLine,
    d1_len: f32,
    damp: f32,
    ap2: Ap,
    d2: DelayLine,
    d2_len: f32,
}

impl Branch {
    fn new() -> Self {
        Self {
            mod_ap: Ap::new(),
            d1: DelayLine::new(),
            d1_len: 0.0,
            damp: 0.0,
            ap2: Ap::new(),
            d2: DelayLine::new(),
            d2_len: 0.0,
        }
    }

    fn reset(&mut self) {
        self.mod_ap.reset();
        self.d1.reset();
        self.damp = 0.0;
        self.ap2.reset();
        self.d2.reset();
    }
}

struct DustTap {
    /// Smoothed 0..1 position walker and its renewing target.
    walk: f32,
    target: f32,
    gain: f32,
}

pub(super) struct Diffusion {
    kind: usize,
    /// The field's own recirculation for the SERIAL voices (PRISM,
    /// DUST): per-lane previous output, re-injected with the hold
    /// gain. PLATE and MESH recirculate inside their own structures.
    field_prev: [f32; 2],
    hold_g: f32,
    // PLATE
    plate_in: PerChannel<[Ap; 2], 2>,
    tank: [Branch; 2],
    plate_g: f32,
    plate_mod: f32,
    tank_damp_k: f32,
    // PRISM
    prism: PerChannel<[Allpass1; PRISM_STAGES], 2>,
    prism_depth: f32,
    // DUST
    dust_lines: PerChannel<DelayLine, 2>,
    dust_taps: [DustTap; MOTE_COUNT],
    dust_spray: f32,
    // MESH
    mesh: [DelayLine; 4],
    mesh_len: [f32; 4],
    mesh_drift: [f32; 4],
    mesh_target: [f32; 4],
    mesh_damp: [f32; 4],
    mesh_damp_k: f32,
    mesh_cos: f32,
    mesh_sin: f32,
    mesh_cos2: f32,
    mesh_sin2: f32,
    /// One deterministic walker driver for every wandering part,
    /// and the renewal countdown that gives the walks their reach.
    rng: Xorshift32,
    renew: u32,
    sr: f32,
}

impl Diffusion {
    pub(super) fn new() -> Self {
        Self {
            kind: 0,
            field_prev: [0.0; 2],
            hold_g: 0.0,
            plate_in: PerChannel::new(|_| [Ap::new(), Ap::new()]),
            tank: [Branch::new(), Branch::new()],
            plate_g: 0.55,
            plate_mod: 0.5,
            tank_damp_k: 0.5,
            prism: PerChannel::new(|_| [Allpass1::new(); PRISM_STAGES]),
            prism_depth: 0.0,
            dust_lines: PerChannel::new(|_| DelayLine::new()),
            dust_taps: core::array::from_fn(|_| DustTap {
                walk: 0.0,
                target: 0.0,
                gain: 0.0,
            }),
            dust_spray: 0.5,
            mesh: core::array::from_fn(|_| DelayLine::new()),
            mesh_len: [0.0; 4],
            mesh_drift: [0.0; 4],
            mesh_target: [0.0; 4],
            mesh_damp: [0.0; 4],
            mesh_damp_k: 0.5,
            mesh_cos: 1.0,
            mesh_sin: 0.0,
            mesh_cos2: 1.0,
            mesh_sin2: 0.0,
            rng: Xorshift32::new(0x51F3_7A2B),
            renew: 0,
            sr: 48_000.0,
        }
    }

    pub(super) fn prepare(&mut self, sr: f32) {
        self.sr = sr;
        let rescale = sr / 48_000.0;
        for (lane_i, lane) in self.plate_in.iter_mut().enumerate() {
            let lens = if lane_i == 0 {
                PLATE_IN_MID
            } else {
                PLATE_IN_SIDE
            };
            for (ap, len) in lane.iter_mut().zip(lens) {
                ap.prepare(len * rescale, 0.0);
            }
        }
        for (i, br) in self.tank.iter_mut().enumerate() {
            br.mod_ap
                .prepare(TANK_MOD_AP[i] * rescale, TANK_MOD_DEPTH + 4.0);
            br.d1_len = TANK_D1[i] * rescale;
            br.d1.prepare(br.d1_len as usize + 8);
            br.d1.reset();
            br.damp = 0.0;
            br.ap2.prepare(TANK_AP2[i] * rescale, 0.0);
            br.d2_len = TANK_D2[i] * rescale;
            br.d2.prepare(br.d2_len as usize + 8);
            br.d2.reset();
        }
        self.tank_damp_k = 1.0 - (-2.0 * core::f32::consts::PI * TANK_DAMP_HZ / sr).exp();
        for lane in self.prism.iter_mut() {
            for ap in lane.iter_mut() {
                ap.prepare(sr);
            }
        }
        for line in self.dust_lines.iter_mut() {
            line.prepare((SPRAY_MAX_MS * 0.001 * sr) as usize + 8);
            line.reset();
        }
        // The cloud is BORN OPEN: walks seeded scattered, or the
        // first second after load reads through a single coincident
        // tap — no scatter at all (a probe caught the closed cloud).
        for i in 0..MOTE_COUNT {
            let w = self.rng.next_bipolar().abs();
            self.dust_taps[i].walk = w;
            self.dust_taps[i].target = w;
        }
        for (i, line) in self.mesh.iter_mut().enumerate() {
            line.prepare((MESH_MS[i] * 1.6 * 0.001 * sr + MESH_MOD_DEPTH + 8.0) as usize);
            line.reset();
        }
        self.mesh_damp = [0.0; 4];
        self.mesh_damp_k = 1.0 - (-2.0 * core::f32::consts::PI * MESH_DAMP_HZ / sr).exp();
        self.field_prev = [0.0; 2];
    }

    /// Param changes become structure, block-rate: a voice switch
    /// resets ALL field state (stale clouds must not thump in); the
    /// active voice's texture constants settle.
    pub(super) fn apply_changes(&mut self, p: &EddyParams) {
        let kind = p.diffusion.index();
        if kind != self.kind {
            self.kind = kind;
            for lane in self.plate_in.iter_mut() {
                for ap in lane.iter_mut() {
                    ap.reset();
                }
            }
            for br in self.tank.iter_mut() {
                br.reset();
            }
            for lane in self.prism.iter_mut() {
                for ap in lane.iter_mut() {
                    ap.reset();
                }
            }
            for line in self.dust_lines.iter_mut() {
                line.reset();
            }
            for line in self.mesh.iter_mut() {
                line.reset();
            }
            self.mesh_damp = [0.0; 4];
            self.field_prev = [0.0; 2];
        }
        // HOLD → the voice's recirculation gain. Ceilings differ by
        // lap period (the plate's figure-8 lap is ~300 ms, so a
        // lower gain already rings for seconds); all leave headroom
        // for the coupled products.
        let hold = p.hold.value();
        self.hold_g = match self.kind {
            0 => 0.82 * hold,
            1 => 0.90 * hold,
            // DUST's ceiling: walking taps inside a 0.9 loop grew
            // 1.47× (the doctrine's fourth appearance). The later
            // "pump" readings at 0.75 turned out to be amplitude
            // WANDER, not growth — sum-to-one taps are peak-passive,
            // and at bleed 0 no coupling even exists (isolated by
            // probe); the stability law now measures the TREND.
            2 => 0.75 * hold,
            _ => 0.88 * hold,
        };
        match self.kind {
            0 => {
                self.plate_g = 0.45 + 0.3 * p.density.value();
                self.plate_mod = p.wander.value();
            }
            1 => {
                // BEND: where the chirp bends — 90 Hz (deep whale)
                // to 1.8 kHz (tight zap). Dispersion is physically a
                // low-frequency phenomenon (allpass group delay goes
                // as 1/f), so the knob lives where the chirp does.
                let hz = 90.0 * 20f32.powf(p.bend.value());
                for lane in self.prism.iter_mut() {
                    for ap in lane.iter_mut() {
                        ap.set_frequency(hz);
                    }
                }
                // Squared taper: resolution where the smear is subtle.
                let s = p.smear.value();
                self.prism_depth = s * s * (PRISM_STAGES - 1) as f32;
            }
            2 => {
                self.dust_spray = p.spray.value();
                // MOTES morphs the cloud's weight curve from one
                // dominant grain toward fine, even dust; amplitude
                // sum-to-one keeps the cloud's coherent gain at one.
                let motes = p.motes.value();
                let sharp = 1.0 / (0.15 + 1.6 * motes);
                let mut sum = 0.0f32;
                for (i, tap) in self.dust_taps.iter_mut().enumerate() {
                    tap.gain = (1.0 - i as f32 / MOTE_COUNT as f32).powf(sharp);
                    sum += tap.gain;
                }
                for tap in self.dust_taps.iter_mut() {
                    tap.gain /= sum;
                }
            }
            _ => {
                let size = 0.35 + 1.15 * p.size.value();
                for i in 0..4 {
                    self.mesh_len[i] = MESH_MS[i] * size * 0.001 * self.sr;
                }
                let weave = p.weave.value();
                // Two rotation stages at GOLDEN-detuned angles: equal
                // angles compose a with-then-against eigenvalue at
                // exactly 1 — a lossless axis a sustained tone rides
                // to runaway (measured 115× before this). WEAVE only
                // rotates; the recirculation belongs to HOLD.
                let theta = 0.25 + weave;
                self.mesh_cos = theta.cos();
                self.mesh_sin = theta.sin();
                let theta2 = 0.618 * theta;
                self.mesh_cos2 = theta2.cos();
                self.mesh_sin2 = theta2.sin();
            }
        }
    }

    /// Renew every wandering target on shared countdowns.
    #[inline]
    fn tick_renewal(&mut self) {
        self.renew = self.renew.wrapping_add(1);
        let slow = (0.4 * self.sr) as u32;
        if self.renew.is_multiple_of(slow.max(1)) {
            for i in 0..2 {
                let w = self.rng.next_bipolar();
                self.tank[i].mod_ap.target = w;
            }
            for i in 0..4 {
                let w = self.rng.next_bipolar();
                self.mesh_target[i] = w;
            }
        }
        let dust_period = (0.14 * self.sr) as u32;
        if self.renew.is_multiple_of(dust_period.max(1)) {
            for i in 0..MOTE_COUNT {
                let w = self.rng.next_bipolar().abs();
                self.dust_taps[i].target = w;
            }
        }
    }

    /// The three-coupling port: feed the wet pair to the field and
    /// return the (heard, regeneration) pairs — DIFFUSE morphs what
    /// the ear gets, BLEED morphs what re-enters the loop, HOLD
    /// (block-rate) is the field's own ring. Both couplings at 0 is
    /// a true bypass — the pairs return untouched (bit-classic) and
    /// no field state ticks.
    #[inline]
    pub(super) fn couple(
        &mut self,
        m: f32,
        s: f32,
        diffuse: f32,
        bleed: f32,
    ) -> ((f32, f32), (f32, f32)) {
        if diffuse <= 0.0 && bleed <= 0.0 {
            return ((m, s), (m, s));
        }
        self.tick_renewal();
        // The voice's EFFECTIVE hold gain: prism's is gated by smear
        // (at smear 0 the cascade is an identity and an ungated
        // loop is a frame-rate one-pole resonator, +20 dB of bass —
        // measured), and the scalings below must see the same gate
        // or they compensate for sustain that no longer exists.
        let hg = if self.kind == 1 {
            self.hold_g * self.prism_depth.min(1.0)
        } else {
            self.hold_g
        };
        let (raw_m, raw_s) = match self.kind {
            0 => self.plate_pair(m, s),
            1 => {
                let (im, is_) = (m + hg * self.field_prev[0], s + hg * self.field_prev[1]);
                let out = self.prism_pair(im, is_);
                self.field_prev = [out.0, out.1];
                out
            }
            2 => {
                let (im, is_) = (m + hg * self.field_prev[0], s + hg * self.field_prev[1]);
                let out = self.dust_pair(im, is_);
                self.field_prev = [out.0, out.1];
                out
            }
            _ => self.mesh_pair(m, s),
        };
        // TWO field scalings, one per coupling. The ear gets a
        // gentle sustain compensation (HOLD dials ring time, not a
        // loudness cliff). The LOOP gets the full (1 − hg)
        // normalization: the field's sustained gain is 1/(1 − hg),
        // so this bounds it at ≤ 1 and the regen morph stays convex
        // — stable at ANY bleed and ANY feedback (without it the
        // coupled product exploded to 1e24 at full settings —
        // measured). Both are exactly 1.0 at hold 0, which keeps
        // the diagonal (bleed = diffuse, hold 0) equal to the pure
        // in-loop topology.
        //
        // DUST is the exception BOTH ways, because its cloud is
        // LOSSY (~2/3 of a decorrelated field gone per pass — that
        // is the crumble): its field never builds the sustain the
        // compensations assume, so the ear pays no compensation,
        // and the regen morph keeps most of the raw repeat — at
        // full bleed the unscaled replace-morph starved the loop
        // with a quarter-scale field and the WHOLE echo train died
        // within two repeats (measured −81 dB where classic reads
        // −46). The 0.25 blend depth keeps the trade musical:
        // full bleed costs ~3 dB per repeat of decay for the
        // crumble in the loop, instead of ending the train.
        // Coherent worst case stays bounded: the cloud's coherent
        // gain is one, and 0.75 + 0.25·(1) ≤ 1 — additive bleed
        // was considered and REJECTED (coherent sustained field ×
        // added repeat = provable runaway; the doctrine holds).
        let dust = self.kind == 2;
        let k_hear = if dust { 1.0 } else { 1.0 - 0.55 * hg };
        let k_loop = 1.0 - hg;
        let bleed_eff = if dust { bleed * 0.25 } else { bleed };
        let (hm, hs) = (raw_m * k_hear, raw_s * k_hear);
        let (lm, ls) = (raw_m * k_loop, raw_s * k_loop);
        (
            (m + diffuse * (hm - m), s + diffuse * (hs - s)),
            (m + bleed_eff * (lm - m), s + bleed_eff * (ls - s)),
        )
    }

    /// PLATE: input diffusion per lane, then the figure-8 tank. The
    /// branches carry (m + s)/(m − s)-flavored injections and the
    /// output recombines sum/difference taps, so the tail
    /// decorrelates the image as it thickens.
    #[inline]
    fn plate_pair(&mut self, m: f32, s: f32) -> (f32, f32) {
        let ease = 1.0 - (-2.0 * core::f32::consts::PI * 0.9 / self.sr).exp();
        let g = self.plate_g;
        // Input diffusion (static, short): the early character.
        let mut dm = m;
        let mut ds = s;
        for ap in self.plate_in[0].iter_mut() {
            dm = ap.tick(dm, g, 0.0, ease);
        }
        for ap in self.plate_in[1].iter_mut() {
            ds = ap.tick(ds, g, 0.0, ease);
        }
        // The figure-8: each branch is fed the injection plus the
        // OTHER branch's end, with the hold gain.
        let hg = self.hold_g;
        let cross = [
            self.tank[1].d2.read_cubic(self.tank[1].d2_len),
            self.tank[0].d2.read_cubic(self.tank[0].d2_len),
        ];
        let inj = [
            (dm + ds) * core::f32::consts::FRAC_1_SQRT_2,
            (dm - ds) * core::f32::consts::FRAC_1_SQRT_2,
        ];
        let depth = TANK_MOD_DEPTH * self.plate_mod;
        let kd = self.tank_damp_k;
        let mut sum = [0.0f32; 2];
        for (i, br) in self.tank.iter_mut().enumerate() {
            let x = br.mod_ap.tick(inj[i] + hg * cross[i], g, depth, ease);
            br.d1.write(x);
            br.d1.advance();
            let v1 = br.d1.read_cubic(br.d1_len);
            // In-tank damping: the half-lap's air.
            br.damp += (v1 - br.damp) * kd;
            let v2 = br.ap2.tick(br.damp, g, 0.0, ease);
            br.d2.write(v2);
            br.d2.advance();
            // The branch's voice is a SPREAD of taps, so one pass
            // reads as a bloom, never one discrete echo.
            let mut acc = 0.5 * v2;
            let mut w = 0.34;
            for off in TANK_TAPS {
                acc += w * br.d1.read_cubic(off.min(br.d1_len));
                w *= 0.72;
            }
            sum[i] = acc;
        }
        (
            (sum[0] + sum[1]) * core::f32::consts::FRAC_1_SQRT_2,
            (sum[0] - sum[1]) * core::f32::consts::FRAC_1_SQRT_2,
        )
    }

    #[inline]
    fn prism_pair(&mut self, m: f32, s: f32) -> (f32, f32) {
        // The tap slides CONTINUOUSLY along the cascade (blend of
        // the two bracketing stage outputs). Only the stages up to
        // the tap plus a GUARD BAND tick: the band stays warm ahead
        // of the 50 ms smoothed tap, and the untouched far stages
        // cost nothing (ticking all 96 at smear 0.1 measured the
        // same +343 ns/frame as smear 1 — 100% cost for 1% use).
        // The two LANES run interleaved through the cascade: two
        // independent dependency chains pipeline where one serial
        // chain stalls. Per-lane arithmetic is unchanged — outputs
        // are bit-identical to the per-lane loop.
        const GUARD: usize = 16;
        let depth = self.prism_depth.min((PRISM_STAGES - 1) as f32);
        let k0 = depth as usize;
        let frac = depth - k0 as f32;
        let last = (k0 + GUARD).min(PRISM_STAGES - 1);
        let (mid_lane, side_lane) = self.prism.pair_mut();
        let (mut xm, mut xs) = (m, s);
        let (mut before, mut after) = ((m, s), (m, s));
        for i in 0..=last {
            if i == k0 {
                before = (xm, xs);
            }
            xm = mid_lane[i].process(xm);
            xs = side_lane[i].process(xs);
            if i == k0 {
                after = (xm, xs);
            }
        }
        (
            before.0 + (after.0 - before.0) * frac,
            before.1 + (after.1 - before.1) * frac,
        )
    }

    #[inline]
    fn dust_pair(&mut self, m: f32, s: f32) -> (f32, f32) {
        // The cloud positions are shared by lanes — one cloud, both
        // ears: scatter never splits the image. The walk slows some
        // as HOLD rises (prudence, not fear: the cloud is
        // peak-passive — its response WANDERS, it cannot grow).
        let rate = 0.4 + 0.8 * (1.0 - self.hold_g / 0.75);
        let ease = 1.0 - (-2.0 * core::f32::consts::PI * rate / self.sr).exp();
        let reach = self.dust_spray * SPRAY_MAX_MS * 0.001 * self.sr;
        let (mid, side) = self.dust_lines.pair_mut();
        mid.write(m);
        mid.advance();
        side.write(s);
        side.advance();
        let (mut om, mut os) = (0.0f32, 0.0f32);
        for tap in self.dust_taps.iter_mut() {
            tap.walk += (tap.target - tap.walk) * ease;
            let pos = (tap.walk * reach).max(1.0);
            om += tap.gain * mid.read_cubic(pos);
            os += tap.gain * side.read_cubic(pos);
        }
        (om, os)
    }

    #[inline]
    fn mesh_pair(&mut self, m: f32, s: f32) -> (f32, f32) {
        let ease = 1.0 - (-2.0 * core::f32::consts::PI * 0.8 / self.sr).exp();
        let kd = self.mesh_damp_k;
        for i in 0..4 {
            // A few samples of slow line motion + per-line damping:
            // the late field breathes and darkens instead of ringing
            // (static undamped strands were the "fixed" tail).
            self.mesh_drift[i] += (self.mesh_target[i] - self.mesh_drift[i]) * ease;
            let read =
                self.mesh[i].read_cubic(self.mesh_len[i] + MESH_MOD_DEPTH * self.mesh_drift[i]);
            self.mesh_damp[i] += (read - self.mesh_damp[i]) * kd;
        }
        let v = self.mesh_damp;
        // The weave: image-space rotations across the line pairs —
        // orthogonal, so the mesh conserves what it circulates.
        let (c, sn) = (self.mesh_cos, self.mesh_sin);
        let (c2, sn2) = (self.mesh_cos2, self.mesh_sin2);
        let (a, b) = (c * v[0] - sn * v[1], sn * v[0] + c * v[1]);
        let (d, e) = (c * v[2] - sn * v[3], sn * v[2] + c * v[3]);
        let w = [
            c2 * a - sn2 * d,
            c2 * b - sn2 * e,
            sn2 * a + c2 * d,
            sn2 * b + c2 * e,
        ];
        let g = self.hold_g;
        self.mesh[0].write(0.5 * m + g * w[0]);
        self.mesh[1].write(0.5 * s + g * w[1]);
        self.mesh[2].write(0.5 * m + g * w[2]);
        self.mesh[3].write(0.5 * s + g * w[3]);
        for line in self.mesh.iter_mut() {
            line.advance();
        }
        let out = core::f32::consts::FRAC_1_SQRT_2;
        ((w[0] + w[2]) * out, (w[1] + w[3]) * out)
    }
}
