//! Nonlinear kernels from exponential sine sweeps, as an analysis (DSP.md
//! M6, characteristic C12): the harmonic responses one sweep separates
//! (Farina, AES 108, 2000), the generalised Hammerstein kernels Novak's
//! conversion gives from them (Novak, Simon, Kadlec & Lotton, IEEE TIM
//! 59(8), 2010; the synchronized sweep of Novak, Lotton & Simon, JAES
//! 63(10), 2015), and the diagonal of the low-order Volterra kernels from
//! sweeps at several levels. A compact, plottable picture of a model's or
//! a device's *weak* nonlinearity, to compare models, black boxes and
//! captures; [`hammerstein::fit`](super::hammerstein::fit) builds a runtime
//! model from the same separation.
//!
//! **Harmonic responses.** A synchronized sweep `x = a·sin φ(t)` has
//! `n·φ(t) = φ(t + L ln n)`, so the n-th harmonic the system makes is the
//! sweep itself, `L ln n` early. Deconvolved by the sweep's analytic
//! spectrum, the output holds one impulse response per harmonic;
//! windowed apart, [`HarmonicResponses`] holds each one's spectrum
//! `Hₙ(f)`, indexed by the *output* frequency `f` (the n-th harmonic of an
//! input at `f / n`). The level of harmonic `n` of a sine at `f₀` and the
//! sweep's amplitude is `|Hₙ(n·f₀)| / |H₁(f₀)|` re the fundamental.
//!
//! **Generalised Hammerstein kernels.** If the system is `Σₘ Gₘ * xᵐ`
//! (one filter per power), expanding `sinᵐ` ties the harmonic responses
//! to the branch filters by an upper-triangular matrix
//! ([`hammerstein::coefficient`](super::hammerstein::coefficient)):
//! `Hₙ(f) = Σₘ Aₙₘ(a)·Gₘ(f)`, solved from the highest order down. For a
//! system that *is* a generalised Hammerstein system, `Gₘ` is the same
//! at every level; how far it moves with level is Novak et al.'s (2010)
//! classification criterion ([`level_dependence`]).
//!
//! **Volterra diagonals.** A Volterra system's m-th kernel `Vₘ(f₁, …,
//! fₘ)` puts `a^{m−1}·(2j)^{1−m}·Vₘ(f/m, …, f/m)` into harmonic `m` at the
//! output frequency `f`; the higher odd (even) orders add terms in
//! `a^{m+1}, a^{m+3}, …` that hold their kernels *off* the diagonal. One
//! sweep cannot part them: [`Volterra::from_levels`] fits, per harmonic
//! and frequency, a polynomial in `a²` across sweeps at several levels
//! and keeps its constant term — the diagonal `Vₘ(f/m, …, f/m)` of the
//! pure m-th order kernel, for any Volterra system (a Hammerstein system's
//! is `Gₘ(f)`; a Wiener system's `c·L(f/m)ᵐ`). Indexed, like the others,
//! by the output frequency.
//!
//! # Valid ranges
//!
//! - **Weak nonlinearity.** The expansions hold for a system a truncated
//!   power (Volterra) series describes at the sweep's levels: below a
//!   knee. A hard clipper past its knee has no convergent low-order
//!   series; its kernels then describe that level alone, and
//!   [`level_dependence`] grows.
//! - **Time invariance.** A system that changes over the sweep (an
//!   envelope follower, bias that shifts after a burst, a modulated
//!   delay) smears each harmonic response over its neighbours.
//! - **Memory within the windows.** Each harmonic's window reaches
//!   halfway to its neighbours, `½·L·ln((n+1)/n)` seconds; a response
//!   longer than that leaks into the next harmonic. Size the sweep from
//!   the system's memory (≈0.6 s for order 5 at a 20 ms response).
//! - **Band.** Each harmonic's band starts at `n·f₁` (output frequency);
//!   cut there, its response rings for longer than its window and leaks
//!   into the low end of every kernel, scaled up by `a^{1−m}` in the
//!   higher ones. The analysis tapers the deconvolution to zero below
//!   `order·f₁` (a raised cosine in log frequency to `8·order·f₁`), so
//!   every edge rises smoothly, and divides the taper back out: kernels
//!   are measured from `2·order·f₁` (where it passes a quarter) to
//!   `0.9·f₂`. The leakage left is a floor in each kernel's
//!   *contribution* (its harmonic re the linear response) that falls
//!   steeply with frequency, at the same multiples of the band's start
//!   whatever `f₁` (measured on stave's circuits): about −80 dB just
//!   above it, −90 dB at 2×, −100 dB at 3× and −115 dB at 5×. Start the
//!   sweep well below the lowest frequency of interest.
//! - **The sweep's analytic spectrum.** Novak's deconvolution divides by
//!   the stationary-phase spectrum, which leaves a phase error of
//!   `1/(12·2πfL)` (7e-4 at 100 Hz for `L = 0.2 s`); the analysis adds the
//!   Stirling series' next terms (the exact spectrum is a Gamma
//!   function), so what remains is the windows' truncation.
//! - **Sweep level.** Too low and the higher kernels sit in numerical
//!   noise; too high and the series no longer converges. Measure at
//!   several levels and check [`level_dependence`].
//! - **Aliasing.** A system run at its host rate folds harmonics above
//!   Nyquist back into the band; the kernels then carry them. Measure
//!   through an oversampled region when the system aliases.
//!
//! # Verified
//!
//! `identify::kernels::tests`, sweeps from 5 Hz at three levels, each
//! system run 4× oversampled. A generalised Hammerstein system (three
//! one-pole branches): every `Gₘ` within 5e-3 of its peak from 100 Hz to
//! 18 kHz (measured 3.1e-3, `G₃` at the quietest level), the Volterra
//! diagonals within 1.5e-3 (5.4e-4), `G` moving across levels by under
//! 3e-3 (1.7e-3), and a prediction at another level within 1e-6 (1.1e-7).
//! A memoryless polynomial to the fifth power: every kernel flat at its
//! coefficient to 2e-3 above 1 kHz (6.6e-4, `G₅` at a = 0.25) and, as a
//! contribution re the linear response, to 2e-4 from 100 Hz (1.0e-4);
//! harmonic levels to 0.01 dB of the closed form (measured equal to
//! 1e-4 dB). A Wiener system (a one-pole into `x + 0.3x² − 0.2x³`): the
//! Volterra diagonals `c·L(f/m)ᵐ` within 3e-3 (1.1e-3), while its
//! Hammerstein `G₁` moves 3e-2 across the levels. `stave-test`'s
//! `tests/kernels.rs`: the harmonics the kernels read against the R17
//! characterisation's tones at the same level.
//!
//! ```
//! use stave_measure::identify::hammerstein::stimulus;
//! use stave_measure::identify::kernels::{Kernels, Volterra};
//! use stave_measure::suite::sweep::Sweep;
//!
//! // A memoryless polynomial at three levels: every kernel is flat at its
//! // coefficient, and the Volterra diagonals part the orders exactly.
//! let rate = 48_000.0;
//! let curve = |x: f64| x + 0.2 * x * x - 0.1 * x * x * x;
//! let levels: Vec<Kernels> = [0.1, 0.2, 0.4]
//!     .iter()
//!     .map(|&a| {
//!         let sweep = Sweep::synchronized(20.0, 4_000.0, 0.5, a);
//!         let output: Vec<f64> = stimulus(&sweep, rate, 0.01, 0.1).iter().map(|&x| curve(x)).collect();
//!         Kernels::measure(&sweep, rate, &output, 3).unwrap()
//!     })
//!     .collect();
//! let g2 = levels[0].hammerstein[1].at(1_000.0).unwrap();
//! assert!((g2.re - 0.2).abs() < 1e-3 && g2.im.abs() < 1e-3);
//! let v = Volterra::from_levels(&levels).unwrap();
//! let v3 = v.kernels[2].at(3_000.0).unwrap();
//! assert!((v3.re + 0.1).abs() < 1e-3, "{v3:?}");
//! ```

use std::f64::consts::{PI, TAU};

use super::hammerstein::coefficient;
use super::periodic::{spectrum, waveform};
use super::solve::least_squares;
use crate::suite::sweep::Sweep;
use crate::suite::{divide, Complex};

/// The harmonic responses one synchronized sweep separates, on a common
/// grid of bins `0, bin_hz, 2·bin_hz, …` up to Nyquist.
#[derive(Clone, Debug)]
pub struct HarmonicResponses {
    /// The sweep that was played (its amplitude is the level measured at).
    pub sweep: Sweep,
    pub rate: f64,
    /// The grid's spacing, Hz.
    pub bin_hz: f64,
    /// `Hₙ` for `n = 1 …`, on every bin (only [`valid`](Self::valid)
    /// bins are measurements).
    pub responses: Vec<Vec<Complex<f64>>>,
    /// The linear response's window after its arrival, samples: the
    /// longest causal memory the separation keeps.
    pub linear_window: usize,
    /// Where the low-edge taper passes a quarter (`2·order·f₁`), below
    /// which nothing is measured; `None` untapered.
    pub edge_hz: Option<f64>,
}

/// The phase the stationary-phase spectrum leaves out at `ω = 2πfL`:
/// the sweep's exact spectrum is `L·Γ(−jω)·(−j2πf₁L)^{jω}/(2j)` (the
/// substitution `u = e^{t/L}`), whose Stirling series past its leading
/// term, `1/(12s) − 1/(360s³) + 1/(1260s⁵)` at `s = −jω`, is the phase
/// `1/(12ω) + 1/(360ω³) + 1/(1260ω⁵)`: 7e-4 rad at 100 Hz for `L = 0.2 s`,
/// the error left by the leading term alone. (Below `ω ≈ 1`, under the
/// taper, the series diverges and is not used.)
fn stirling(w: f64) -> f64 {
    if w < 1.0 {
        return 0.0;
    }
    1.0 / (12.0 * w) + 1.0 / (360.0 * w.powi(3)) + 1.0 / (1260.0 * w.powi(5))
}

/// The taper's gain below which it is not divided back out (it passes a
/// quarter at [`HarmonicResponses::edge_hz`]).
const EDGE_FLOOR: f64 = 0.25;

impl HarmonicResponses {
    /// Separate `output`, the response at `rate` to
    /// [`stimulus`](super::hammerstein::stimulus)`(sweep, …)` (a power of
    /// two long), into harmonics `1 ..= order`. `None` when the sweep is
    /// not synchronized (`f₁·L` not whole: the harmonics' phases would not
    /// line up) or too short to give each harmonic 16 samples of window.
    pub fn separate(sweep: &Sweep, rate: f64, output: &[f64], order: usize) -> Option<Self> {
        Self::separate_with(sweep, rate, output, order, true)
    }

    /// [`separate`](Self::separate), with or without the low-edge taper
    /// (the M3 fit's separation has none, as it was fitted).
    pub(crate) fn separate_with(sweep: &Sweep, rate: f64, output: &[f64], order: usize, tapered: bool) -> Option<Self> {
        let n = output.len();
        assert!(n.is_power_of_two(), "pad the sweep to a power of two (stimulus())");
        let f1 = sweep.low_hz;
        let l = sweep.rate_constant();
        if (f1 * l - (f1 * l).round()).abs() > 1e-6 {
            return None;
        }
        let a = sweep.amplitude;
        let y = spectrum(output);
        let df = rate / n as f64;
        // Deconvolve by the sweep's analytic spectrum (Novak et al. 2015, by
        // stationary phase): X̃(f) = (a/2)·√(L/f)·e^{j(2πfL(1 − ln(f/f₁)) − π/4)},
        // times `rate` for the DFT. Dividing by it, not by the sampled sweep's
        // spectrum, needs no band edges: 1/X̃ falls as √f towards DC, so the
        // even branches' DC and whatever lies below f₁ fade instead of
        // ringing through every window (a taper's ringing, cut by a window,
        // leaves an error falling only as f₁/f).
        let order = order.max(1);
        // The analysis tapers the deconvolution's low edge (module docs):
        // zero below `order·f₁`, where every harmonic's own band starts, a
        // raised cosine in log frequency up to 8× that.
        let edge = tapered.then_some((order as f64 * f1, 8.0 * order as f64 * f1));
        let gain = |f: f64| match edge {
            None => 1.0,
            Some((lo, _)) if f <= lo => 0.0,
            Some((lo, hi)) if f < hi => 0.5 - 0.5 * (PI * (f / lo).ln() / (hi / lo).ln()).cos(),
            Some(_) => 1.0,
        };
        let mut d = vec![Complex::new(0.0, 0.0); n];
        for k in 1..n / 2 {
            let f = k as f64 * df;
            let mut phase = TAU * f * l * (1.0 - (f / f1).ln()) - std::f64::consts::FRAC_PI_4;
            if tapered {
                phase += stirling(TAU * f * l);
            }
            let mag = 0.5 * a * (l / f).sqrt() * rate;
            let xt = Complex::new(mag * phase.cos(), mag * phase.sin());
            d[k] = divide(y[k], xt).scale(gain(f));
            d[n - k] = d[k].conj();
        }
        let ir = waveform(&d);
        // Each harmonic's window reaches halfway to its neighbours (harmonic
        // n arrives at −L·ln n): back towards n + 1 and on towards n − 1
        // (the linear response, with no neighbour ahead, as far on as
        // harmonic 2 is behind it). A band-limited response rings both ways
        // of its arrival; a window cut short before it leaves an error
        // falling only as 1/f, so the reach back matters as much as on.
        let gap = |a: usize, b: usize| l * (b as f64 / a as f64).ln() * rate;
        let back: Vec<usize> = (1..=order).map(|h| (0.5 * gap(h, h + 1)).floor() as usize).collect();
        let on: Vec<usize> = (1..=order)
            .map(|h| if h == 1 { gap(1, 2).floor() as usize } else { (0.5 * gap(h - 1, h)).floor() as usize })
            .collect();
        if back.iter().chain(&on).any(|&w| w < 16) {
            return None;
        }
        let widest = (0..order).map(|i| back[i] + on[i]).max()?;
        let nfft = (2 * widest).next_power_of_two();
        let mut responses: Vec<Vec<Complex<f64>>> = Vec::with_capacity(order);
        for h in 1..=order {
            let (pre, post) = (back[h - 1], on[h - 1]);
            let p = -(l * (h as f64).ln() * rate);
            let start = p.floor() as i64 - pre as i64;
            let offset = p - start as f64;
            let mut seg = vec![0.0; nfft];
            let (fade_in, fade_out) = (pre / 2, post / 4);
            for (i, s) in seg.iter_mut().enumerate().take(pre + post) {
                let at = (start + i as i64).rem_euclid(n as i64) as usize;
                let w = if i < fade_in {
                    0.5 - 0.5 * (PI * i as f64 / fade_in as f64).cos()
                } else if i >= pre + post - fade_out {
                    let j = i - (pre + post - fade_out);
                    0.5 + 0.5 * (PI * j as f64 / fade_out as f64).cos()
                } else {
                    1.0
                };
                *s = ir[at] * w;
            }
            let s = spectrum(&seg);
            responses.push(
                (0..=nfft / 2)
                    .map(|k| {
                        let w = TAU * k as f64 / nfft as f64;
                        let shift = Complex::new((w * offset).cos(), (w * offset).sin());
                        // The taper divided back out where it passes enough.
                        let g = gain(k as f64 * rate / nfft as f64);
                        (s[k] * shift).scale(if g >= EDGE_FLOOR { 1.0 / g } else { 1.0 })
                    })
                    .collect(),
            );
        }
        Some(Self {
            sweep: *sweep,
            rate,
            bin_hz: rate / nfft as f64,
            responses,
            linear_window: on[0],
            edge_hz: edge.map(|(lo, _)| 2.0 * lo),
        })
    }

    /// The highest harmonic separated.
    pub fn order(&self) -> usize {
        self.responses.len()
    }

    /// Whether bin `k` of kernel `m` is measured: output frequencies
    /// from `2·m·f₁` (and the taper's [`edge_hz`](Self::edge_hz)) to
    /// `0.9·f₂` (`f₂` at most Nyquist).
    pub fn valid(&self, m: usize, k: usize) -> bool {
        let f = k as f64 * self.bin_hz;
        let low = (2.0 * self.sweep.low_hz * m as f64).max(self.edge_hz.unwrap_or(0.0));
        f >= low && f <= 0.9 * self.sweep.high_hz.min(0.5 * self.rate)
    }

    /// Novak's conversion: the generalised Hammerstein kernels `Gₘ` on
    /// every bin, from the highest order down. Below its band (output
    /// frequencies under `2·m·f₁`, from inputs the sweep never played) a
    /// branch is unmeasured, yet the lower harmonics there still hold its
    /// contribution (`sin³` has a fundamental): it is held at its lowest
    /// measured value, so the lower branches are not corrupted by its
    /// absence.
    pub fn hammerstein(&self) -> Vec<Vec<Complex<f64>>> {
        let order = self.order();
        let a = self.sweep.amplitude;
        let bins = self.responses[0].len();
        let mut g = vec![vec![Complex::new(0.0, 0.0); bins]; order];
        for m in (1..=order).rev() {
            for k in 0..bins {
                if !self.valid(m, k) {
                    continue;
                }
                let mut rhs = self.responses[m - 1][k];
                for q in m + 1..=order {
                    rhs = rhs - coefficient(m, q, a) * g[q - 1][k];
                }
                g[m - 1][k] = divide(rhs, coefficient(m, m, a));
            }
            if let Some(first) = (0..bins).find(|&k| self.valid(m, k)) {
                let held = g[m - 1][first];
                for v in &mut g[m - 1][..first] {
                    *v = held;
                }
            }
        }
        g
    }

    /// Kernel `m`'s valid bins of `grid` (one row of
    /// [`responses`](Self::responses) or [`hammerstein`](Self::hammerstein)).
    fn kernel(&self, m: usize, grid: &[Complex<f64>]) -> Kernel {
        Kernel {
            order: m,
            points: (1..grid.len())
                .filter(|&k| self.valid(m, k))
                .map(|k| (k as f64 * self.bin_hz, grid[k]))
                .collect(),
        }
    }
}

/// One kernel's frequency response, on `(Hz, value)` points ascending in
/// output frequency over its measured band.
#[derive(Clone, Debug, PartialEq)]
pub struct Kernel {
    /// Its order (1 is the linear response).
    pub order: usize,
    pub points: Vec<(f64, Complex<f64>)>,
}

impl Kernel {
    /// The response at `hz`, linear between points; `None` outside the band.
    pub fn at(&self, hz: f64) -> Option<Complex<f64>> {
        let i = self.points.partition_point(|p| p.0 < hz);
        if i == self.points.len() {
            return None;
        }
        let (f1, v1) = self.points[i];
        if f1 == hz {
            return Some(v1);
        }
        let (f0, v0) = *self.points.get(i.checked_sub(1)?)?;
        let t = (hz - f0) / (f1 - f0);
        Some(v0.scale(1.0 - t) + v1.scale(t))
    }

    /// The largest magnitude over the band.
    pub fn peak(&self) -> f64 {
        self.points.iter().fold(0.0, |p, (_, v)| p.max(v.norm()))
    }

    /// The response at each of `hz` inside the band (for reports: a few
    /// points per octave instead of every bin).
    pub fn sampled(&self, hz: &[f64]) -> Vec<(f64, Complex<f64>)> {
        hz.iter().filter_map(|&f| Some((f, self.at(f)?))).collect()
    }
}

/// The kernels one sweep measures: its harmonic responses and the
/// generalised Hammerstein kernels they give, at the sweep's level.
#[derive(Clone, Debug)]
pub struct Kernels {
    pub sweep: Sweep,
    pub rate: f64,
    /// Farina's harmonic responses `Hₙ`, `n = 1 …` (index `n − 1`).
    pub harmonics: Vec<Kernel>,
    /// Novak's generalised Hammerstein kernels `Gₘ`, `m = 1 …`.
    pub hammerstein: Vec<Kernel>,
}

impl Kernels {
    /// The kernels to `order` of `output`, the response at `rate` to
    /// [`stimulus`](super::hammerstein::stimulus)`(sweep, …)`. `None` as
    /// for [`HarmonicResponses::separate`].
    pub fn measure(sweep: &Sweep, rate: f64, output: &[f64], order: usize) -> Option<Self> {
        Some(Self::of(&HarmonicResponses::separate(sweep, rate, output, order)?))
    }

    /// The kernels of separated harmonic responses.
    pub fn of(h: &HarmonicResponses) -> Self {
        let g = h.hammerstein();
        Self {
            sweep: h.sweep,
            rate: h.rate,
            harmonics: (1..=h.order()).map(|n| h.kernel(n, &h.responses[n - 1])).collect(),
            hammerstein: (1..=h.order()).map(|m| h.kernel(m, &g[m - 1])).collect(),
        }
    }

    /// The level measured at: the sweep's peak.
    pub fn amplitude(&self) -> f64 {
        self.sweep.amplitude
    }

    pub fn order(&self) -> usize {
        self.harmonics.len()
    }

    /// Harmonic `n` of a sine at `f0` Hz and the sweep's amplitude, dB re
    /// its fundamental, as measured: `|Hₙ(n·f₀)| / |H₁(f₀)|`.
    pub fn harmonic_dbc(&self, n: usize, f0: f64) -> Option<f64> {
        let h = self.harmonics.get(n.checked_sub(1)?)?.at(n as f64 * f0)?;
        let f = self.harmonics[0].at(f0)?;
        Some(20.0 * (h.norm() / f.norm()).log10())
    }

    /// The harmonic responses the Hammerstein kernels give at another
    /// amplitude: `Hₙ(n·f₀) = Σₘ Aₙₘ(a)·Gₘ(n·f₀)` for `n = 1 … order`, so
    /// harmonic `n` of `amplitude·sin(2π f₀ t)` is `amplitude·Hₙ`. `None`
    /// where a kernel is unmeasured. Exact for a generalised Hammerstein
    /// system; for another, it holds near the sweep's level.
    pub fn predict(&self, f0: f64, amplitude: f64) -> Vec<Option<Complex<f64>>> {
        let order = self.order();
        (1..=order)
            .map(|n| {
                let f = n as f64 * f0;
                (n..=order).step_by(2).try_fold(Complex::new(0.0, 0.0), |acc, m| {
                    Some(acc + coefficient(n, m, amplitude) * self.hammerstein[m - 1].at(f)?)
                })
            })
            .collect()
    }
}

/// How far the generalised Hammerstein kernel `Gₘ` moves across sweeps at
/// several levels (Novak, Simon & Lotton 2010's classification): the
/// largest `|Gₘ,ℓ(f) − Ḡₘ(f)|` over `band` (Hz, output frequency) and the
/// levels, re the peak of the mean `Ḡₘ` there. Near 0 for a generalised
/// Hammerstein system, down to the quietest level's measurement error
/// (which grows as `a^{1−m}` at the band's low end); larger where the
/// system's filtering moves with level (a Wiener system, a clipper past
/// its knee). `None` for fewer than two levels, mismatched grids or an
/// unmeasured order.
pub fn level_dependence(levels: &[Kernels], m: usize, band: (f64, f64)) -> Option<f64> {
    let kernels: Vec<&Kernel> = levels.iter().map(|k| k.hammerstein.get(m.checked_sub(1)?)).collect::<Option<_>>()?;
    if kernels.len() < 2 || !same_grid(&kernels) {
        return None;
    }
    let n = kernels.len() as f64;
    let inside: Vec<usize> = (0..kernels[0].points.len())
        .filter(|&i| (band.0..=band.1).contains(&kernels[0].points[i].0))
        .collect();
    let mean: Vec<Complex<f64>> = inside
        .iter()
        .map(|&i| kernels.iter().fold(Complex::new(0.0, 0.0), |a, k| a + k.points[i].1).scale(1.0 / n))
        .collect();
    let peak = mean.iter().fold(0.0f64, |p, v| p.max(v.norm()));
    if peak == 0.0 {
        return (!inside.is_empty()).then_some(0.0);
    }
    let worst = kernels
        .iter()
        .flat_map(|k| inside.iter().zip(&mean).map(|(&i, m)| (k.points[i].1 - *m).norm()))
        .fold(0.0, f64::max);
    Some(worst / peak)
}

fn same_grid(kernels: &[&Kernel]) -> bool {
    kernels
        .iter()
        .all(|k| k.points.len() == kernels[0].points.len() && k.points.iter().zip(&kernels[0].points).all(|(a, b)| a.0 == b.0))
}

/// The diagonals of a system's low-order Volterra kernels, from sweeps at
/// several levels (module docs): `Vₘ(f/m, …, f/m)` per output frequency.
#[derive(Clone, Debug)]
pub struct Volterra {
    /// `Vₘ` for `m = 1 …` (index `m − 1`).
    pub kernels: Vec<Kernel>,
    /// The sweeps' amplitudes, ascending.
    pub amplitudes: Vec<f64>,
    /// How many powers of `a²` each order's fit held (index `m − 1`): the
    /// order itself and the next ones of its parity up to the sweeps'
    /// order, at most one per level.
    pub terms: Vec<usize>,
    /// Whether each order's fit held every higher order the sweeps
    /// separated (`terms` reached them): if not, the orders above were
    /// assumed absent from that harmonic.
    pub complete: Vec<bool>,
}

impl Volterra {
    /// The Volterra diagonals from `levels`, one [`Kernels`] per sweep:
    /// the same sweep (band, length, rate, order) at different
    /// amplitudes. Per harmonic `n` and frequency, `Hₙ / aⁿ⁻¹` is fitted
    /// by least squares (each level weighted by `aⁿ⁻¹`, so in the
    /// harmonic's own units) as a polynomial in `a²` with as many terms as the
    /// orders `n, n + 2, …` the sweeps separated, at most one per level;
    /// its constant term over `(2j)^{1−n}` is `Vₙ`. One level gives
    /// Farina's (2001) single-level diagonal, which carries the higher
    /// orders' contributions; three levels part orders up to 5 in every
    /// harmonic. `None` for mismatched sweeps or a fit that is
    /// rank-deficient (two equal levels).
    pub fn from_levels(levels: &[Kernels]) -> Option<Self> {
        let first = levels.first()?;
        let order = first.order();
        let same = |k: &Kernels| {
            k.order() == order
                && k.rate == first.rate
                && k.sweep.low_hz == first.sweep.low_hz
                && k.sweep.high_hz == first.sweep.high_hz
                && k.sweep.seconds == first.sweep.seconds
        };
        if !levels.iter().all(same) {
            return None;
        }
        let mut sorted: Vec<&Kernels> = levels.iter().collect();
        sorted.sort_by(|a, b| a.amplitude().total_cmp(&b.amplitude()));
        let amplitudes: Vec<f64> = sorted.iter().map(|k| k.amplitude()).collect();
        // Powers of a² scaled by the largest level: the columns stay O(1).
        let top = amplitudes[amplitudes.len() - 1];
        let mut kernels = Vec::with_capacity(order);
        let mut terms = Vec::with_capacity(order);
        let mut complete = Vec::with_capacity(order);
        for n in 1..=order {
            let wanted = (order - n) / 2 + 1;
            let k_terms = wanted.min(sorted.len());
            let rows: Vec<&Kernel> = sorted.iter().map(|k| &k.harmonics[n - 1]).collect();
            if !same_grid(&rows) {
                return None;
            }
            // Each row weighted by (a/top)^{n−1}, back to the harmonic's own
            // units: its measurement error is about the same at every level,
            // so the quiet levels (divided by a^{n−1}) do not dominate.
            let weight = |a: f64| (a / top).powi(n as i32 - 1);
            let design: Vec<f64> = amplitudes
                .iter()
                .flat_map(|&a| (0..k_terms).map(move |t| (a / top).powi(2 * t as i32) * weight(a)))
                .collect();
            // (2j)^{n−1}: undo the expansion's (2j)^{1−n}.
            let unwind = coefficient(n, n, 1.0);
            let mut points = Vec::with_capacity(rows[0].points.len());
            for i in 0..rows[0].points.len() {
                let scaled: Vec<Complex<f64>> = rows
                    .iter()
                    .zip(&amplitudes)
                    .map(|(r, &a)| r.points[i].1.scale(a.powi(1 - n as i32) * weight(a)))
                    .collect();
                let re = least_squares(design.clone(), sorted.len(), k_terms, scaled.iter().map(|v| v.re).collect())?;
                let im = least_squares(design.clone(), sorted.len(), k_terms, scaled.iter().map(|v| v.im).collect())?;
                points.push((rows[0].points[i].0, divide(Complex::new(re[0], im[0]), unwind)));
            }
            kernels.push(Kernel { order: n, points });
            terms.push(k_terms);
            complete.push(k_terms == wanted);
        }
        Some(Self {
            kernels,
            amplitudes,
            terms,
            complete,
        })
    }

    /// Harmonic `n` of a sine at `f0` and `amplitude`, dB re its
    /// fundamental, from the diagonals' lowest-order terms alone
    /// (`|a^{n−1}·2^{1−n}·Vₙ(n·f₀)| / |V₁(f₀)|`): the weak-signal limit,
    /// where each harmonic is its own order's.
    pub fn harmonic_dbc(&self, n: usize, f0: f64, amplitude: f64) -> Option<f64> {
        let v = self.kernels.get(n.checked_sub(1)?)?.at(n as f64 * f0)?;
        let f = self.kernels[0].at(f0)?;
        let h = v.norm() * (0.5 * amplitude).powi(n as i32 - 1);
        Some(20.0 * (h / f.norm()).log10())
    }
}

#[cfg(test)]
mod tests;
