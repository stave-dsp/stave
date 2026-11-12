//! `Early` — the GENERATED early reflections: the first 120 ms of the
//! decoded room (space.rs), as sparse taps.
//!
//! What you play passes a crossover into low /
//! mid / high (500 Hz, 4 kHz — the bands the IRs were measured in; the
//! three sum back to the input exactly). Each band has its own taps, two
//! per 5 ms bin at fixed jittered times; a bin's taps carry the decoded
//! share of that band's energy. Three tap sets — shared, left-only,
//! right-only — blend to the decoded early L/R correlation.
//!
//! Bin k is k·5 ms after the DIRECT sound — the room's own gap before
//! its first reflection is in the pattern (empty early bins). The
//! direct sound itself was cut from the measured IRs (its first 2 ms),
//! so bin 0's taps sit at 2–5 ms: the dry path is the direct.
//!
//! Levels: the room's WHOLE energy is `whole` at the mid band (matched to
//! the knobs' room — every point of the map about as loud as any other);
//! each other band's follows its own decay (a band that rings shorter
//! carries less: `late_energy[b] / late_energy[mid]` of it). A bin's taps
//! carry its share of its band's whole, the late network — through the
//! feed, scaled by `feed_gain` — the rest.
//!
//! The FEED: the late network's input — the dry signal, band by band at
//! each band's late level, a moment after the direct. A real room's
//! reverberant decay starts AT the onset (measured: a plateau that
//! decays from t = 0); fed the smeared reflections instead, the tail's
//! decay began 100–150 ms late and bulged. (The lines' own diffusion
//! makes the network dense; the reflections fill what it leaves.)

use stave_dsp::{DelayLine, Svf};

use super::space::{Anatomy, ER_BANDS, ER_BINS, ER_BIN_S};

const TAPS: usize = 2;
const SETS: usize = 3; // shared, L, R
const FIRST_BIN: usize = 0;
/// Bin 0's taps start here (the direct's 2 ms were cut from the IRs).
const DIRECT_S: f32 = 0.002;
const MID: usize = 1;
const CROSSOVER_HZ: (f32, f32) = (500.0, 4_000.0);
/// Butterworth (k = √2) in the SVF's resonance terms.
/// 4th-order Butterworth as two SVF stages (Q 0.541, 1.307 in the SVF's
/// resonance terms, k = 1/Q = 2 − 2·res) — the SAME split the IRs were
/// measured with (ir_measure.py): with gentler 2nd-order slopes the low
/// band's reflections leaked past 500 Hz and every band's early share
/// came out wrong (low too weak, mid too strong).
const BUTTERWORTH4: [f32; 2] = [1.0 - 0.5 / 0.541_196, 1.0 - 0.5 / 1.306_563];

/// The 3-band split: low (4th-order LP), high (4th-order HP), mid = rest.
#[derive(Clone, Copy)]
pub(super) struct Crossover {
    low: [Svf; 2],
    high: [Svf; 2],
}

impl Crossover {
    pub(super) fn new() -> Self {
        Self { low: [Svf::new(); 2], high: [Svf::new(); 2] }
    }

    pub(super) fn prepare(&mut self, sr: f32) {
        for k in 0..2 {
            self.low[k].prepare(sr);
            self.high[k].prepare(sr);
            self.low[k].set(CROSSOVER_HZ.0, BUTTERWORTH4[k]);
            self.high[k].set(CROSSOVER_HZ.1, BUTTERWORTH4[k]);
        }
    }

    #[inline]
    pub(super) fn split(&mut self, x: f32) -> [f32; 3] {
        let l = self.low[0].process_lp(x, 0);
        let low = self.low[1].process_lp(l, 0);
        let h = self.high[0].process(x, 0).hp;
        let high = self.high[1].process(h, 0).hp;
        [low, x - low - high, high]
    }
}
/// Per-block glide of the tap gains (a morph never steps).
const GAIN_GLIDE: f32 = 0.3;

/// Deterministic jitter in [0, 1) for a tap.
fn jitter(set: usize, band: usize, bin: usize, tap: usize) -> f32 {
    let mut h = (set as u32)
        .wrapping_mul(7919)
        .wrapping_add((band as u32).wrapping_mul(104_729))
        .wrapping_add((bin as u32).wrapping_mul(31))
        .wrapping_add((tap as u32).wrapping_mul(2_654_435_761))
        .wrapping_mul(0x9E37_79B9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

fn sign(set: usize, band: usize, bin: usize, tap: usize) -> f32 {
    if jitter(set + 11, band + 5, bin + 3, tap + 1) < 0.5 {
        -1.0
    } else {
        1.0
    }
}

pub(super) struct Early {
    sr: f32,
    split: Crossover,
    bands: [DelayLine; ER_BANDS],
    /// Tap delays (samples), [set][band][bin][tap].
    at: [[[[usize; TAPS]; ER_BINS]; ER_BANDS]; SETS],
    /// Each tap's fixed sign, [set][band][bin][tap].
    signs: [[[[f32; TAPS]; ER_BINS]; ER_BANDS]; SETS],
    /// Each bin's tap level per band, each set's share of the output
    /// (stereo blend × amount), each band's feed level into the late
    /// network — RAMPED PER SAMPLE toward the block's (glided) target:
    /// a per-block step is a click (measured: morph clicks at block
    /// boundaries).
    level: [[f32; ER_BINS]; ER_BANDS],
    level_step: [[f32; ER_BINS]; ER_BANDS],
    mix: [f32; SETS],
    mix_step: [f32; SETS],
    feed_gain: [f32; ER_BANDS],
    feed_step: [f32; ER_BANDS],
    /// 1/√ of how much the crossover's bands' energies sum past the
    /// input's once decorrelated (they overlap at the crossovers; they
    /// sum back coherently, but the taps make them incoherent).
    deflate: f32,
    diffuser: [FeedDiffuser; 2],
}


/// The feed's INPUT DIFFUSION: four Schroeder allpasses (ms, gain) in
/// series. Without it the late network's first arrivals are lumps at
/// its lines' lengths — measured: per-bin lumps above the room's own
/// early energy, which the reflections can't take back (they can't go
/// negative), so the window ran loud and grainy. Diffused, the network
/// swells in smoothly and the reflections fill around it exactly.
const FEED_DIFFUSION: [[(f32, f32); 4]; 2] = [
    [(4.71, 0.7), (7.13, 0.7), (11.27, 0.62), (17.93, 0.62)],
    // The right side's: other lengths, so the two sides are DECORRELATED
    // (one mono feed made the tail too narrow — measured late IACC 0.22
    // against real rooms' 0.09).
    [(5.23, 0.7), (6.67, 0.7), (12.07, 0.62), (16.81, 0.62)],
];

pub(super) struct FeedDiffuser {
    stages: [(DelayLine, usize, f32); 4],
}

impl FeedDiffuser {
    pub(super) fn new() -> Self {
        Self { stages: core::array::from_fn(|_| (DelayLine::new(), 1, 0.0)) }
    }

    pub(super) fn prepare(&mut self, sr: f32, side: usize) {
        for (st, &(ms, g)) in self.stages.iter_mut().zip(&FEED_DIFFUSION[side]) {
            st.1 = (ms * 0.001 * sr).round().max(1.0) as usize;
            st.2 = g;
            st.0.prepare(st.1 + 2);
        }
    }

    #[inline]
    pub(super) fn tick(&mut self, mut x: f32) -> f32 {
        for (line, len, g) in &mut self.stages {
            let d = line.read_int(*len);
            let y = -*g * x + d;
            line.write(x + *g * y);
            line.advance();
            x = y;
        }
        x
    }
}

impl Early {
    pub(super) fn new() -> Self {
        Self {
            sr: 48_000.0,
            split: Crossover::new(),
            bands: core::array::from_fn(|_| DelayLine::new()),
            at: [[[[0; TAPS]; ER_BINS]; ER_BANDS]; SETS],
            signs: [[[[0.0; TAPS]; ER_BINS]; ER_BANDS]; SETS],
            level: [[0.0; ER_BINS]; ER_BANDS],
            level_step: [[0.0; ER_BINS]; ER_BANDS],
            mix: [0.0; SETS],
            mix_step: [0.0; SETS],
            feed_gain: [1.0; ER_BANDS],
            feed_step: [0.0; ER_BANDS],
            deflate: 1.0,
            diffuser: [FeedDiffuser::new(), FeedDiffuser::new()],
        }
    }

    pub(super) fn prepare(&mut self, sr: f32) {
        self.sr = sr;
        self.split.prepare(sr);
        // Measure the inflation: an impulse through these very filters.
        let mut energy = 0.0f32;
        for i in 0..(0.1 * sr) as usize {
            let x = if i == 0 { 1.0 } else { 0.0 };
            let [l, m, h] = self.split.split(x);
            energy += l * l + m * m + h * h;
        }
        self.split.prepare(sr);
        self.deflate = energy.max(1.0).sqrt().recip();
        self.diffuser[0].prepare(sr, 0);
        self.diffuser[1].prepare(sr, 1);
        let span = (ER_BINS as f32 * ER_BIN_S * sr) as usize + 4;
        for b in &mut self.bands {
            b.prepare(span);
        }
        let bin = ER_BIN_S * sr;
        for s in 0..SETS {
            for b in 0..ER_BANDS {
                for k in 0..ER_BINS {
                    for t in 0..TAPS {
                        // Taps spread across their bin: each in its own half.
                        let pos = (t as f32 + jitter(s, b, k, t)) / TAPS as f32;
                        let lo = if k == 0 { DIRECT_S / ER_BIN_S } else { 0.0 };
                        self.at[s][b][k][t] = ((k as f32 + lo + (1.0 - lo) * pos) * bin) as usize;
                    }
                }
            }
        }
        for s in 0..SETS {
            for b in 0..ER_BANDS {
                for k in 0..ER_BINS {
                    for t in 0..TAPS {
                        self.signs[s][b][k][t] = sign(s, b, k, t);
                    }
                }
            }
        }
        self.level_step = [[0.0; ER_BINS]; ER_BANDS];
        self.mix_step = [0.0; SETS];
        self.feed_step = [0.0; ER_BANDS];
    }

    /// Aim at a room. `whole`: the room's energy for a unit impulse;
    /// `late_energy[b]`: what the late network makes of a unit feed in
    /// band b, and `late_share[b][k]` the share of it landing in early
    /// bin k. `amount` 0 … 1 fades the reflections in. Once a block.
    ///
    /// The late network's early energy is COUNTED: its time profile (the
    /// feed is the reflections' own pattern, so the profile convolved
    /// with it) takes part of each bin, the taps supply what's missing,
    /// and its level is set so its energy after the window is the
    /// room's — the room's whole time–frequency envelope, not its parts.
    /// `frames`: the block the ramps run over.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn aim(&mut self, a: &Anatomy, whole: f32, late_energy: [f32; ER_BANDS], late_share: &[[f32; ER_BINS - 1]; ER_BANDS], amount: f32, settle: bool, frames: usize) {
        let glide = if settle { 1.0 } else { GAIN_GLIDE };
        let per = 1.0 / frames.max(1) as f32;
        // Ramp `cur` over this block to a GAIN_GLIDE step toward `to`.
        let ramp = |cur: &mut f32, step: &mut f32, to: f32| {
            if settle {
                *cur = to;
                *step = 0.0;
            } else {
                *step = (to - *cur) * glide * per;
            }
        };
        let rho = a.early_corr.clamp(0.0, 1.0);
        let mix_to = [rho.sqrt() * amount, (1.0 - rho).sqrt() * amount, (1.0 - rho).sqrt() * amount];
        for s in 0..SETS {
            ramp(&mut self.mix[s], &mut self.mix_step[s], mix_to[s]);
        }
        for b in 0..ER_BANDS {
            let early_share: f32 = a.early[b].iter().sum();
            let whole_b = whole * late_energy[b] / late_energy[MID].max(1e-6);
            // Where the late network's energy lands in the window.
            let r: [f32; ER_BINS] = core::array::from_fn(|k| if k < ER_BINS - 1 { late_share[b][k] } else { 0.0 });
            let inside = r.iter().sum::<f32>().min(0.95);
            // Its level: after the window, the room's remaining energy.
            let late_total = whole_b * (1.0 - early_share).max(0.0) / (1.0 - inside);
            let fg = (late_total / late_energy[b].max(1e-6)).sqrt();
            ramp(&mut self.feed_gain[b], &mut self.feed_step[b], fg);
            for k in 0..ER_BINS {
                #[allow(clippy::absurd_extreme_comparisons)] // FIRST_BIN is a tuning knob; 0 today
                let e = if k < FIRST_BIN { 0.0 } else { (a.early[b][k] * whole_b - late_total * r[k]).max(0.0) };
                let g = (e / TAPS as f32).sqrt() * self.deflate;
                ramp(&mut self.level[b][k], &mut self.level_step[b][k], g);
            }
        }
    }

    /// One frame of (mono) input → the reflections [L, R] and the FEED
    /// (the late network's input: the reflections, energy-flat, at each
    /// band's late level).
    #[inline]
    pub(super) fn tick(&mut self, x: f32) -> ([f32; 2], [f32; 2]) {
        let split = self.split.split(x);
        let mut out = [0.0f32; SETS];
        let mut feed = 0.0f32;
        for (b, line) in self.bands.iter_mut().enumerate() {
            line.write(split[b]);
            for k in 0..ER_BINS {
                self.level[b][k] += self.level_step[b][k];
            }
            let mut band = [0.0f32; SETS];
            for (s, o) in band.iter_mut().enumerate() {
                let (at, sg) = (&self.at[s][b], &self.signs[s][b]);
                for k in FIRST_BIN..ER_BINS {
                    let lv = self.level[b][k];
                    for t in 0..TAPS {
                        *o += sg[k][t] * lv * line.read_int(at[k][t]);
                    }
                }
            }
            line.advance();
            self.feed_gain[b] += self.feed_step[b];
            // The bands sum back COHERENTLY here (same delay): no deflate.
            feed += self.feed_gain[b] * split[b];
            for (o, v) in out.iter_mut().zip(band) {
                *o += v;
            }
        }
        for (m, d) in self.mix.iter_mut().zip(self.mix_step) {
            *m += d;
        }
        let shared = out[0] * self.mix[0];
        ([shared + out[1] * self.mix[1], shared + out[2] * self.mix[2]], [self.diffuser[0].tick(feed), self.diffuser[1].tick(feed)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::space::{decode, at_map};

    /// The reflections' impulse response puts its energy where the
    /// decoded room says: per 5 ms bin, each band's share against the
    /// given late energy, weighted by how much of the impulse each
    /// crossover band carries.
    #[test]
    fn the_reflections_follow_the_decoded_pattern() {
        let sr = 48_000.0;
        let a = decode(&at_map(0.3, 0.6));
        let mut e = Early::new();
        e.prepare(sr);
        e.aim(&a, 1.0, [1.0; 3], &[[0.0; ER_BINS - 1]; 3], 1.0, true, 256);
        let n = (0.13 * sr) as usize;
        let ir: Vec<[f32; 2]> = (0..n).map(|i| e.tick(if i == 0 { 1.0 } else { 0.0 }).0).collect();
        // The impulse's energy per crossover band (the same filters).
        let mut xo = Crossover::new();
        xo.prepare(sr);
        let mut f = [0.0f32; 3];
        for i in 0..n {
            let x = if i == 0 { 1.0 } else { 0.0 };
            for (fb, v) in f.iter_mut().zip(xo.split(x)) {
                *fb += v * v;
            }
        }
        let bin = (ER_BIN_S * sr) as usize;
        let (mut got, mut want, mut worst) = (0.0f32, 0.0f32, 0.0f32);
        for k in 2..ER_BINS - 1 {
            let g: f32 = ir[k * bin..((k + 1) * bin).min(n)].iter().map(|s| s[0] * s[0]).sum();
            let w: f32 = (0..3).map(|b| f[b] * a.early[b][k]).sum::<f32>() * e.deflate * e.deflate;
            got += g;
            want += w;
            if w > 1e-4 * want.max(1e-9) {
                worst = worst.max((g / w).ln().abs());
            }
        }
        // Bands overlap at the crossovers and taps straddle bin edges:
        // the total tightly, each bin within ×3.
        assert!((got / want).ln().abs() < 0.25, "early energy {got:.3e} vs {want:.3e}");
        assert!(worst < 3f32.ln(), "worst bin off by ×{:.2}", worst.exp());
    }

    /// Fully correlated rooms are mono; decorrelated ones are not.
    #[test]
    fn early_width_follows_the_room() {
        let sr = 48_000.0;
        for (rho, want_corr) in [(1.0f32, 0.99f32), (0.0, 0.3)] {
            let mut a = decode(&at_map(0.5, 0.5));
            a.early_corr = rho;
            let mut e = Early::new();
            e.prepare(sr);
            e.aim(&a, 1.0, [1.0; 3], &[[0.0; ER_BINS - 1]; 3], 1.0, true, 256);
            let ir: Vec<[f32; 2]> = (0..(0.13 * sr) as usize).map(|i| e.tick(if i == 0 { 1.0 } else { 0.0 }).0).collect();
            let (ll, rr, lr) = ir.iter().fold((0.0, 0.0, 0.0), |(a, b, c), s| (a + s[0] * s[0], b + s[1] * s[1], c + s[0] * s[1]));
            let corr = lr / (ll * rr).sqrt();
            if rho == 1.0 {
                assert!(corr > want_corr, "corr {corr}");
            } else {
                assert!(corr.abs() < want_corr, "corr {corr}");
            }
        }
    }
}
