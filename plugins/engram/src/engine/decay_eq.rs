//! `DecayEq` — each delay line's ATTENUATION FILTER for the generated
//! room: a graphic EQ in the feedback path whose gain at each of the 7
//! measured octaves (125 Hz … 8 kHz) is exactly what that octave needs
//! per pass to decay at its own T60 (−60 dB · length / T60). A real room
//! rings at a different rate in every band; a one- or two-band damping
//! filter can't say that (the mids then ring at the lows' rate, the tail
//! runs long and loud, the colour over time is wrong).
//!
//! The method is the accurate-GEQ attenuation filter of Schlecht &
//! Välimäki: a broadband gain carries the mean; a low shelf, five
//! peaks and a high shelf carry each octave's deviation, their gains
//! solved through the bands' measured INTERACTION matrix (each filter
//! leaks into its neighbours), then refined against the filters' true
//! response (large attenuations aren't linear in dB).

use stave_dsp::{Biquad, FilterSpec};

pub(super) const OCTAVES: usize = 7;
const CENTRES: [f32; OCTAVES] = [125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0];
/// The filters: a low shelf between the two lowest centres, peaks on
/// the middle five, a high shelf between the two highest.
const SHELF_LO_HZ: f32 = 176.8;
const SHELF_HI_HZ: f32 = 5657.0;
const PEAK_Q: f32 = 1.41;
/// The prototype gain the interaction matrix is measured at (dB).
const PROTO_DB: f32 = -6.0;
/// The most any one filter is asked to deviate from the mean (dB).
const MAX_DEV_DB: f32 = 30.0;
/// Refinement passes at most.
const REFINE: usize = 4;

fn spec(k: usize, db: f32) -> FilterSpec {
    match k {
        0 => FilterSpec::LowShelfRbj { hz: SHELF_LO_HZ, db },
        6 => FilterSpec::HighShelfRbj { hz: SHELF_HI_HZ, db },
        _ => FilterSpec::Peak { hz: CENTRES[k], db, q: PEAK_Q },
    }
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-9).log10()
}

/// Solve A·x = b (7×7, partial pivoting).
fn solve(mut a: [[f32; OCTAVES]; OCTAVES], mut b: [f32; OCTAVES]) -> [f32; OCTAVES] {
    for c in 0..OCTAVES {
        let p = (c..OCTAVES).max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs())).unwrap_or(c);
        a.swap(c, p);
        b.swap(c, p);
        let d = if a[c][c].abs() < 1e-9 { 1e-9 } else { a[c][c] };
        for r in c + 1..OCTAVES {
            let f = a[r][c] / d;
            for k in c..OCTAVES {
                a[r][k] -= f * a[c][k];
            }
            b[r] -= f * b[c];
        }
    }
    let mut x = [0.0f32; OCTAVES];
    for c in (0..OCTAVES).rev() {
        let s: f32 = (c + 1..OCTAVES).map(|k| a[c][k] * x[k]).sum();
        x[c] = (b[c] - s) / if a[c][c].abs() < 1e-9 { 1e-9 } else { a[c][c] };
    }
    x
}

/// The filter design for one sample rate: the interaction matrix (each
/// filter's dB response at every centre, per dB of its gain).
pub(super) struct Design {
    sr: f32,
    /// interaction[centre][filter]
    interaction: [[f32; OCTAVES]; OCTAVES],
    grid: [f32; GRID],
}

impl Design {
    pub(super) fn new(sr: f32) -> Self {
        let mut interaction = [[0.0; OCTAVES]; OCTAVES];
        for f in 0..OCTAVES {
            let mut bq = Biquad::new();
            bq.set(spec(f, PROTO_DB), sr);
            for (c, row) in interaction.iter_mut().enumerate() {
                row[f] = db(bq.magnitude_at(CENTRES[c].min(0.45 * sr), sr)) / PROTO_DB;
            }
        }
        let grid = core::array::from_fn(|k| (20.0 * 1000f32.powf(k as f32 / (GRID - 1) as f32)).min(0.45 * sr));
        Self { sr, interaction, grid }
    }

    /// Load `filters` (and return the broadband gain) so the cascade's
    /// response at the centres is `target_db` (per pass, ≤ 0).
    pub(super) fn fit(&self, target_db: &[f32; OCTAVES], filters: &mut [Biquad; OCTAVES]) -> f32 {
        let mean = target_db.iter().sum::<f32>() / OCTAVES as f32;
        let dev: [f32; OCTAVES] = core::array::from_fn(|k| target_db[k] - mean);
        let mut g = solve(self.interaction, dev);
        for v in &mut g {
            *v = v.clamp(-MAX_DEV_DB, MAX_DEV_DB);
        }
        for (k, f) in filters.iter_mut().enumerate() {
            f.set(spec(k, g[k]), self.sr);
        }
        // Refine against the true response until within 0.2 dB (typical
        // rooms: once; extreme ones: a few times).
        for _ in 0..REFINE {
            let resp: [f32; OCTAVES] = core::array::from_fn(|c| filters.iter().map(|f| db(f.magnitude_at(CENTRES[c].min(0.45 * self.sr), self.sr))).sum());
            let resid: [f32; OCTAVES] = core::array::from_fn(|c| dev[c] - resp[c]);
            if resid.iter().all(|r| r.abs() < 0.2) {
                break;
            }
            let dg = solve(self.interaction, resid);
            for (k, f) in filters.iter_mut().enumerate() {
                g[k] = (g[k] + dg[k]).clamp(-MAX_DEV_DB, MAX_DEV_DB);
                f.set(spec(k, g[k]), self.sr);
            }
        }
        // STABILITY, guaranteed: the fit is exact at the centres but can
        // overshoot between them; anywhere the loop gain reached 0 dB
        // the tail would never decay (measured: one room rang forever).
        // So the whole response, on a dense grid, stays below −MARGIN.
        let resp = |hz: f32| mean + filters.iter().map(|f| db(f.magnitude_at(hz, self.sr))).sum::<f32>();
        let coarse: [f32; GRID] = core::array::from_fn(|k| resp(self.grid[k]));
        let mut peak = coarse.iter().copied().fold(f32::MIN, f32::max);
        // Ripples can be narrower than the grid: search finely around
        // EVERY local maximum (a narrow ripple can sit below a broad
        // plateau elsewhere on the grid and still peak above it).
        for k in 0..GRID {
            let left = if k > 0 { coarse[k - 1] } else { f32::MIN };
            let right = if k + 1 < GRID { coarse[k + 1] } else { f32::MIN };
            if coarse[k] < left || coarse[k] < right {
                continue;
            }
            let (lo, hi) = (self.grid[k.saturating_sub(1)], self.grid[(k + 1).min(GRID - 1)]);
            for j in 1..16 {
                peak = peak.max(resp(lo * (hi / lo).powf(j as f32 / 16.0)));
            }
        }
        let mean = if peak > -MARGIN_DB { mean - (peak + MARGIN_DB) } else { mean };
        10f32.powf(mean / 20.0)
    }
}

/// Where stability is checked: 20 Hz … 20 kHz, log-spaced (≈ 1/6 oct).
const GRID: usize = 64;
/// The least any frequency is attenuated per pass (dB).
const MARGIN_DB: f32 = 0.01;

/// A line's attenuation filter: broadband gain × the cascade.
#[derive(Clone)]
pub(super) struct DecayEq {
    pub(super) gain: f32,
    pub(super) filters: [Biquad; OCTAVES],
}

impl DecayEq {
    pub(super) fn new() -> Self {
        Self { gain: 1.0, filters: core::array::from_fn(|_| Biquad::new()) }
    }

    /// The cascade alone — the SHAPE of the attenuation, its broadband
    /// `gain` applied where the line feeds back (as the knobs' room
    /// damps what it hears and attenuates only what it feeds back).
    #[inline]
    pub(super) fn shape(&mut self, x: f32) -> f32 {
        let mut y = x;
        for f in &mut self.filters {
            y = f.process(y);
        }
        y
    }

    /// The per-pass target for a line of `len` samples: each octave
    /// decays 60 dB in its T60. (The room adds its diffusers' delay.)
    #[cfg(test)]
    pub(super) fn targets(len: f32, sr: f32, t60: &[f32; OCTAVES]) -> [f32; OCTAVES] {
        core::array::from_fn(|k| -60.0 * len / (t60[k].max(0.05) * sr))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cascade hits each octave's per-pass target: typical rooms
    /// within 0.3 dB, extreme ones (a bright 0.2 s room on a 190 ms
    /// line) within 1.5 dB.
    /// Whatever is asked, the loop never gains anywhere: wild targets
    /// (bright and dull, long and short) stay below 0 dB on a fine grid.
    #[test]
    fn no_frequency_ever_gains() {
        let sr = 48_000.0;
        let d = Design::new(sr);
        let mut rng = 0x1234_5678u32;
        let mut rnd = || {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            rng as f32 / u32::MAX as f32
        };
        for _ in 0..300 {
            let len = (5.0 + 200.0 * rnd()) * 0.001 * sr;
            let t60: [f32; OCTAVES] = core::array::from_fn(|_| 0.1 * 300f32.powf(rnd()));
            let mut eq = DecayEq::new();
            eq.gain = d.fit(&DecayEq::targets(len, sr, &t60), &mut eq.filters);
            for k in 0..400 {
                let hz = 20.0 * 1000f32.powf(k as f32 / 399.0);
                let g = db(eq.gain) + eq.filters.iter().map(|f| db(f.magnitude_at(hz.min(0.45 * sr), sr))).sum::<f32>();
                assert!(g < 0.05, "{hz:.0} Hz gains {g:.2} dB (len {len:.0}, t60 {t60:?})");
            }
        }
    }

    #[test]
    fn the_filter_hits_every_octave() {
        let sr = 48_000.0;
        let d = Design::new(sr);
        for (len_ms, t60, tol) in [
            (50.0f32, [2.5f32, 2.3, 2.0, 1.8, 1.6, 1.3, 0.9], 0.3f32),
            (120.0, [6.0, 5.5, 4.0, 3.6, 3.5, 3.7, 4.5], 0.3),
            (30.0, [0.6, 0.5, 0.45, 0.4, 0.35, 0.3, 0.2], 0.3),
            (190.0, [1.0, 0.8, 0.5, 0.3, 0.25, 0.22, 0.2], 1.5),
        ] {
            let len = len_ms * 0.001 * sr;
            let target = DecayEq::targets(len, sr, &t60);
            let mut eq = DecayEq::new();
            eq.gain = d.fit(&target, &mut eq.filters);
            let _ = eq.shape(0.0);
            for (k, &c) in CENTRES.iter().enumerate() {
                let got = db(eq.gain) + eq.filters.iter().map(|f| db(f.magnitude_at(c, sr))).sum::<f32>();
                assert!((got - target[k]).abs() < tol, "len {len_ms} ms, {c} Hz: {got:.2} dB vs {:.2}", target[k]);
            }
        }
    }
}
