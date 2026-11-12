//! The engram ENGINE — a reverb with an associative memory. The engine
//! is its COMPONENTS; `process` below is the whole dance:
//! * `listen.rs` — `Listen`: the ear — harmony, timbre, rhythm, onsets.
//! * `bank.rs` — `Bank`: the memory — moments of your playing, keyed by
//!   those fingerprints, recalled by attention, played back by grain.
//! * `room.rs` — `Room`: sixteen delay-line neurons, wired by where
//!   their nodes sit (always energy-preserving).
//! * `geometry.rs` — `Geometry`: the drawn + Hebbian-learned node layout.
//! * `space.rs` — the GENERATIVE ROOM: a VAE learned ≈700 real reverbs;
//!   a point in its latent decodes to a room's anatomy, which drives
//!   `early.rs` (generated reflections), the room's per-band decay and
//!   `voicing.rs` (colour, width). The point comes from the MAP, is
//!   pulled by the memory (recalled moments bring back the room they
//!   were heard in — STEER) and wanders (DRIFT).
//! * `telemetry.rs` — `Telemetry`: the display's feed (packed).
//!
//! Signal: what you play goes into the room (an ordinary, drawable
//! reverb). Meanwhile the ear fingerprints it, the bank remembers it,
//! and — every block — attention retrieves the past moments most like
//! NOW (older than DÉJÀ). Those memories play INTO the room, so the
//! tail is your playing's reverb plus the reverb of what this reminds
//! the room of. When you stop, the last query holds and recall fades
//! at the room's T60 (HOLD keeps it, and stops new memories).

extern crate alloc;
use alloc::sync::Arc;

use stave_core::params::ContinuousControl;
use stave_core::{DebugBus, Environment, ProcessCtx, Tail};

use crate::params::EngramParams;

mod bank;
mod decay_eq;
mod describe;
mod early;
mod geometry;
mod listen;
mod room;
mod space;
pub(crate) mod space_model;
mod telemetry;
mod voicing;
#[cfg(test)]
mod bench;
#[cfg(test)]
mod features;

use self::bank::{Bank, Key, GRAIN_S};
use self::geometry::Geometry;
use self::listen::Listen;
use self::room::{Room, Shape, LINES};
use self::early::Early;
use self::space::{at_map, decode, to_map, Anatomy, Latent};
use self::space_model::LATENT;
use self::telemetry::Telemetry;
use self::voicing::Voicing;

/// The ear's rate (it hears harmony, not air).
const EAR_HZ: f32 = 12_000.0;
/// Recalled memories into the room, and a little straight out (so the
/// remembered moment is recognisable, not only reverberated).
const INTO_ROOM: f32 = 1.0;
const DIRECT: f32 = 0.35;
/// How fast the room moves through the learned space (s): a morph you
/// hear happen, never a jump.
const SPACE_GLIDE_S: f32 = 1.2;
/// DECAY/TONE's glide in the generated room (s).
const KNOB_GLIDE_S: f32 = 0.08;
/// DRIFT: the wander's time scale (s) and its reach at full (latent sd).
const DRIFT_S: f32 = 6.0;
const DRIFT_REACH: f32 = 1.0;
/// The late network hears the dry signal this long after the direct (the
/// direct's 2 ms, as cut from the IRs): its decay starts at the onset.
const LATE_GAP_S: f32 = 0.002;
/// DECAY's and TONE's defaults: there, the generated room is its own.
const DECAY_REF: f32 = 3.0;
const TONE_REF: f32 = 0.6;
/// The late network's impulse energy (one channel, fed as at SPACE 1),
/// by SIZE × T60 — MEASURED (`calibration::late_energy_table`), not
/// modelled: it grows ~√T60 and falls with SIZE. Sets early : late.
/// (The default drawing; a redrawn room is near enough.)
#[cfg_attr(not(test), allow(dead_code))]
const LATE_SIZES: [f32; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];
const LATE_T60S: [f32; 6] = [0.1, 0.3, 1.0, 3.0, 10.0, 30.0];
const LATE_ENERGY: [[f32; 6]; 5] = [
    [0.27680, 0.46917, 1.14946, 2.98954, 9.18845, 26.59913],
    [0.24114, 0.29383, 0.58110, 1.42108, 4.31761, 12.47101],
    [0.23505, 0.25887, 0.42738, 0.97064, 2.86814, 8.23508],
    [0.23193, 0.24875, 0.36205, 0.76368, 2.17587, 6.16726],
    [0.22444, 0.24551, 0.32362, 0.63352, 1.75142, 4.94695],
];
/// The late network's LOSSLESS early profile by SIZE: energy per 5 ms
/// bin, 0–115 ms after the direct, for a unit feed impulse after
/// LATE_GAP_S (`calibration::late_early_table`). It overlaps the
/// reflections' window — `Early::aim` fills only what it leaves.
const LATE_EARLY: [[f32; 23]; 5] = [
    [0.00000, 0.00000, 0.00010, 0.00356, 0.00615, 0.00922, 0.01235, 0.02123, 0.02940, 0.03048, 0.04377, 0.04790, 0.05324, 0.06601, 0.05589, 0.05323, 0.07478, 0.06967, 0.06438, 0.06046, 0.07600, 0.06481, 0.05951],
    [0.00000, 0.00000, 0.00000, 0.00000, 0.00000, 0.00000, 0.00074, 0.00353, 0.00432, 0.00765, 0.01129, 0.01559, 0.01651, 0.01802, 0.02495, 0.02247, 0.02409, 0.02803, 0.02523, 0.02699, 0.02574, 0.02673, 0.02598],
    [0.00000, 0.00000, 0.00000, 0.00000, 0.00000, 0.00000, 0.00000, 0.00000, 0.00000, 0.00047, 0.00079, 0.00352, 0.00518, 0.00811, 0.01122, 0.01359, 0.01447, 0.01556, 0.01751, 0.01874, 0.01715, 0.01685, 0.01785],
    [0.00000, 0.00000, 0.00000, 0.00000, 0.00000, 0.00000, 0.00000, 0.00000, 0.00000, 0.00000, 0.00000, 0.00000, 0.00026, 0.00085, 0.00148, 0.00362, 0.00585, 0.00754, 0.01050, 0.01171, 0.01489, 0.01513, 0.01622],
    [0.00000, 0.00000, 0.00000, 0.00000, 0.00000, 0.00000, 0.00000, 0.00000, 0.00000, 0.00000, 0.00000, 0.00000, 0.00000, 0.00000, 0.00000, 0.00000, 0.00087, 0.00106, 0.00230, 0.00383, 0.00571, 0.00665, 0.00911],
];

/// The late network's share of its own energy in each early bin, for a
/// unit feed at SIZE `size` decaying at `t60`.
fn late_early_share(size: f32, t60: f32) -> [f32; 23] {
    let sz = size.clamp(0.0, 1.0) * 4.0;
    let j = (sz as usize).min(3);
    let fs = sz - j as f32;
    let total = late_energy(t60, size).max(1e-9);
    core::array::from_fn(|k| {
        let lossless = LATE_EARLY[j][k] * (1.0 - fs) + LATE_EARLY[j + 1][k] * fs;
        let since = ((k as f32 + 0.5) * 0.005 - LATE_GAP_S).max(0.0);
        lossless * 10f32.powf(-6.0 * since / t60.max(0.05)) / total
    })
}

/// LATE_ENERGY, interpolated (log T60, log energy; linear SIZE).
fn late_energy(t60: f32, size: f32) -> f32 {
    let lt = t60.clamp(LATE_T60S[0], LATE_T60S[5]).ln();
    let i = (0..5).find(|&i| lt <= LATE_T60S[i + 1].ln()).unwrap_or(4);
    let ft = (lt - LATE_T60S[i].ln()) / (LATE_T60S[i + 1].ln() - LATE_T60S[i].ln());
    let sz = size.clamp(0.0, 1.0) * 4.0;
    let j = (sz as usize).min(3);
    let fs = sz - j as f32;
    let at = |r: usize| LATE_ENERGY[r][i].ln() * (1.0 - ft) + LATE_ENERGY[r][i + 1].ln() * ft;
    (at(j) * (1.0 - fs) + at(j + 1) * fs).exp()
}

/// The window loudness is matched over (s).
const LOUDNESS_S: f32 = 0.5;

/// The room's whole energy such that its energy in the first LOUDNESS_S
/// equals the knobs' room's (`knob_energy`, an exponential tail of T60
/// `t60`): the reflections (`frac` of the whole) all land inside it,
/// the late tail only its first part.
fn loudness_matched(a: &Anatomy, knob_energy: f32, t60: f32) -> f32 {
    let frac = a.early.iter().map(|b| b.iter().sum::<f32>()).sum::<f32>() / a.early.len() as f32;
    let inside = 1.0 - 10f32.powf(-6.0 * LOUDNESS_S / t60.max(0.05));
    knob_energy * inside / (frac + (1.0 - frac) * inside).max(1e-6)
}

fn geomean(t: &[f32]) -> f32 {
    (t.iter().map(|v| v.ln()).sum::<f32>() / t.len() as f32).exp()
}

/// The longest the generated room may ring: DECAY's own top (a stretched
/// hall can't outlast what the knob allows the knobs' room).
const MAX_T60: f32 = 30.0;

/// The generated room's late T60 per octave (125 Hz … 8 kHz): DECAY
/// stretching all of them, TONE tilting more toward the top, around
/// their defaults; capped at DECAY's own top.
fn late_t60_oct(a: &Anatomy, decay_s: f32, tone: f32) -> [f32; 7] {
    let stretch = decay_s / DECAY_REF;
    core::array::from_fn(|k| {
        let tilt = 2f32.powf((tone - TONE_REF) * 2.5 * k as f32 / 6.0);
        (a.t60[k] * stretch * tilt).clamp(0.1, MAX_T60)
    })
}

/// The same in the reflections' three bands (low, mid, high).
fn late_t60(a: &Anatomy, decay_s: f32, tone: f32) -> [f32; 3] {
    let t = late_t60_oct(a, decay_s, tone);
    [geomean(&t[0..3]), geomean(&t[2..5]), geomean(&t[5..7])]
}

#[inline]
fn db_to_amp(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

struct Controls {
    size: ContinuousControl,
    decay: ContinuousControl,
    tone: ContinuousControl,
    recall: ContinuousControl,
    mix: ContinuousControl,
    output: ContinuousControl,
}

impl Controls {
    fn new(p: &EngramParams) -> Self {
        Self {
            size: ContinuousControl::new(&p.size),
            decay: ContinuousControl::new(&p.decay),
            tone: ContinuousControl::new(&p.tone),
            recall: ContinuousControl::new(&p.recall),
            mix: ContinuousControl::new(&p.mix),
            output: ContinuousControl::new(&p.output),
        }
    }

    fn prepare(&mut self, sr: f64, p: &EngramParams) {
        self.size.prepare(sr, &p.size);
        self.decay.prepare(sr, &p.decay);
        self.tone.prepare(sr, &p.tone);
        self.recall.prepare(sr, &p.recall);
        self.mix.prepare(sr, &p.mix);
        self.output.prepare(sr, &p.output);
    }
}

/// The sixteen drawn node positions, from the params.
fn drawn(p: &EngramParams) -> [[f32; 2]; LINES] {
    let xy = [
        (&p.node_0_x, &p.node_0_y), (&p.node_1_x, &p.node_1_y), (&p.node_2_x, &p.node_2_y), (&p.node_3_x, &p.node_3_y),
        (&p.node_4_x, &p.node_4_y), (&p.node_5_x, &p.node_5_y), (&p.node_6_x, &p.node_6_y), (&p.node_7_x, &p.node_7_y),
        (&p.node_8_x, &p.node_8_y), (&p.node_9_x, &p.node_9_y), (&p.node_10_x, &p.node_10_y), (&p.node_11_x, &p.node_11_y),
        (&p.node_12_x, &p.node_12_y), (&p.node_13_x, &p.node_13_y), (&p.node_14_x, &p.node_14_y), (&p.node_15_x, &p.node_15_y),
    ];
    core::array::from_fn(|i| [xy[i].0.value(), xy[i].1.value()])
}

pub struct EngramEngine {
    params: Arc<EngramParams>,
    controls: Controls,
    sr: f32,
    room: Room,
    geometry: Geometry,
    ear: Listen,
    bank: Bank,
    decim: usize,
    phase: usize,
    acc: f32,
    /// Recall's fade since you stopped (1 while you play).
    fade: f32,
    /// The recalled level (for the display).
    recall_env: f32,
    recall_env_k: f32,
    /// The FORGET button's last state (any change = a press).
    forget_was: bool,
    /// The ear's latest hearing (refreshed at its rate), and the onset
    /// flash for the display.
    hearing: self::listen::Hearing,
    pulse: f32,
    /// The generative room: where it is in the learned space (gliding),
    /// the map's point, the dream's offset, the decoded anatomy.
    early: Early,
    voicing: Voicing,
    late_in: [stave_dsp::DelayLine; 2],
    late_gap: f32,
    late_gap_to: f32,
    late_gap_k: f32,
    z: Latent,
    map_at: [f32; 2],
    map_z: Latent,
    dream: Latent,
    dream_rng: stave_dsp::Xorshift32,
    anatomy: Anatomy,
    /// DECAY and TONE as the generated room hears them: glided per
    /// block (the knobs' room has its own per-frame ramps).
    space_decay: f32,
    space_tone: f32,
    /// For the face: where the room is on the map, the nearest real
    /// space, where memory pulls it and how hard, the mid T60.
    map_live: [f32; 2],
    map_near: usize,
    map_pull: [f32; 2],
    pull: f32,
    t60_mid: f32,
    tail_s: f32,
    settle_space: bool,
    telemetry: Telemetry,
}

impl EngramEngine {
    /// Once a block: move the room through the learned space and aim the
    /// reflections, the late decay and the voicing at where it is.
    fn aim_space(&mut self, p: &EngramParams, frames: f32) {
        let dt = frames / self.sr;
        let at = [p.space_x.value(), p.space_y.value()];
        if at != self.map_at {
            self.map_at = at;
            self.map_z = at_map(at[0], at[1]);
        }
        // Where the map says, pulled toward the rooms memory recalls.
        let mut target = self.map_z;
        self.pull = 0.0;
        if let Some((zr, mass)) = self.bank.remembered_space() {
            let k = p.steer.value() * mass;
            for (t, r) in target.iter_mut().zip(zr) {
                *t += (r - *t) * k;
            }
            if k > 0.01 {
                self.map_pull = to_map(&zr).0;
                self.pull = k;
            }
        }
        // The dream: an Ornstein–Uhlenbeck wander around the target.
        let reach = p.drift.value() * DRIFT_REACH;
        let theta = dt / DRIFT_S;
        let sigma = reach * (2.0 * theta).sqrt();
        for d in &mut self.dream {
            let n = (0..4).map(|_| self.dream_rng.next_bipolar()).sum::<f32>() * 0.866; // ≈ N(0, 1)
            *d += -*d * theta + sigma * n;
        }
        let k = if self.settle_space { 1.0 } else { 1.0 - (-dt / SPACE_GLIDE_S).exp() };
        for ((z, t), d) in self.z.iter_mut().zip(target).zip(self.dream) {
            *z += (t + d - *z) * k;
        }
        let (live, near) = to_map(&self.z);
        self.map_live = live;
        self.map_near = near;
        self.bank.set_here(&self.z, live);
        self.anatomy = decode(&self.z);

        let gen = p.space.value();
        let k = if self.settle_space { 1.0 } else { 1.0 - (-dt / KNOB_GLIDE_S).exp() };
        self.space_decay += (p.decay.value() - self.space_decay) * k;
        self.space_tone += (p.tone.value() - self.space_tone) * k;
        let decay = self.space_decay;
        let oct = late_t60_oct(&self.anatomy, decay, self.space_tone);
        let t = late_t60(&self.anatomy, decay, self.space_tone);
        let shape = Shape { size: p.size.value(), decay_s: p.decay.value(), tone: p.tone.value(), hold: p.hold.value(), gen };
        self.room.aim_late(&oct, shape);
        // As loud as the knobs' room at the generated room's (mid) decay
        // — over the first LOUDNESS_S, the energy you hear while playing
        // (a 30 s tail spreads its energy over 30 s; reflections land in
        // 0.12) — split early : late as the learned room has it.
        let size = p.size.value();
        let whole = loudness_matched(&self.anatomy, late_energy(t[1], size), t[1]);
        let mut late = t.map(|t60| late_energy(t60, size));
        // The network's low band carries a little more than its white
        // share — not its decay, which is exact in every band (measured,
        // `band_balance::late_band_balance`), but its level, more the
        // longer it rings (×1.0 at 0.5 s … ×1.4 at 6 s; mid and high
        // flat). Counted, so early : late holds in the lows too.
        late[0] *= (1.05 + 0.095 * (t[0] / 0.5).max(1.0).log2()).max(1.0);
        let share = t.map(|t60| late_early_share(size, t60));
        self.early.aim(&self.anatomy, whole, late, &share, gen, self.settle_space, frames as usize);
        self.voicing.aim(&self.anatomy, gen, self.settle_space, frames as usize);
        self.late_gap_to = gen * LATE_GAP_S * self.sr;
        if self.settle_space {
            self.late_gap = self.late_gap_to;
        }
        self.t60_mid = gen * t[1] + (1.0 - gen) * decay;
        let longest = oct.iter().copied().fold(0.0f32, f32::max);
        self.tail_s = 1.5 * (decay + gen * (longest - decay));
        self.settle_space = false;
    }
}

impl stave_core::Processor for EngramEngine {
    type Params = EngramParams;

    fn new(params: Arc<EngramParams>) -> Self {
        Self {
            controls: Controls::new(&params),
            forget_was: params.forget.value(),
            params,
            sr: 48_000.0,
            room: Room::new(),
            geometry: Geometry::new(),
            ear: Listen::new(),
            bank: Bank::new(),
            decim: 4,
            phase: 0,
            acc: 0.0,
            fade: 1.0,
            recall_env: 0.0,
            recall_env_k: 0.0,
            hearing: Default::default(),
            pulse: 0.0,
            early: Early::new(),
            voicing: Voicing::new(),
            late_in: core::array::from_fn(|_| stave_dsp::DelayLine::new()),
            late_gap: 0.0,
            late_gap_to: 0.0,
            late_gap_k: 0.0,
            z: [0.0; LATENT],
            map_at: [-1.0; 2],
            map_z: [0.0; LATENT],
            dream: [0.0; LATENT],
            dream_rng: stave_dsp::Xorshift32::new(0x5EED_1234),
            anatomy: decode(&[0.0; LATENT]),
            space_decay: DECAY_REF,
            space_tone: TONE_REF,
            map_live: [0.5; 2],
            map_near: 0,
            map_pull: [0.5; 2],
            pull: 0.0,
            t60_mid: DECAY_REF,
            tail_s: 3.0,
            settle_space: true,
            telemetry: Telemetry::new(),
        }
    }

    fn activate(&mut self, env: Environment) {
        let sr = env.sample_rate;
        self.sr = sr;
        self.controls.prepare(f64::from(sr), &self.params);
        self.room.prepare(sr);
        self.geometry.reset();
        self.geometry.step(&drawn(&self.params), 0.0, true);
        self.room.reshape(&self.geometry.radius(), &self.geometry.distances());
        self.room.settle();
        self.decim = ((sr / EAR_HZ).round() as usize).max(1);
        self.ear.prepare(sr, sr / self.decim as f32, GRAIN_S);
        self.bank.prepare(sr);
        self.phase = 0;
        self.acc = 0.0;
        self.fade = 1.0;
        self.recall_env = 0.0;
        self.recall_env_k = 1.0 - (-1.0 / (0.08 * sr)).exp();
        self.forget_was = self.params.forget.value();
        self.early.prepare(sr);
        self.voicing.prepare(sr);
        for l in &mut self.late_in {
            l.prepare((LATE_GAP_S * sr) as usize + 8);
        }
        self.late_gap_k = 1.0 - (-1.0 / (0.05 * sr)).exp();
        self.map_at = [-1.0; 2];
        self.dream = [0.0; LATENT];
        self.settle_space = true;
    }

    fn attach_telemetry(&mut self, bus: Arc<DebugBus>) {
        self.telemetry.attach(bus);
    }

    fn process(&mut self, ctx: &mut ProcessCtx<'_>) {
        let p = self.params.clone();
        let hold = p.hold.value();
        // FORGET: any press wipes the bank.
        let forget = p.forget.value();
        if forget != self.forget_was {
            self.forget_was = forget;
            self.bank.forget();
        }
        // The shape: where the nodes sit now, and the room they make.
        self.geometry.step(&drawn(&p), p.plasticity.value(), hold);
        self.room.reshape(&self.geometry.radius(), &self.geometry.distances());
        // Attention, once a block: the past moments most like now.
        let min_age = (p.deja.value() * self.sr) as u64;
        let h = self.ear.hearing();
        let now = Key { chroma: h.chroma, timbre: h.timbre, rhythm: self.ear.rhythm(), tonal: h.tonal, percussive: false };
        self.bank.attend(now, self.ear.present(), p.focus.value(), p.matching.value(), min_age);
        let texture = p.texture.value();
        let frames = ctx.frames as f32;
        self.aim_space(&p, frames);

        let c = &mut self.controls;
        let mut size_ramp = c.size.begin_block(ctx.params, &p.size);
        let mut decay_ramp = c.decay.begin_block(ctx.params, &p.decay);
        let mut tone_ramp = c.tone.begin_block(ctx.params, &p.tone);
        let mut recall_ramp = c.recall.begin_block(ctx.params, &p.recall);
        let mut mix_ramp = c.mix.begin_block(ctx.params, &p.mix);
        let mut out_ramp = c.output.begin_block(ctx.params, &p.output);
        let (decim, inv, sr) = (self.decim, 1.0 / self.decim as f32, self.sr);
        let (room, geometry, ear, bank) = (&mut self.room, &mut self.geometry, &mut self.ear, &mut self.bank);
        let (phase, acc, fade) = (&mut self.phase, &mut self.acc, &mut self.fade);
        let (recall_env, recall_env_k) = (&mut self.recall_env, self.recall_env_k);
        let (hearing, pulse) = (&mut self.hearing, &mut self.pulse);
        let (early, voicing, late_in) = (&mut self.early, &mut self.voicing, &mut self.late_in);
        let (late_gap, late_gap_to, late_gap_k) = (&mut self.late_gap, self.late_gap_to, self.late_gap_k);
        let gen = p.space.value();
        let fade_t60 = self.tail_s / 1.5;

        let lanes = ctx.lanes::<2>();
        let active = lanes.active();
        lanes.run(|fr| {
            let shape = Shape { size: size_ramp.next(), decay_s: decay_ramp.next(), tone: tone_ramp.next(), hold, gen };
            let recall = recall_ramp.next();
            let dry = if active >= 2 { [fr.input(0), fr.input(1)] } else { [fr.input(0); 2] };
            let mono = 0.5 * (dry[0] + dry[1]);

            // The ear (timbre at the host rate, the rest at its own) and
            // the plasticity's eyes.
            ear.air(mono);
            *acc += mono;
            *phase += 1;
            let mut onset = false;
            if *phase == decim {
                ear.hear(*acc * inv);
                onset = ear.onset();
                *hearing = ear.hearing();
                *acc = 0.0;
                *phase = 0;
                geometry.observe(room.outputs());
                if onset {
                    *pulse = 1.0;
                }
            }
            let playing = ear.present();
            let _ = bank.capture(mono, hearing, playing, onset, !hold);

            // What it remembers, fading at the room's T60 once you stop.
            if playing {
                *fade = 1.0;
            } else if !hold {
                *fade *= 10f32.powf(-3.0 / (fade_t60 * sr));
            }
            let memory = bank.recall(onset, texture) * recall * *fade;
            *recall_env += (memory.abs() - *recall_env) * recall_env_k;

            let into = memory * INTO_ROOM;
            let x = [dry[0] + into, dry[1] + into];
            // The reflections, and the late network's input: by SPACE,
            // the dry signal per band at each band's late level, diffused
            // (decorrelated L/R), 2 ms after the direct — its decay starts
            // at the onset, as a real room's does.
            let (e, feed) = early.tick(0.5 * (x[0] + x[1]));
            *late_gap += (late_gap_to - *late_gap) * late_gap_k;
            let mut late = [0.0f32; 2];
            for (c, l) in late_in.iter_mut().enumerate() {
                l.write(x[c] + gen * (feed[c] - x[c]));
                late[c] = l.read(*late_gap);
                l.advance();
            }
            let r = voicing.widen(room.tick(late, shape));
            let r = voicing.colour_tick([r[0] + e[0], r[1] + e[1]]);
            let wet = [r[0] + memory * DIRECT, r[1] + memory * DIRECT];

            let a = mix_ramp.next() * core::f32::consts::FRAC_PI_2;
            // MIX 0 is bit-exact: cos(0) = 1 exactly, sin(0) = 0 exactly.
            let (dg, wg) = (a.cos(), a.sin());
            let g = db_to_amp(out_ramp.next());
            if active >= 2 {
                fr.write(0, (dry[0] * dg + wet[0] * wg) * g);
                fr.write(1, (dry[1] * dg + wet[1] * wg) * g);
            } else if active == 1 {
                fr.write(0, (dry[0] * dg + 0.5 * (wet[0] + wet[1]) * wg) * g);
            }
        });

        let tonal = self.hearing.tonal;
        self.telemetry.publish(&self.room, &self.geometry, &self.bank, self.recall_env, self.pulse, tonal);
        self.telemetry.publish_space(&self.bank, self.map_live, self.map_near, self.map_pull, self.pull, self.t60_mid);
        self.pulse *= 0.6;
    }

    fn latency_samples(&self) -> u32 {
        0
    }

    fn tail(&self) -> Tail {
        let p = &self.params;
        if p.hold.value() {
            Tail::Infinite
        } else {
            Tail::Samples((self.tail_s * self.sr) as u32)
        }
    }
}


#[cfg(test)]
mod diag {
    use super::*;

    /// Run the ear + bank over `sig` and dump the bank at `until` s.
    fn trace(sig: impl Fn(f32) -> f32, until: f32, matching: f32) {
        let sr = 48_000.0f32;
        let mut ear = Listen::new();
        ear.prepare(sr, 12_000.0, GRAIN_S);
        let mut bank = Bank::new();
        bank.prepare(sr);
        let mut acc = 0.0;
        let mut h = ear.hearing();
        for i in 0..(until * sr) as usize {
            let x = sig(i as f32 / sr);
            ear.air(x);
            acc += x;
            let mut onset = false;
            if i % 4 == 3 {
                ear.hear(acc / 4.0);
                acc = 0.0;
                onset = ear.onset();
                h = ear.hearing();
            }
            if i % 256 == 0 {
                let now = Key { chroma: h.chroma, timbre: h.timbre, rhythm: ear.rhythm(), tonal: h.tonal, percussive: false };
                bank.attend(now, ear.present(), 0.9, matching, (2.0 * sr) as u64);
            }
            let _ = bank.capture(x, &h, ear.present(), onset, true);
            let _ = bank.recall(onset, 0.1);
        }
        std::println!("query tonal {:.2}", bank.query().tonal);
        bank.debug_dump();
    }

    #[test]
    #[ignore]
    fn trace_match() {
        let tau = core::f32::consts::TAU;
        let chord = |f: [f32; 3], t: f32, bright: bool| -> f32 {
            f.iter().map(|f| if bright { (1..=14).map(|h| 0.12 * (h as f32 * tau * f * t).sin() / h as f32).sum::<f32>() } else { 0.12 * (tau * f * t).sin() }).sum()
        };
        let a = [220.0, 261.63, 329.63];
        let b = [185.0, 233.08, 277.18];
        let sig = move |t: f32| if t < 3.0 { chord(a, t, true) } else if t < 3.5 { 0.0 } else if t < 6.5 { chord(b, t, false) } else if t < 7.0 { 0.0 } else if t < 9.5 { chord(a, t, false) } else { 0.0 };
        trace(sig, 8.0, 0.0);
    }

    #[test]
    #[ignore]
    fn trace_kicks() {
        let kick = |t: f32| 0.7 * (-t / 0.12).exp() * (core::f32::consts::TAU * (60.0 * t + 40.0 * (1.0 - (-t / 0.03).exp()) * 0.03)).sin();
        let hat = |t: f32| { let i = (t * 48_000.0) as u32; let n = |k: u32| { let mut h = k.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B; h ^= h >> 15; h = h.wrapping_mul(0x2C1B_3C6D); h ^= h >> 12; h as f32 / u32::MAX as f32 - 0.5 }; 0.9 * (-t / 0.025).exp() * (n(i) - n(i.wrapping_sub(1))) };
        let sig = move |t: f32| if t < 3.0 { kick(t % 0.25) } else if t < 3.5 { 0.0 } else if t < 6.5 { hat(t % 0.125) } else if t < 7.0 { 0.0 } else { kick(t % 0.25) };
        trace(sig, 8.0, 0.3);
    }
}

#[cfg(test)]
mod calibration {
    use super::*;

    /// The late network's impulse energy (one channel), with the
    /// generated decay at T60 `t` everywhere, SIZE `size`, the default
    /// drawing — fed as the engine feeds it at SPACE 1 (both channels).
    fn late_energy(t: f32, size: f32) -> f32 {
        let sr = 48_000.0;
        let mut room = Room::new();
        room.prepare(sr);
        let mut geometry = Geometry::new();
        let nodes: [[f32; 2]; LINES] = core::array::from_fn(|i| {
            let (x, y) = crate::params::default_node(i);
            [x as f32, y as f32]
        });
        geometry.reset();
        geometry.step(&nodes, 0.0, true);
        room.reshape(&geometry.radius(), &geometry.distances());
        room.settle();
        let shape = Shape { size, decay_s: t, tone: TONE_REF, hold: false, gen: 1.0 };
        let mut dl = self::super::early::FeedDiffuser::new();
        let mut dr = self::super::early::FeedDiffuser::new();
        dl.prepare(sr, 0);
        dr.prepare(sr, 1);
        let mut e = 0.0f64;
        let n = (t * 2.0 * sr) as usize;
        for i in 0..n {
            if i % 256 == 0 {
                room.aim_late(&[t; 7], shape);
            }
            let d = if i == 0 { 1.0 } else { 0.0 };
            let y = room.tick([dl.tick(d), dr.tick(d)], shape);
            e += f64::from(y[0]) * f64::from(y[0]);
        }
        e as f32
    }

    /// The late network's LOSSLESS early profile (HOLD: no decay): energy
    /// per 5 ms bin, 0–115 ms after the direct, for a unit feed impulse
    /// arriving after LATE_GAP_S — by SIZE. Prints LATE_EARLY.
    #[test]
    #[ignore]
    fn late_early_table() {
        let sr = 48_000.0;
        let bin = (0.005 * sr) as usize;
        let gap = (LATE_GAP_S * sr) as usize;
        for &size in &LATE_SIZES {
            let mut room = Room::new();
            room.prepare(sr);
            let mut geometry = Geometry::new();
            let nodes: [[f32; 2]; LINES] = core::array::from_fn(|i| {
                let (x, y) = crate::params::default_node(i);
                [x as f32, y as f32]
            });
            geometry.reset();
            geometry.step(&nodes, 0.0, true);
            room.reshape(&geometry.radius(), &geometry.distances());
            room.settle();
            let shape = Shape { size, decay_s: 3.0, tone: TONE_REF, hold: true, gen: 1.0 };
            let mut dl = self::super::early::FeedDiffuser::new();
            let mut dr = self::super::early::FeedDiffuser::new();
            dl.prepare(sr, 0);
            dr.prepare(sr, 1);
            let mut e = [0.0f64; 23];
            for i in 0..23 * bin {
                if i % 256 == 0 {
                    room.aim_late(&[3.0; 7], shape);
                }
                // The feed as the engine makes it: diffused, after the gap.
                let d = if i == gap { 1.0 } else { 0.0 };
                let y = room.tick([dl.tick(d), dr.tick(d)], shape);
                e[i / bin] += f64::from(y[0]) * f64::from(y[0]);
            }
            let row: alloc::vec::Vec<alloc::string::String> = e.iter().map(|v| alloc::format!("{v:.5}")).collect();
            std::println!("    [{}],", row.join(", "));
        }
    }

    /// Prints LATE_ENERGY for pasting (after changing the room).
    #[test]
    #[ignore]
    fn late_energy_table() {
        for &size in &LATE_SIZES {
            let row: alloc::vec::Vec<alloc::string::String> = LATE_T60S.iter().map(|&t| alloc::format!("{:.5}", late_energy(t, size))).collect();
            std::println!("    [{}],", row.join(", "));
        }
    }

    /// The table (and its interpolation) is what the late network does,
    /// off the grid too.
    #[test]
    fn late_energy_is_calibrated() {
        for (t, size) in [(0.2f32, 0.1f32), (0.7, 0.6), (2.0, 0.5), (5.0, 0.9), (20.0, 0.3)] {
            let (got, want) = (late_energy(t, size), super::late_energy(t, size));
            assert!((got / want).ln().abs() < 0.15, "T60 {t} size {size}: measured {got:.4} vs table {want:.4}");
        }
    }
}


#[cfg(test)]
mod loudness {
    use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};

    /// Wet rms over 3 s of music-like noise (white through a 500 Hz
    /// one-pole: most energy low, as in a mix — what the colour's
    /// loudness-neutral weights assume).
    fn wet_rms(set: &[(&str, f64)]) -> f32 {
        let mut plugin = crate::Engram::new();
        plugin.debug_attach(DebugBus::new());
        let table = plugin.param_table();
        for (n, v) in set {
            table.by_name(n).unwrap().set_plain(*v);
        }
        plugin.activate(48_000.0, 256);
        let (mut s, mut lp) = (0x1234_5678u32, 0.0f32);
        let k = 1.0 - (-core::f32::consts::TAU * 500.0 / 48_000.0).exp();
        let (mut e, mut n) = (0.0f64, 0usize);
        let (mut il, mut ir, mut ol, mut or) = (vec![0.0f32; 256], vec![0.0f32; 256], vec![0.0f32; 256], vec![0.0f32; 256]);
        for _ in 0..(3 * 48_000 / 256) {
            for v in il.iter_mut() {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                lp += ((s as f32 / u32::MAX as f32 * 2.0 - 1.0) - lp) * k;
                *v = lp;
            }
            ir.copy_from_slice(&il);
            let mut ip = [core::ptr::null_mut(); 2];
            let mut op = [core::ptr::null_mut(); 2];
            let mut ctx = ProcessCtx {
                sample_rate: 48_000.0,
                frames: 256,
                transport: None,
                input: AudioBuffer::from_slices(&mut ip, [&mut il, &mut ir], 256),
                output: AudioBuffer::from_slices(&mut op, [&mut ol, &mut or], 256),
                aux: &[],
                params: stave_core::ParamBlock::empty(),
                events: &[],
            };
            plugin.process(&mut ctx);
            for v in &ol {
                e += f64::from(v * v);
                n += 1;
            }
        }
        (e / n as f64).sqrt() as f32
    }

    /// Anywhere on the map, the generated room is about as loud as the
    /// knobs' room at the same (mid) decay — the map changes the room,
    /// not the level.
    #[test]
    fn the_generated_room_is_as_loud_as_the_knobs_room() {
        let base = [("mix", 1.0), ("recall", 0.0), ("size", 0.5), ("plasticity", 0.0), ("drift", 0.0), ("steer", 0.0)];
        let mut worst = 0.0f32;
        for (x, y) in [(0.1f64, 0.1f64), (0.3, 0.7), (0.5, 0.5), (0.7, 0.3), (0.9, 0.9), (0.2, 0.9), (0.9, 0.2)] {
            let a = super::decode(&super::at_map(x as f32, y as f32));
            let t = super::late_t60(&a, 3.0, super::TONE_REF);
            let mut g = base.to_vec();
            g.extend([("space", 1.0), ("tone", 0.6), ("space x", x), ("space y", y), ("decay", 3.0)]);
            let mut k = base.to_vec();
            k.extend([("space", 0.0), ("tone", 1.0), ("decay", f64::from(t[1]))]);
            let db = 20.0 * (wet_rms(&g) / wet_rms(&k)).log10();
            std::println!("({x}, {y}) mid T60 {:.2} s: {db:+.1} dB", t[1]);
            worst = worst.max(db.abs());
        }
        assert!(worst < 3.0, "a map point is {worst:.1} dB off the knobs' room");
    }
}



#[cfg(test)]
mod band_balance {
    use super::*;

    /// The late network's energy per reflection band (the engine's own
    /// split), relative to a white signal's share — fed as at SPACE 1.
    #[test]
    #[ignore]
    fn late_band_balance() {
        let sr = 48_000.0;
        for &size in &[0.0f32, 0.5, 1.0] {
            for &t in &[0.5f32, 2.0, 6.0] {
                let mut room = Room::new();
                room.prepare(sr);
                let mut geometry = Geometry::new();
                let nodes: [[f32; 2]; LINES] = core::array::from_fn(|i| {
                    let (x, y) = crate::params::default_node(i);
                    [x as f32, y as f32]
                });
                geometry.reset();
                geometry.step(&nodes, 0.0, true);
                room.reshape(&geometry.radius(), &geometry.distances());
                room.settle();
                let shape = Shape { size, decay_s: t, tone: TONE_REF, hold: false, gen: 1.0 };
                let mut dl = self::super::early::FeedDiffuser::new();
                let mut dr = self::super::early::FeedDiffuser::new();
                dl.prepare(sr, 0);
                dr.prepare(sr, 1);
                let (mut xo_in, mut xo_out) = (self::super::early::Crossover::new(), self::super::early::Crossover::new());
                xo_in.prepare(sr);
                xo_out.prepare(sr);
                let (mut ein, mut eout) = ([0.0f64; 3], [0.0f64; 3]);
                let mut trace: [alloc::vec::Vec<f32>; 3] = Default::default();
                for i in 0..(t * 2.0 * sr) as usize {
                    if i % 256 == 0 {
                        room.aim_late(&[t; 7], shape);
                    }
                    let d = if i == 0 { 1.0 } else { 0.0 };
                    let y = room.tick([dl.tick(d), dr.tick(d)], shape);
                    for (b, v) in xo_in.split(d).iter().enumerate() {
                        ein[b] += f64::from(*v) * f64::from(*v);
                    }
                    for (b, v) in xo_out.split(y[0]).iter().enumerate() {
                        eout[b] += f64::from(*v) * f64::from(*v);
                        trace[b].push(*v * *v);
                    }
                }
                let tot: f64 = eout.iter().sum();
                let tin: f64 = ein.iter().sum();
                let r: alloc::vec::Vec<alloc::string::String> = (0..3).map(|b| alloc::format!("{:.2}", (eout[b] / tot) / (ein[b] / tin))).collect();
                // Each band's T60 from its Schroeder curve (−5 … −25 dB).
                let t60s: alloc::vec::Vec<alloc::string::String> = trace
                    .iter()
                    .map(|e| {
                        let mut edc: alloc::vec::Vec<f64> = e.iter().rev().scan(0.0f64, |a, v| { *a += f64::from(*v); Some(*a) }).collect();
                        edc.reverse();
                        let db = |i: usize| 10.0 * (edc[i] / edc[0]).log10();
                        let i5 = (0..edc.len()).find(|&i| db(i) <= -5.0).unwrap_or(0);
                        let i25 = (0..edc.len()).find(|&i| db(i) <= -25.0).unwrap_or(edc.len() - 1);
                        alloc::format!("{:.2}", 3.0 * (i25 - i5) as f64 / f64::from(sr))
                    })
                    .collect();
                std::println!("size {size} T60 {t}: band share / white share = {}   band T60s {}", r.join(" "), t60s.join(" "));
            }
        }
    }
}

#[cfg(test)]
mod clip_diag {
    use super::*;
    #[test]
    #[ignore]
    fn accounting_clips() {
        for (x, y) in [(0.2f32, 0.2f32), (0.5, 0.5), (0.3, 0.7), (0.7, 0.3), (0.8, 0.8), (0.2, 0.8), (0.8, 0.2)] {
            let a = decode(&at_map(x, y));
            let t = late_t60(&a, 3.0, TONE_REF);
            let size = 0.5;
            let whole = loudness_matched(&a, late_energy(t[1], size), t[1]);
            let late: [f32; 3] = t.map(|v| late_energy(v, size));
            let mut row = alloc::string::String::new();
            for b in 0..3 {
                let share = late_early_share(size, t[b]);
                let frac: f32 = a.early[b].iter().sum();
                let whole_b = whole * late[b] / late[1];
                let inside: f32 = share.iter().sum::<f32>().min(0.95);
                let late_total = whole_b * (1.0 - frac) / (1.0 - inside);
                let (mut tgt0, mut tgt1, mut lt0, mut lt1, mut clip) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
                for k in 0..23 {
                    let target = a.early[b][k] * whole_b;
                    let l = late_total * share[k];
                    clip += (l - target).max(0.0);
                    if k < 10 { tgt0 += target; lt0 += l; } else { tgt1 += target; lt1 += l; }
                }
                row += &alloc::format!(" | b{b} T {:.1} frac {frac:.2} late-in {inside:.2} clip {:.0}% of window; late/target 0–50ms {:.2} 50–115 {:.2}", t[b], 100.0 * clip / (tgt0 + tgt1), lt0 / tgt0.max(1e-9), lt1 / tgt1.max(1e-9));
            }
            std::println!("({x},{y}){row}");
        }
    }
}
