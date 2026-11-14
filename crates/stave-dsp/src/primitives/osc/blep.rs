//! The band-limited corrections [`Osc`](super::Osc) adds at each
//! discontinuity: the two-sample polynomial step and ramp (polyBLEP,
//! polyBLAMP) and the tabulated minimum-phase step and ramp (minBLEP,
//! minBLAMP).
//!
//! Every correction is a *residual*: the band-limited edge minus the
//! naive one, added to the naive waveform around the edge. Steps are in
//! value units (a jump `dv`), ramps in value per sample (a slope change
//! `ds`); an edge at fraction `f` of the interval between samples `n`
//! and `n + 1`.

use crate::primitives::spectral::{Fir, Passband, Window};
use crate::units::Db;

/// The tables, generated from the design in [`MinBlep::design`] and held
/// to it by `tables_match_their_design` (`STAVE_BLESS=1` rewrites them).
mod tables;

/// polyBLEP and polyBLAMP: the residuals of a jump `dv` and a slope
/// change `ds` at fraction `f`, for the sample before the edge (`n`) and
/// the one after (`n + 1`). The step is Välimäki and Huovilainen's
/// two-sample polynomial (2007); the ramp is its integral,
/// `(1 − |τ|)³/6` at `τ` samples from the edge (Esqueda, Välimäki and
/// Bilbao, "Rounding corners with BLAMP", DAFx 2016).
#[inline]
pub(crate) fn poly(f: f64, dv: f64, ds: f64) -> (f64, f64) {
    let g = 1.0 - f;
    let before = 0.5 * dv * g * g + ds * g * g * g / 6.0;
    let after = -0.5 * dv * f * f + ds * f * f * f / 6.0;
    (before, after)
}

/// The polyBLAMP residual of a unit slope change at `tau` samples from
/// it (either side), zero beyond one sample.
#[inline]
pub(super) fn poly_ramp(tau: f64) -> f64 {
    let a = 1.0 - tau.abs();
    if a > 0.0 {
        a * a * a / 6.0
    } else {
        0.0
    }
}

/// Table points per sample.
const OVERSAMPLE: usize = 64;
/// Zero crossings each side of the prototype sinc.
const ZEROS: usize = 32;
/// The prototype's cutoff, a fraction of the base rate: the stopband
/// (Kaiser, 100 dB) starts at Nyquist.
const CUTOFF: f64 = 0.46;
const ATTENUATION_DB: f64 = 100.0;

/// The minimum-phase band-limited step and ramp, tabulated.
pub(crate) struct MinBlep {
    /// The band-limited unit step at `i / OVERSAMPLE` samples after the
    /// edge, settled to 1 by the table's end.
    step: &'static [f64],
    /// Its integral: the band-limited unit ramp.
    ramp: &'static [f64],
    /// Whole samples the band-limited edge lags the naive one: the
    /// prototype is shifted so its group delay at DC is exactly this, so
    /// the ramp residual settles to zero against a naive waveform
    /// delayed by it.
    pub delay: usize,
    /// Samples after the edge the residuals reach.
    pub len: usize,
}

impl MinBlep {
    /// The tables, as compile-time constants: no allocation, no global
    /// state, the same on every platform.
    pub const TABLES: MinBlep = MinBlep {
        step: tables::STEP,
        ramp: tables::RAMP,
        delay: tables::DELAY,
        len: tables::LEN,
    };

    /// The design the tables come from: `(step, ramp, delay, len)`.
    pub(crate) fn design() -> (Vec<f64>, Vec<f64>, usize, usize) {
        let taps = 2 * ZEROS * OVERSAMPLE + 1;
        let lowpass = Fir::windowed_sinc(
            Passband::Lowpass {
                cutoff: CUTOFF / OVERSAMPLE as f64,
            },
            taps,
            Window::kaiser_for(Db::new(ATTENUATION_DB as f32)),
        )
        .minimum_phase();
        let h = lowpass.taps();
        let sum: f64 = h.iter().sum();
        // The centroid (group delay at DC) in table points, of the step
        // as the tables interpolate it (each tap's rise spread over the
        // point before it, hence the half), then the fractional shift
        // that makes it a whole number of samples.
        let centroid: f64 = h
            .iter()
            .enumerate()
            .map(|(i, &v)| i as f64 * v)
            .sum::<f64>()
            / sum
            - 0.5;
        let delay = (centroid / OVERSAMPLE as f64).ceil() as usize;
        let shift = (delay * OVERSAMPLE) as f64 - centroid;
        let whole = shift.floor() as usize;
        let frac = shift - whole as f64;
        let at = |i: usize| -> f64 { h.get(i).copied().unwrap_or(0.0) / sum };
        let len = taps.div_ceil(OVERSAMPLE) + delay + 1;
        let points = len * OVERSAMPLE + 1;
        let shifted = (0..points).map(|i| {
            if i < whole {
                0.0
            } else {
                let j = i - whole;
                (1.0 - frac) * at(j) + if j > 0 { frac * at(j - 1) } else { 0.0 }
            }
        });
        let mut step = Vec::with_capacity(points);
        let mut acc = 0.0;
        for v in shifted {
            acc += v;
            step.push(acc);
        }
        // The ramp: the step's integral (trapezoids), in samples.
        let mut ramp = Vec::with_capacity(points);
        let mut area = 0.0;
        let mut last = 0.0;
        for &s in &step {
            area += 0.5 * (s + last) / OVERSAMPLE as f64;
            last = s;
            ramp.push(area);
        }
        // Trim to where both residuals have settled below 1e-7.
        let settled = |i: usize| {
            let late = i as f64 / OVERSAMPLE as f64 - delay as f64;
            late >= 0.0 && (step[i] - 1.0).abs() < 1e-7 && (ramp[i] - late).abs() < 1e-7
        };
        let last = (0..points).rev().find(|&i| !settled(i)).unwrap_or(0);
        let len = (last / OVERSAMPLE + 2).min(len);
        step.truncate(len * OVERSAMPLE + 1);
        ramp.truncate(len * OVERSAMPLE + 1);
        (step, ramp, delay, len)
    }

    /// The tables.
    pub fn get() -> &'static Self {
        &Self::TABLES
    }

    /// `(step, ramp)` residuals at `tau ≥ 0` samples after the edge,
    /// against the naive step and ramp delayed by [`delay`](Self::delay).
    #[inline]
    pub fn residual(&self, tau: f64) -> (f64, f64) {
        let x = tau * OVERSAMPLE as f64;
        let i = x as usize;
        if i + 1 >= self.step.len() {
            return (0.0, 0.0);
        }
        let f = x - i as f64;
        let step = self.step[i] + f * (self.step[i + 1] - self.step[i]);
        let ramp = self.ramp[i] + f * (self.ramp[i + 1] - self.ramp[i]);
        let late = tau - self.delay as f64;
        if late >= 0.0 {
            (step - 1.0, ramp - late)
        } else {
            (step, ramp)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed tables are the design's (to rounding across
    /// platforms); `tests/minblep_tables.rs` regenerates them.
    #[test]
    #[cfg_attr(miri, ignore = "a minimum-phase FIR design: the native run covers it")]
    fn tables_match_their_design() {
        let (step, ramp, delay, len) = MinBlep::design();
        let t = MinBlep::get();
        assert_eq!((t.delay, t.len, t.step.len()), (delay, len, step.len()), "regenerate: STAVE_BLESS=1 cargo test -p stave-dsp --test minblep_tables");
        for (i, (a, b)) in t.step.iter().zip(&step).chain(t.ramp.iter().zip(&ramp)).enumerate() {
            assert!((a - b).abs() <= 1e-12 * (1.0 + b.abs()), "point {i}: {a} vs {b}; regenerate the tables");
        }
    }

    /// The residuals settle: by the table's end the band-limited step is
    /// 1 and the ramp has caught the delayed naive one.
    #[test]
    fn the_minblep_residuals_settle() {
        let t = MinBlep::get();
        let (s, r) = t.residual(t.len as f64 - 1.0);
        assert!(
            s.abs() < 1e-6 && r.abs() < 1e-6,
            "step {s:e} ramp {r:e}, delay {}",
            t.delay
        );
        // Causal: the band-limited step starts from nothing at the edge.
        let (start, _) = t.residual(0.0);
        assert!(start.abs() < 0.05, "step at the edge {start}");
    }

    /// polyBLAMP is the integral of polyBLEP's residual.
    #[test]
    fn polyblamp_integrates_polyblep() {
        let f = 0.3;
        let (b, a) = poly(f, 0.0, 1.0);
        assert!((b - poly_ramp(-f)).abs() < 1e-15 && (a - poly_ramp(1.0 - f)).abs() < 1e-15);
    }
}
