//! C13 (DSP.md): program material through a model and its reference,
//! judged as a null test — the residual's level, and its noise-to-mask
//! ratio under the reference (BS.1387, `stave_measure::perceptual`), so a
//! difference reads in audibility terms, not as a number that happened
//! to fit. This is the measurement R16 restates budgets in; it takes any
//! two runs over the same input, so a capture, an oversampled run or
//! another solver can be the reference.

use stave_measure::perceptual::nmr::{Nmr, NmrReport};
use stave_measure::perceptual::null::{self, Null};
use stave_measure::perceptual::Playback;

/// A null test on program material.
#[derive(Clone, Debug)]
pub struct NullNmr {
    /// The residual's power re the reference's, dB (−∞ toward a perfect
    /// null).
    pub residual_db: f64,
    /// The largest single-sample difference re the reference's peak, dB.
    pub peak_db: f64,
    /// The residual's NMR under the reference, played with the
    /// reference's peak at 92 dB SPL.
    pub nmr: NmrReport,
    /// R16's verdict: the same null after matching the test to the
    /// reference in level and latency (±1 ms), at the same playback
    /// ([`null::between`]); inaudible at ≤ −10 dB total and worst band.
    pub aligned: Null,
}

impl NullNmr {
    /// `test` against `reference` (time-aligned, at `rate`; the common
    /// length is compared).
    pub fn of(reference: &[f64], test: &[f64], rate: f64) -> Self {
        let n = reference.len().min(test.len());
        let (r, t) = (&reference[..n], &test[..n]);
        let residual: Vec<f64> = r.iter().zip(t).map(|(a, b)| b - a).collect();
        let power = |x: &[f64]| x.iter().map(|v| v * v).sum::<f64>() / n.max(1) as f64;
        let peak = |x: &[f64]| x.iter().fold(0.0f64, |m, v| m.max(v.abs()));
        let full = peak(r).max(1e-300);
        Self {
            residual_db: 10.0 * (power(&residual) / power(r).max(1e-300)).log10(),
            peak_db: 20.0 * (peak(&residual) / full).max(1e-300).log10(),
            nmr: Nmr::new(Playback::full_scale(full)).of_noise(r, &residual, rate),
            aligned: null::between(r, t, rate, Playback::full_scale(full), 1e-3),
        }
    }

    /// The residual is masked: its total NMR at or below `threshold_db`.
    pub fn masked(&self, threshold_db: f64) -> bool {
        self.nmr.masked(threshold_db)
    }

    /// R16's verdict: the aligned null within the inaudible budget.
    pub fn inaudible(&self) -> bool {
        self.aligned.inaudible()
    }

    pub fn describe(&self) -> String {
        let a = &self.aligned;
        format!(
            "residual {:.1} dB (peak {:.1} dB), NMR total {:.1} dB, worst band {:.1} dB at {:.0} Hz, {:.0}% of frames disturbed; \
             aligned ({:+.3} dB, {:+.2} µs): {} at {:.1} dB total, {:.1} dB worst band",
            self.residual_db,
            self.peak_db,
            self.nmr.total_db,
            self.nmr.max_db,
            self.nmr.max_hz,
            100.0 * self.nmr.disturbed,
            a.alignment.gain_db(),
            a.alignment.delay * 1e6,
            null::verdict(a.nmr.total_db, a.nmr.max_db),
            a.nmr.total_db,
            a.nmr.max_db
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn programme(n: usize) -> Vec<f64> {
        // Decaying plucks of a few partials: a stand-in for a DI.
        (0..n)
            .map(|i| {
                let t = (i % 24_000) as f64 / 48_000.0;
                let env = (-6.0 * t).exp();
                (1..6).map(|k| (std::f64::consts::TAU * 110.0 * k as f64 * t).sin() / k as f64).sum::<f64>() * 0.3 * env
            })
            .collect()
    }

    #[test]
    fn a_null_reads_by_the_size_of_its_residual() {
        let x = programme(96_000);
        let same = NullNmr::of(&x, &x, 48_000.0);
        assert!(same.residual_db < -200.0 && same.masked(-60.0), "{}", same.describe());
        // −60 dB of gain error: well under the mask.
        let quiet: Vec<f64> = x.iter().map(|v| v * (1.0 + 1e-3)).collect();
        let q = NullNmr::of(&x, &quiet, 48_000.0);
        assert!((q.residual_db + 60.0).abs() < 0.1, "{}", q.describe());
        assert!(q.masked(-10.0), "{}", q.describe());
        // Aligned, the gain error is gone; a delay of 5 samples too.
        assert!((q.aligned.alignment.gain_db() + 20.0 * (1.0f64 + 1e-3).log10()).abs() < 1e-6 && q.inaudible(), "{}", q.describe());
        let late: Vec<f64> = (0..x.len()).map(|i| if i < 5 { 0.0 } else { x[i - 5] }).collect();
        let l = NullNmr::of(&x, &late, 48_000.0);
        assert!((l.aligned.alignment.delay * 48_000.0 + 5.0).abs() < 1e-3 && l.inaudible(), "{}", l.describe());
        // A hard clip at a third of the peak: audible.
        let clipped: Vec<f64> = x.iter().map(|v| v.clamp(-0.1, 0.1)).collect();
        let c = NullNmr::of(&x, &clipped, 48_000.0);
        assert!(!c.masked(0.0), "{}", c.describe());
        assert!(c.nmr.total_db > q.nmr.total_db + 40.0);
    }
}
