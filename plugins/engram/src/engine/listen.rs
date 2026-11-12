//! `Listen` — the memory's EAR. It hears three things about what you
//! play, so memory can be by harmony, by sound, or by groove:
//!
//! * HARMONY — chroma: twelve pitch classes, each four narrow band-passes
//!   an octave apart (C2 … B5), summed and enveloped. An A-minor chord
//!   reads the same in any voicing.
//! * TIMBRE — the spectrum's SHAPE and how it MOVES: sixteen bands
//!   (50 Hz … 16 kHz, filtered at the host rate so hats and air are
//!   heard), log-enveloped; the fingerprint is each band's mean level
//!   (mean removed — loudness does not count) and, at half weight, its
//!   spread over the grain (a sustained pad holds still, a shaker
//!   flickers, a decaying note falls). Chosen by the corpus feature
//!   study (features.rs): 2.2× the category accuracy of the spectrum's
//!   shape alone at 4.8 kHz.
//! * RHYTHM — onsets (a fast envelope leaping over a slow one), counted
//!   into eight steps across the last grain; the fingerprint is the
//!   pattern's density and DFT magnitudes (all four bins, to the
//!   every-other-step pulse) — SHIFT-INVARIANT, so a groove
//!   matches itself whichever beat it starts on.
//!
//! And how TONAL it is: how concentrated the chroma is (noise lights
//! every pitch class alike; a chord lights a few). When you play
//! drums, harmony is discounted automatically — the chroma of noise
//! means nothing.
//!
//! It also tells PRESENCE: whether you are playing, held through the
//! gaps between hits (a sparse groove is still playing).

use stave_dsp::Svf;

use super::describe::{Describe, D};

pub(super) const CLASSES: usize = 12;
pub(super) const BANDS: usize = 16;
/// The timbre fingerprint: band means (centred) + band spreads.
pub(super) const TIMBRE: usize = 2 * BANDS;
/// The spreads' weight in it.
const SPREAD_W: f32 = 0.5;
pub(super) const STEPS: usize = 8;
/// The rhythm fingerprint: density + |DFT| bins 1..4 of the eight-step
/// pattern (bin 4 is the every-other-step pulse — a hat's eighths).
pub(super) const RHYTHM: usize = 5;
const OCTAVES: usize = 4;
const LOW_OCTAVE: i32 = 2;
const Q: f32 = 20.0;
const ATTACK_MS: f32 = 12.0;
const RELEASE_MS: f32 = 150.0;
const BAND_LO_HZ: f32 = 50.0;
const BAND_HI_HZ: f32 = 16_000.0;
const BAND_Q: f32 = 1.4;
/// The live timbre's memory (s): its running mean and spread span about
/// a grain, as a stored memory's do.
const TIMBRE_TAU_S: f32 = 0.35;
/// Input below this mean level is silence (≈ −54 dBFS); presence holds
/// this long through gaps.
pub(super) const GATE: f32 = 0.002;
const HOLD_S: f32 = 0.4;
/// Onsets: the fast envelope must leap this far over the slow one, and
/// no two onsets closer than the refractory time.
const ONSET_RATIO: f32 = 1.8;
const REFRACTORY_S: f32 = 0.06;

fn class_hz(class: usize, octave: i32) -> f32 {
    16.351_6 * 2f32.powf(octave as f32 + class as f32 / 12.0)
}

fn coef(ms: f32, rate: f32) -> f32 {
    1.0 - (-1000.0 / (ms * rate)).exp()
}

/// The ear's three fingerprints at one moment, and how tonal it is.
#[derive(Copy, Clone, Default)]
pub(super) struct Hearing {
    pub(super) chroma: [f32; CLASSES],
    /// The live timbre fingerprint (running mean + spread).
    pub(super) timbre: [f32; TIMBRE],
    /// This moment's band levels (log10): a memory's timbre is these
    /// gathered over its grain.
    pub(super) logs: [f32; BANDS],
    pub(super) tonal: f32,
    /// The input's FAST envelope (1.5 ms up, 15 ms down) — each hit is
    /// a spike in it; a sustained sound holds it level.
    pub(super) level: f32,
}

pub(super) struct Listen {
    rate: f32,
    classes: [[Svf; OCTAVES]; CLASSES],
    chroma_env: [f32; CLASSES],
    bands: [Svf; BANDS],
    band_env: [f32; BANDS],
    logs: [f32; BANDS],
    log_mean: [f32; BANDS],
    log_sq: [f32; BANDS],
    timbre_k: f32,
    att: f32,
    rel: f32,
    band_att: f32,
    band_rel: f32,
    level: f32,
    level_k: f32,
    hold: f32,
    /// Onset detector: fast and slow envelopes, refractory countdown.
    fast: f32,
    slow: f32,
    fast_att: f32,
    fast_rel: f32,
    slow_k: f32,
    refractory: f32,
    onset: bool,
    /// The rolling rhythm: onsets per step over the last grain.
    steps: [f32; STEPS],
    /// The window descriptor (same sub-blocks as the rhythm's steps).
    describe: Describe,
    step_at: usize,
    step_len: usize,
    step_t: usize,
}

impl Listen {
    pub(super) fn new() -> Self {
        Self {
            rate: 12_000.0,
            classes: core::array::from_fn(|_| core::array::from_fn(|_| Svf::new())),
            chroma_env: [0.0; CLASSES],
            bands: core::array::from_fn(|_| Svf::new()),
            band_env: [0.0; BANDS],
            logs: [SILENT_LOG; BANDS],
            log_mean: [SILENT_LOG; BANDS],
            log_sq: [SILENT_LOG * SILENT_LOG; BANDS],
            timbre_k: 0.0,
            att: 0.0,
            rel: 0.0,
            band_att: 0.0,
            band_rel: 0.0,
            level: 0.0,
            level_k: 0.0,
            hold: 0.0,
            fast: 0.0,
            slow: 0.0,
            fast_att: 0.0,
            fast_rel: 0.0,
            slow_k: 0.0,
            refractory: 0.0,
            onset: false,
            steps: [0.0; STEPS],
            describe: Describe::new(),
            step_at: 0,
            step_len: 1,
            step_t: 0,
        }
    }

    /// Prepare at the ear's own rate (`host` is the host's: the timbre
    /// bands run there); `grain_s` is the memory's grain (the rhythm's
    /// window).
    pub(super) fn prepare(&mut self, host: f32, rate: f32, grain_s: f32) {
        self.rate = rate;
        let res = 1.0 - 1.0 / (2.0 * Q);
        for (c, row) in self.classes.iter_mut().enumerate() {
            for (k, f) in row.iter_mut().enumerate() {
                f.prepare(rate);
                f.set(class_hz(c, LOW_OCTAVE + k as i32).min(0.45 * rate), res);
            }
        }
        let band_res = 1.0 - 1.0 / (2.0 * BAND_Q);
        for (b, f) in self.bands.iter_mut().enumerate() {
            let hz = BAND_LO_HZ * (BAND_HI_HZ / BAND_LO_HZ).powf(b as f32 / (BANDS - 1) as f32);
            f.prepare(host);
            f.set(hz.min(0.45 * host), band_res);
        }
        self.chroma_env = [0.0; CLASSES];
        self.band_env = [0.0; BANDS];
        self.logs = [SILENT_LOG; BANDS];
        self.log_mean = [SILENT_LOG; BANDS];
        self.log_sq = [SILENT_LOG * SILENT_LOG; BANDS];
        self.timbre_k = 1.0 - (-1.0 / (TIMBRE_TAU_S * rate)).exp();
        self.att = coef(ATTACK_MS, rate);
        self.rel = coef(RELEASE_MS, rate);
        self.band_att = coef(5.0, host);
        self.band_rel = coef(80.0, host);
        self.level = 0.0;
        self.level_k = coef(60.0, rate);
        self.hold = 0.0;
        self.fast = 0.0;
        self.slow = 0.0;
        self.fast_att = coef(1.5, rate);
        self.fast_rel = coef(15.0, rate);
        self.slow_k = coef(60.0, rate);
        self.refractory = 0.0;
        self.onset = false;
        self.steps = [0.0; STEPS];
        self.step_at = 0;
        self.step_len = ((grain_s / STEPS as f32) * rate).max(1.0) as usize;
        self.describe.prepare(self.step_len);
        self.step_t = 0;
    }

    /// One HOST frame: the timbre bands (they need the whole spectrum).
    #[inline]
    pub(super) fn air(&mut self, x: f32) {
        for b in 0..BANDS {
            let a = (self.bands[b].process(x, 0).bp / BAND_Q).abs();
            let k = if a > self.band_env[b] { self.band_att } else { self.band_rel };
            self.band_env[b] += (a - self.band_env[b]) * k;
        }
    }

    /// One EAR frame (decimated).
    #[inline]
    pub(super) fn hear(&mut self, u: f32) {
        for c in 0..CLASSES {
            let s: f32 = self.classes[c].iter_mut().map(|f| f.process(u, 0).bp).sum::<f32>() / Q;
            let a = s.abs();
            let k = if a > self.chroma_env[c] { self.att } else { self.rel };
            self.chroma_env[c] += (a - self.chroma_env[c]) * k;
        }
        for b in 0..BANDS {
            let l = (self.band_env[b] + 1e-6).log10();
            self.logs[b] = l;
            self.log_mean[b] += (l - self.log_mean[b]) * self.timbre_k;
            self.log_sq[b] += (l * l - self.log_sq[b]) * self.timbre_k;
        }
        let a = u.abs();
        self.level += (a - self.level) * self.level_k;
        self.hold = if self.level > GATE { HOLD_S } else { (self.hold - 1.0 / self.rate).max(0.0) };

        // Onsets.
        let k = if a > self.fast { self.fast_att } else { self.fast_rel };
        self.fast += (a - self.fast) * k;
        self.slow += (self.fast - self.slow) * self.slow_k;
        self.refractory = (self.refractory - 1.0 / self.rate).max(0.0);
        self.onset = false;
        if self.refractory == 0.0 && self.fast > ONSET_RATIO * self.slow + GATE {
            self.onset = true;
            self.refractory = REFRACTORY_S;
            self.steps[self.step_at] += 1.0;
        }
        let present = self.hold > 0.0;
        self.describe.push(present, &self.logs, self.fast, self.onset, &self.chroma_env, tonality(&self.chroma_env));
        self.step_t += 1;
        if self.step_t >= self.step_len {
            self.step_t = 0;
            self.step_at = (self.step_at + 1) % STEPS;
            self.steps[self.step_at] = 0.0;
        }
    }

    /// Ear frames per window sub-block (a grain is `STEPS` of them).
    #[allow(dead_code)] // the describer pipeline: wired in, not yet read
    pub(super) fn step_len(&self) -> usize {
        self.step_len
    }

    /// The descriptor of the last grain's window.
    #[allow(dead_code)] // the describer pipeline: wired in, not yet read
    pub(super) fn descriptor(&self) -> Option<[f32; D]> {
        self.describe.descriptor()
    }

    /// The share of the window you were playing in.
    #[allow(dead_code)] // the describer pipeline: wired in, not yet read
    pub(super) fn window_played(&self) -> f32 {
        self.describe.played()
    }

    /// Did a new window close since last asked?
    #[allow(dead_code)] // the describer pipeline: wired in, not yet read
    pub(super) fn window_fresh(&mut self) -> bool {
        self.describe.take_fresh()
    }

    /// Playing — held through the gaps between hits.
    pub(super) fn present(&self) -> bool {
        self.hold > 0.0
    }

    /// Did an onset just happen (this ear step)?
    pub(super) fn onset(&self) -> bool {
        self.onset
    }

    pub(super) fn hearing(&self) -> Hearing {
        let spread: [f32; BANDS] = core::array::from_fn(|b| (self.log_sq[b] - self.log_mean[b] * self.log_mean[b]).max(0.0).sqrt());
        Hearing {
            chroma: unit(&self.chroma_env),
            timbre: timbre_key(&self.log_mean, &spread),
            logs: self.logs,
            tonal: tonality(&self.chroma_env),
            level: self.fast,
        }
    }

    /// The groove of the last grain, shift-invariant.
    pub(super) fn rhythm(&self) -> [f32; RHYTHM] {
        rhythm_of(&self.steps)
    }
}

pub(super) fn unit<const N: usize>(v: &[f32; N]) -> [f32; N] {
    let n = v.iter().map(|e| e * e).sum::<f32>().sqrt();
    if n < 1e-9 {
        return [0.0; N];
    }
    core::array::from_fn(|c| v[c] / n)
}

/// A silent band's log level.
const SILENT_LOG: f32 = -6.0;

/// The timbre fingerprint from band log MEANS and SPREADS: the means
/// centred (loudness does not count), the spreads at `SPREAD_W`, unit.
pub(super) fn timbre_key(mean: &[f32; BANDS], spread: &[f32; BANDS]) -> [f32; TIMBRE] {
    if mean.iter().all(|m| *m <= SILENT_LOG + 0.01) {
        return [0.0; TIMBRE];
    }
    let avg = mean.iter().sum::<f32>() / BANDS as f32;
    unit(&core::array::from_fn(|i| if i < BANDS { mean[i] - avg } else { SPREAD_W * spread[i - BANDS] }))
}

/// How TONAL: how concentrated the chroma's energy is — the share in
/// its four strongest pitch classes, rescaled so flat noise (a third of
/// it, four of twelve) reads 0 and a chord (most of it) reads near 1.
/// (A flatness measure fails here: the band-passes leak, so no pitch
/// class ever reads near zero.)
fn tonality(env: &[f32; CLASSES]) -> f32 {
    let total: f32 = env.iter().map(|e| e * e).sum();
    if total < 1e-12 {
        return 0.0;
    }
    let mut e: [f32; CLASSES] = core::array::from_fn(|c| env[c] * env[c]);
    e.sort_by(|a, b| b.total_cmp(a));
    let share = e[..4].iter().sum::<f32>() / total;
    ((share - 0.4) / 0.45).clamp(0.0, 1.0)
}

/// A step pattern's groove: [density, |X1| … |X4|], unit length
/// (|DFT| does not care where the pattern starts).
pub(super) fn rhythm_of(steps: &[f32; STEPS]) -> [f32; RHYTHM] {
    let total: f32 = steps.iter().sum();
    if total <= 0.0 {
        return [0.0; RHYTHM];
    }
    let mut v = [0.0f32; RHYTHM];
    v[0] = total / STEPS as f32;
    for k in 1..RHYTHM {
        let (mut re, mut im) = (0.0, 0.0);
        for (n, s) in steps.iter().enumerate() {
            let a = core::f32::consts::TAU * (k * n) as f32 / STEPS as f32;
            re += s * a.cos();
            im -= s * a.sin();
        }
        v[k] = re.hypot(im) / total;
    }
    unit(&v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_groove_matches_itself_on_any_beat() {
        let a = [1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0];
        let shifted = [0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0];
        let other = [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
        let (ra, rs, ro) = (rhythm_of(&a), rhythm_of(&shifted), rhythm_of(&other));
        let cos = |x: &[f32; RHYTHM], y: &[f32; RHYTHM]| x.iter().zip(y).map(|(p, q)| p * q).sum::<f32>();
        assert!(cos(&ra, &rs) > 0.999, "a shifted groove is the same groove");
        assert!(cos(&ra, &ro) < 0.9, "a different groove is not");
    }

    #[test]
    fn noise_is_not_tonal_and_a_chord_is() {
        let flat = [1.0f32; CLASSES];
        // Leaky, as the real band-passes are: every class reads a bit.
        let mut chord = [0.15f32; CLASSES];
        chord[0] = 1.0;
        chord[4] = 0.8;
        chord[7] = 0.9;
        assert!(tonality(&flat) < 0.05);
        assert!(tonality(&chord) > 0.6);
    }
}
