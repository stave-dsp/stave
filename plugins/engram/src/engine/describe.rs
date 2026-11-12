//! `Describe` — one window's DESCRIPTOR: what the ear heard over the
//! last grain (eight sub-blocks, the rhythm's steps), as 64 numbers that
//! the learned embedding (embed.rs) turns into the memory's key.
//!
//! The SAME function describes a memory (the window that is its grain)
//! and the query (the last grain's worth, refreshed every sub-block),
//! so the two are measured identically — no running-average query
//! against block-exact memories.
//!
//! Everything in it is loudness-invariant, and as much as possible
//! PITCH-invariant (the network learns the rest from multisampled
//! instruments): the spectrum's shape, how each band flickers and
//! drifts, the level envelope's slope and spikiness, onset density,
//! tonality, the harmony's INTERVAL content (chroma autocorrelation —
//! a major triad reads the same in any key), and the groove.

use super::listen::{rhythm_of, unit, BANDS, CLASSES, RHYTHM, STEPS};

/// The descriptor's length.
#[allow(dead_code)] // the describer pipeline: wired in, not yet read
pub(super) const D: usize = 3 * BANDS + 5 + 6 + RHYTHM;

/// One sub-block's running sums (over the frames you were playing).
#[derive(Copy, Clone, Default)]
struct Sub {
    n: f32,
    logs: [f32; BANDS],
    logs_sq: [f32; BANDS],
    level: f32,
    level_sq: f32,
    level_max: f32,
    onsets: f32,
    chroma: [f32; CLASSES],
    tonal: f32,
}

pub(super) struct Describe {
    ring: [Sub; STEPS],
    at: usize,
    cur: Sub,
    t: usize,
    len: usize,
    fresh: bool,
}

/// A log level floor (the level envelope's, in log10).
const LEVEL_FLOOR: f32 = 1e-5;

impl Describe {
    pub(super) fn new() -> Self {
        Self { ring: [Sub::default(); STEPS], at: 0, cur: Sub::default(), t: 0, len: 1, fresh: false }
    }

    /// `len`: ear frames per sub-block (the rhythm's step).
    pub(super) fn prepare(&mut self, len: usize) {
        self.ring = [Sub::default(); STEPS];
        self.at = 0;
        self.cur = Sub::default();
        self.t = 0;
        self.len = len.max(1);
        self.fresh = false;
    }

    /// One ear frame. Returns true when a sub-block just closed (a new
    /// window is ready).
    #[inline]
    #[allow(clippy::too_many_arguments)]
    pub(super) fn push(&mut self, present: bool, logs: &[f32; BANDS], level: f32, onset: bool, chroma_env: &[f32; CLASSES], tonal: f32) -> bool {
        let c = &mut self.cur;
        if onset {
            c.onsets += 1.0;
        }
        if present {
            c.n += 1.0;
            for b in 0..BANDS {
                c.logs[b] += logs[b];
                c.logs_sq[b] += logs[b] * logs[b];
            }
            let l = (level + LEVEL_FLOOR).log10();
            c.level += l;
            c.level_sq += l * l;
            c.level_max = if c.n == 1.0 { l } else { c.level_max.max(l) };
            for (a, v) in c.chroma.iter_mut().zip(chroma_env) {
                *a += v;
            }
            c.tonal += tonal;
        }
        self.t += 1;
        if self.t < self.len {
            return false;
        }
        self.t = 0;
        self.ring[self.at] = core::mem::take(&mut self.cur);
        self.at = (self.at + 1) % STEPS;
        self.fresh = true;
        true
    }

    /// The descriptor of the last STEPS sub-blocks (oldest first), or
    /// None if you played in none of them.
    #[allow(dead_code)] // the describer pipeline: wired in, not yet read
    pub(super) fn descriptor(&self) -> Option<[f32; D]> {
        let subs: [&Sub; STEPS] = core::array::from_fn(|k| &self.ring[(self.at + k) % STEPS]);
        let n: f32 = subs.iter().map(|s| s.n).sum();
        if n < 1.0 {
            return None;
        }
        let mut d = [0.0f32; D];
        // Bands: mean (centred), spread over frames, drift across subs.
        let mut mean = [0.0f32; BANDS];
        for b in 0..BANDS {
            let (s, sq): (f32, f32) = subs.iter().fold((0.0, 0.0), |(a, q), u| (a + u.logs[b], q + u.logs_sq[b]));
            mean[b] = s / n;
            d[BANDS + b] = (sq / n - mean[b] * mean[b]).max(0.0).sqrt();
            let (mut m, mut k) = (0.0f32, 0.0f32);
            for u in subs.iter().filter(|u| u.n > 0.0) {
                m += u.logs[b] / u.n;
                k += 1.0;
            }
            let m = m / k.max(1.0);
            let v = subs.iter().filter(|u| u.n > 0.0).map(|u| (u.logs[b] / u.n - m).powi(2)).sum::<f32>() / k.max(1.0);
            d[2 * BANDS + b] = v.sqrt();
        }
        let avg = mean.iter().sum::<f32>() / BANDS as f32;
        for b in 0..BANDS {
            d[b] = mean[b] - avg;
        }
        // The level envelope: spread, spikiness, slope across the window.
        let o = 3 * BANDS;
        let (ls, lsq): (f32, f32) = subs.iter().fold((0.0, 0.0), |(a, q), u| (a + u.level, q + u.level_sq));
        let lm = ls / n;
        let lmax = subs.iter().filter(|u| u.n > 0.0).map(|u| u.level_max).fold(f32::MIN, f32::max);
        d[o] = (lsq / n - lm * lm).max(0.0).sqrt();
        d[o + 1] = lmax - lm;
        let (mut sxy, mut sxx, mut k, mut tm, mut ym) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
        for (i, u) in subs.iter().enumerate().filter(|(_, u)| u.n > 0.0) {
            tm += i as f32;
            ym += u.level / u.n;
            k += 1.0;
        }
        if k > 1.0 {
            tm /= k;
            ym /= k;
            for (i, u) in subs.iter().enumerate().filter(|(_, u)| u.n > 0.0) {
                sxy += (i as f32 - tm) * (u.level / u.n - ym);
                sxx += (i as f32 - tm).powi(2);
            }
        }
        d[o + 2] = if sxx > 0.0 { sxy / sxx } else { 0.0 };
        let onsets: f32 = subs.iter().map(|u| u.onsets).sum();
        d[o + 3] = onsets / STEPS as f32;
        d[o + 4] = subs.iter().map(|u| u.tonal).sum::<f32>() / n;
        // Harmony's interval content: circular autocorrelation of the
        // unit chroma, lags 1…6 (key-invariant).
        let o = o + 5;
        let mut ch = [0.0f32; CLASSES];
        for u in &subs {
            for (a, v) in ch.iter_mut().zip(&u.chroma) {
                *a += v;
            }
        }
        let ch = unit(&ch);
        for lag in 1..=6 {
            d[o + lag - 1] = (0..CLASSES).map(|c| ch[c] * ch[(c + lag) % CLASSES]).sum();
        }
        // The groove.
        let o = o + 6;
        let steps: [f32; STEPS] = core::array::from_fn(|k| subs[k].onsets);
        let r = rhythm_of(&steps);
        d[o..o + RHYTHM].copy_from_slice(&r);
        Some(d)
    }

    /// The share of the window's ear frames you were playing in.
    #[allow(dead_code)] // the describer pipeline: wired in, not yet read
    pub(super) fn played(&self) -> f32 {
        self.ring.iter().map(|u| u.n).sum::<f32>() / (STEPS * self.len) as f32
    }

    /// Did a sub-block close since the last `take_fresh`?
    #[allow(dead_code)] // the describer pipeline: wired in, not yet read
    pub(super) fn take_fresh(&mut self) -> bool {
        core::mem::take(&mut self.fresh)
    }
}
