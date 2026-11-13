//! The YIN period detector (de Cheveigné & Kawahara, *YIN, a fundamental
//! frequency estimator for speech and music*, JASA 2002), its
//! difference function computed by FFT correlation so a detection costs
//! three transforms rather than `W·τ_max` products.

use crate::primitives::spectral::{Complex, Fft};

/// YIN's absolute threshold on the cumulative-mean-normalised
/// difference (step 4).
const THRESHOLD: f32 = 0.15;
/// Above this aperiodicity at the chosen lag, the frame is unvoiced.
const UNVOICED: f32 = 0.3;

#[derive(Clone, Debug)]
pub(super) struct Yin {
    /// The integration window `W`.
    window: usize,
    min_lag: usize,
    max_lag: usize,
    fft: Fft<f32>,
    a: Vec<f32>,
    b: Vec<f32>,
    fa: Vec<Complex<f32>>,
    fb: Vec<Complex<f32>>,
    r: Vec<f32>,
    /// Prefix sums of the squared segment.
    energy: Vec<f64>,
    d: Vec<f32>,
    /// `d'`, the cumulative-mean-normalised difference.
    dn: Vec<f32>,
}

impl Yin {
    /// A detector for periods from `min_lag` to `max_lag` samples over a
    /// window of `max_lag` samples: it reads segments of
    /// [`segment`](Self::segment) samples.
    pub(super) fn new(min_lag: usize, max_lag: usize) -> Self {
        let min_lag = min_lag.max(2);
        let max_lag = max_lag.max(min_lag + 2);
        let window = max_lag;
        let len = (window + max_lag + 1).next_power_of_two();
        Self {
            window,
            min_lag,
            max_lag,
            fft: Fft::new(len),
            a: vec![0.0; len],
            b: vec![0.0; len],
            fa: vec![Complex::default(); len / 2 + 1],
            fb: vec![Complex::default(); len / 2 + 1],
            r: vec![0.0; len],
            energy: vec![0.0; window + max_lag + 2],
            d: vec![0.0; max_lag + 2],
            dn: vec![0.0; max_lag + 2],
        }
    }

    /// The samples a detection reads: `W + τ_max + 1`.
    pub(super) fn segment(&self) -> usize {
        self.window + self.max_lag + 1
    }

    /// The period of `x` (the last [`segment`](Self::segment) samples,
    /// oldest first, written by `fill`) in samples, refined between lags
    /// by a parabola, or `None` when it is unvoiced or silent.
    /// Realtime: never allocates.
    pub(super) fn period(&mut self, fill: impl FnOnce(&mut [f32])) -> Option<f64> {
        let seg = self.segment();
        fill(&mut self.b[..seg]);
        self.b[seg..].fill(0.0);
        let w = self.window;
        self.a[..w].copy_from_slice(&self.b[..w]);
        self.a[w..].fill(0.0);
        self.energy[0] = 0.0;
        for i in 0..seg {
            self.energy[i + 1] = self.energy[i] + f64::from(self.b[i]) * f64::from(self.b[i]);
        }
        let ea = self.energy[w];
        if ea < 1e-10 * w as f64 {
            return None;
        }
        // r(τ) = Σ_{j<W} a_j·b_{j+τ}: conj(A)·B, inverse.
        self.fft.forward_real(&self.a, &mut self.fa);
        self.fft.forward_real(&self.b, &mut self.fb);
        for (x, y) in self.fb.iter_mut().zip(&self.fa) {
            *x = y.conj() * *x;
        }
        self.fft.inverse_real(&mut self.fb, &mut self.r);
        let scale = 1.0 / self.r.len() as f64;
        // d(τ) and its cumulative-mean normalisation d'(τ).
        self.d[0] = 0.0;
        self.dn[0] = 1.0;
        let mut sum = 0.0f64;
        for tau in 1..=self.max_lag {
            let e = self.energy[tau + w] - self.energy[tau];
            let d = (ea + e - 2.0 * f64::from(self.r[tau]) * scale).max(0.0);
            self.d[tau] = d as f32;
            sum += d;
            self.dn[tau] = if sum > 0.0 { (d * tau as f64 / sum) as f32 } else { 1.0 };
        }
        // The first dip under the threshold, walked down to its floor;
        // else the global minimum, if periodic enough.
        let range = self.min_lag..=self.max_lag;
        let tau = match range.clone().find(|&t| self.dn[t] < THRESHOLD) {
            Some(mut t) => {
                while t < self.max_lag && self.dn[t + 1] < self.dn[t] {
                    t += 1;
                }
                t
            }
            None => {
                let t = range.min_by(|&a, &b| self.dn[a].total_cmp(&self.dn[b]))?;
                if self.dn[t] >= UNVOICED {
                    return None;
                }
                t
            }
        };
        // A parabola through the raw difference around the dip.
        let tau = tau.clamp(1, self.max_lag - 1);
        let (l, c, r) = (
            f64::from(self.d[tau - 1]),
            f64::from(self.d[tau]),
            f64::from(self.d[tau + 1]),
        );
        let den = l - 2.0 * c + r;
        let shift = if den > 0.0 { (0.5 * (l - r) / den).clamp(-0.5, 0.5) } else { 0.0 };
        Some(tau as f64 + shift)
    }
}
