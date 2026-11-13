//! Fitting a Wiener–Hammerstein model to periodic records by the best
//! split of the best linear approximation (Sjöberg, Lauwers & Schoukens,
//! "Identification of Wiener–Hammerstein models: two algorithms based on
//! the best split of a linear model applied to the SYSID'09 benchmark
//! problem", Control Engineering Practice 20(11), 2012; the BLA's role
//! from Schoukens & Ljung 2019).
//!
//! 1. The best linear approximation (BLA) from random-phase multisines
//!    at one level: for a Wiener–Hammerstein system it is `H·G` times a
//!    gain (Bussgang), so its poles and zeros are both filters'.
//! 2. A rational fit of it ([`Rational::fit_order`]).
//! 3. Every split of its poles and zeros (real roots alone, complex ones
//!    with their conjugates) between `H` and `G`; for each, the curve
//!    that fits best is linear least squares — `y = G(Σ cⱼ·φⱼ(H(x))) +
//!    d` is linear in the spline's knot values `cⱼ` and the offset `d` —
//!    solved in the frequency domain over the band (each period is
//!    periodic, so filtering is exact there). Every split is ranked with
//!    a coarse spline, the best few refitted with the full one.
//! 4. How much the BLA's *shape* moves with level, which a
//!    Wiener–Hammerstein system cannot do (only its gain moves): the
//!    measure of where the model will fall short.

use stave_dsp::learned::block::{spline_weights, Linear, Nonlinearity, WienerHammerstein};

use super::periodic::{odd_bins, spectrum, waveform, Records};
use super::rational::{factors, Factor, Point, Rational};
use super::solve::Normal;
use crate::suite::{divide, Complex};

/// Which block structure to fit.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Structure {
    /// `G(f(H(x)))`: every split of the BLA's roots.
    WienerHammerstein,
    /// `f(H(x))`: every root before the curve.
    Wiener,
    /// `G(f(x))`: every root after it.
    Hammerstein,
}

/// The fit's settings.
#[derive(Clone, Debug)]
pub struct Options {
    pub structure: Structure,
    /// The highest BLA order tried (as many zeros as poles).
    pub max_order: usize,
    /// An order is kept when its worst BLA error is within this of the
    /// best order's, dB.
    pub tolerance_db: f64,
    /// Spline knots of the final curve.
    pub knots: usize,
    /// Spline knots while ranking splits.
    pub rank_knots: usize,
    /// Splits refitted with the full spline.
    pub refine: usize,
    /// The level (input RMS) the BLA is read at: the lowest by default.
    pub bla_rms: Option<f64>,
    /// The band the fit is judged over, Hz (default: the records' band).
    pub band: Option<(f64, f64)>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            structure: Structure::WienerHammerstein,
            max_order: 5,
            tolerance_db: 0.05,
            knots: 24,
            rank_knots: 10,
            refine: 4,
            bla_rms: None,
            band: None,
        }
    }
}

/// How the BLA's shape at one level differs from the reference level's
/// after the mean complex gain is taken out, third octave by third
/// octave, beyond twice its standard error over the realisations: what
/// a Wiener–Hammerstein model cannot follow.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Shape {
    pub rms: f64,
    /// The gain between this level's BLA and the reference's, dB.
    pub gain_db: f64,
    /// The worst magnitude deviation left, dB, and where.
    pub db: f64,
    pub at_hz: f64,
    /// The worst phase deviation left, degrees.
    pub degrees: f64,
}

/// A fitted model and how well it fits.
#[derive(Clone, Debug)]
pub struct Fit {
    pub model: WienerHammerstein,
    /// The BLA's rational fit and its worst error, dB.
    pub bla: Rational,
    pub bla_error_db: f64,
    /// Which roots went where.
    pub split: String,
    pub splits_tried: usize,
    /// Per level (input RMS): the in-band output error, dB re the
    /// output's in-band power (DC excluded), over that level's records.
    pub error_db: Vec<(f64, f64)>,
    /// The BLA's shape against level ([`Shape`]).
    pub shapes: Vec<Shape>,
    /// The fitted curve never turns back.
    pub monotone: bool,
}

/// The BLA at `rms`: the average of `Y/X` over the level's records on
/// the excited bins, as `(bin, value)`.
pub fn bla(records: &Records, rms: f64) -> Vec<(usize, Complex<f64>)> {
    let mut sum: Vec<(usize, Complex<f64>)> = Vec::new();
    let mut count = 0.0;
    for r in records.at(rms) {
        let n = r.input.len();
        let bins = odd_bins(n, records.rate, records.low_hz, records.high_hz);
        let (x, y) = (spectrum(&r.input), spectrum(&r.output));
        if sum.is_empty() {
            sum = bins.iter().map(|&k| (k, Complex::new(0.0, 0.0))).collect();
        }
        for (k, s) in sum.iter_mut() {
            *s = *s + divide(y[*k], x[*k]);
        }
        count += 1.0;
    }
    sum.into_iter().map(|(k, s)| (k, s.scale(1.0 / count))).collect()
}

/// The BLA's shape at every level against the one at `reference`.
pub fn shapes(records: &Records, reference: f64) -> Vec<Shape> {
    let base = bla(records, reference);
    let len = records.records[0].input.len() as f64;
    let hz = |k: usize| k as f64 * records.rate / len;
    // Third-octave bands over the excited bins.
    let band_of = |k: usize| (3.0 * (hz(k) / records.low_hz).log2()).floor() as i64;
    records
        .levels()
        .into_iter()
        .map(|rms| {
            // Each realisation's ratio to the reference BLA, bin by bin.
            let per: Vec<Vec<Complex<f64>>> = records
                .at(rms)
                .map(|r| {
                    let (x, y) = (spectrum(&r.input), spectrum(&r.output));
                    base.iter().map(|(k, b)| divide(divide(y[*k], x[*k]), *b)).collect()
                })
                .collect();
            let count = per.len() as f64;
            let all = per.iter().flatten().fold(Complex::new(0.0, 0.0), |s, r| s + *r);
            let gain = all.scale(1.0 / (count * base.len() as f64));
            // Per band and realisation, the mean ratio over the band's
            // bins; then the mean over realisations and its standard
            // error. The stochastic distortion on the excited bins varies
            // from realisation to realisation; a corner that moves with
            // level does not. What is reported is the deviation beyond
            // twice the standard error.
            let mut bands: Vec<(i64, Vec<Complex<f64>>, f64)> = Vec::new();
            for (i, (k, _)) in base.iter().enumerate() {
                let b = band_of(*k);
                if bands.last().map(|x| x.0) != Some(b) {
                    bands.push((b, vec![Complex::new(0.0, 0.0); per.len()], 0.0));
                }
                let last = bands.last_mut().expect("pushed");
                for (r, p) in per.iter().enumerate() {
                    last.1[r] = last.1[r] + divide(p[i], gain);
                }
                last.2 += 1.0;
            }
            let (mut db, mut at, mut deg) = (0.0f64, 0.0, 0.0f64);
            for (band, sums, bins) in bands {
                let means: Vec<Complex<f64>> = sums.iter().map(|s| s.scale(1.0 / bins)).collect();
                let m = means.iter().fold(Complex::new(0.0, 0.0), |s, v| s + *v).scale(1.0 / count);
                let se = if means.len() > 1 {
                    (means.iter().map(|v| (*v - m).norm_sqr()).sum::<f64>() / (count - 1.0) / count).sqrt()
                } else {
                    0.0
                };
                let excess = ((m - Complex::new(1.0, 0.0)).norm() - 2.0 * se).max(0.0);
                if excess == 0.0 {
                    continue;
                }
                // The deviation's share in magnitude and in phase.
                let mag = (20.0 * m.norm().log10()).abs();
                let share = excess / (m - Complex::new(1.0, 0.0)).norm();
                if mag * share > db {
                    db = mag * share;
                    at = records.low_hz * 2f64.powf((band as f64 + 0.5) / 3.0);
                }
                deg = deg.max(m.arg().to_degrees().abs() * share);
            }
            Shape {
                rms,
                gain_db: 20.0 * gain.norm().log10(),
                db,
                at_hz: at,
                degrees: deg,
            }
        })
        .collect()
}

/// A record's spectra and the band's bins.
struct Spectra {
    x: Vec<Complex<f64>>,
    y: Vec<Complex<f64>>,
    rms: f64,
}

/// `r`'s response on every bin of a period of `n` (both halves).
fn on_bins(r: &Rational, n: usize) -> Vec<Complex<f64>> {
    let mut h = vec![Complex::new(0.0, 0.0); n];
    for k in 0..=n / 2 {
        h[k] = r.response(std::f64::consts::TAU * k as f64 / n as f64);
        if k > 0 && k < n / 2 {
            h[n - k] = h[k].conj();
        }
    }
    h
}

/// The best curve for one split: knots and values, offset, and the
/// in-band error and output powers per record.
struct Solved {
    knots: Vec<f64>,
    values: Vec<f64>,
    offset: f64,
    error: Vec<(f64, f64, f64)>,
}

impl Solved {
    /// The mean relative error over the records: every level counts the
    /// same, so the clean levels are fitted as well as the driven ones.
    fn total(&self) -> f64 {
        self.error.iter().map(|r| r.1 / r.2.max(1e-300)).sum::<f64>() / self.error.len().max(1) as f64
    }
}

/// `count` knots spanning `u`'s range with a margin, halfway between
/// evenly spaced and at `u`'s quantiles: dense where the signal spends
/// its time (small signals at every level), still reaching its extremes.
fn place_knots(us: &[Vec<f64>], count: usize) -> Vec<f64> {
    let mut all: Vec<f64> = us.iter().flatten().copied().collect();
    all.sort_by(f64::total_cmp);
    let (lo, hi) = (all[0], all[all.len() - 1]);
    let margin = 0.02 * (hi - lo).max(1e-12);
    let (lo, hi) = (lo - margin, hi + margin);
    let last = count - 1;
    let mut knots: Vec<f64> = (0..count)
        .map(|k| {
            let q = k as f64 / last as f64;
            let even = lo + (hi - lo) * q;
            let quantile = all[((all.len() - 1) as f64 * q).round() as usize];
            if k == 0 {
                lo
            } else if k == last {
                hi
            } else {
                0.5 * (even + quantile)
            }
        })
        .collect();
    let gap = 1e-6 * (hi - lo);
    for k in 1..count {
        knots[k] = knots[k].max(knots[k - 1] + gap);
    }
    knots
}

fn solve(data: &[Spectra], band: &[usize], h: &Rational, g: &Rational, knots: usize) -> Option<Solved> {
    let n = data[0].x.len();
    let (hb, gb) = (on_bins(h, n), on_bins(g, n));
    // The filtered inputs, and the curve's domain over all of them.
    let us: Vec<Vec<f64>> = data
        .iter()
        .map(|d| waveform(&d.x.iter().zip(&hb).map(|(a, b)| *a * *b).collect::<Vec<_>>()))
        .collect();
    let grid = place_knots(&us, knots);
    let cols = knots + 1;
    let mut normal = Normal::new(cols);
    let mut columns_of = Vec::with_capacity(data.len());
    for (d, u) in data.iter().zip(&us) {
        // The basis columns' spectra, filtered by G.
        let mut basis = vec![vec![0.0; n]; knots];
        for (t, &v) in u.iter().enumerate() {
            for (j, w) in spline_weights(&grid, v) {
                basis[j][t] += w;
            }
        }
        let columns: Vec<Vec<Complex<f64>>> = basis
            .iter()
            .map(|c| spectrum(c).iter().zip(&gb).map(|(a, b)| *a * *b).collect())
            .collect();
        // Each record weighted by its in-band output power: relative
        // error, level by level.
        let power: f64 = band.iter().map(|&k| 2.0 * d.y[k].norm_sqr()).sum::<f64>().max(1e-300);
        let weight = 1.0 / power;
        // DC: the offset's column is n there, and nowhere else.
        let mut row = vec![0.0; cols];
        for (j, c) in columns.iter().enumerate() {
            row[j] = c[0].re;
        }
        row[knots] = n as f64;
        normal.add(&row, d.y[0].re, weight);
        for &k in band {
            let (mut re, mut im) = (vec![0.0; cols], vec![0.0; cols]);
            for (j, c) in columns.iter().enumerate() {
                re[j] = c[k].re;
                im[j] = c[k].im;
            }
            normal.add(&re, d.y[k].re, 2.0 * weight);
            normal.add(&im, d.y[k].im, 2.0 * weight);
        }
        columns_of.push(columns);
    }
    let x = normal.solve(1e-10)?;
    let error = data
        .iter()
        .zip(&columns_of)
        .map(|(d, columns)| {
            let (mut e, mut p) = (0.0, 0.0);
            for &k in band {
                let m = columns
                    .iter()
                    .zip(&x)
                    .fold(Complex::new(0.0, 0.0), |s, (c, v)| s + c[k].scale(*v));
                e += (d.y[k] - m).norm_sqr();
                p += d.y[k].norm_sqr();
            }
            (d.rms, e, p)
        })
        .collect();
    Some(Solved {
        knots: grid,
        values: x[..knots].to_vec(),
        offset: x[knots],
        error,
    })
}

/// `r` scaled to unit peak magnitude over the band's bins.
fn normalised(mut r: Rational, band: &[usize], n: usize) -> Rational {
    let peak = band
        .iter()
        .map(|&k| r.response(std::f64::consts::TAU * k as f64 / n as f64).norm())
        .fold(0.0f64, f64::max);
    if peak > 0.0 {
        r.gain /= peak;
    }
    r
}

fn describe(r: &Rational) -> String {
    format!("{} zeros, {} poles", r.zeros.len(), r.poles.len())
}

/// Fit a Wiener–Hammerstein model to `records` (multisine periods at
/// several levels; [`Records::measure`]).
pub fn fit(records: &Records, options: &Options) -> Option<Fit> {
    let n = records.records.first()?.input.len();
    let rate = records.rate;
    let (low, high) = options.band.unwrap_or((records.low_hz, records.high_hz));
    let df = rate / n as f64;
    let band: Vec<usize> = (1..n / 2).filter(|&k| k as f64 * df >= low && k as f64 * df <= high).collect();
    let levels = records.levels();
    let reference = options.bla_rms.unwrap_or(levels[0]);

    // 1–2: the BLA and its rational fit, weighted towards equal density
    // per octave.
    let points: Vec<Point> = bla(records, reference)
        .into_iter()
        .map(|(k, h)| Point {
            w: std::f64::consts::TAU * k as f64 / n as f64,
            h,
            weight: 1.0 / k as f64,
        })
        .collect();
    let (linear, bla_error_db) = Rational::fit_order(&points, options.max_order, options.tolerance_db)?;

    // 3: every split.
    let zf = factors(&linear.zeros);
    let pf = factors(&linear.poles);
    let count = zf.len() + pf.len();
    let masks: Vec<u32> = match options.structure {
        Structure::Wiener => vec![(1u32 << count) - 1],
        Structure::Hammerstein => vec![0],
        Structure::WienerHammerstein => (0..1u32 << count).collect(),
    };
    let split = |mask: u32| {
        let mut parts: [(Vec<&Factor>, Vec<&Factor>); 2] = [(Vec::new(), Vec::new()), (Vec::new(), Vec::new())];
        for (i, f) in zf.iter().enumerate() {
            parts[usize::from(mask >> i & 1 == 1)].0.push(f);
        }
        for (i, f) in pf.iter().enumerate() {
            parts[usize::from(mask >> (zf.len() + i) & 1 == 1)].1.push(f);
        }
        let h = normalised(Rational::from_factors(1.0, &parts[1].0, &parts[1].1), &band, n);
        let mut g = Rational::from_factors(1.0, &parts[0].0, &parts[0].1);
        g.delay = linear.delay;
        (h, normalised(g, &band, n))
    };
    let all: Vec<Spectra> = records
        .records
        .iter()
        .map(|r| Spectra {
            x: spectrum(&r.input),
            y: spectrum(&r.output),
            rms: r.rms,
        })
        .collect();
    // Rank on one realisation a level.
    let mut seen = Vec::new();
    let ranking: Vec<Spectra> = all
        .iter()
        .filter(|s| {
            let first = !seen.contains(&s.rms.to_bits());
            seen.push(s.rms.to_bits());
            first
        })
        .map(|s| Spectra {
            x: s.x.clone(),
            y: s.y.clone(),
            rms: s.rms,
        })
        .collect();
    let mut ranked: Vec<(f64, u32)> = masks
        .iter()
        .filter_map(|&m| {
            let (h, g) = split(m);
            solve(&ranking, &band, &h, &g, options.rank_knots).map(|s| (s.total(), m))
        })
        .collect();
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (mask, solved) = ranked
        .iter()
        .take(options.refine.max(1))
        .filter_map(|&(_, m)| {
            let (h, g) = split(m);
            solve(&all, &band, &h, &g, options.knots).map(|s| (m, s))
        })
        .min_by(|a, b| a.1.total().total_cmp(&b.1.total()))?;
    let (h, g) = split(mask);
    let nonlinearity = Nonlinearity::Spline {
        knots: solved.knots.clone(),
        values: solved.values.clone(),
    };
    let monotone = nonlinearity.is_monotone();
    let model = WienerHammerstein::new(
        rate as f32,
        Linear::Rational(h.sections()),
        nonlinearity,
        Linear::Rational(g.sections()),
    )
    .with_offset(solved.offset);
    let error_db = levels
        .iter()
        .map(|&rms| {
            let (e, p) = solved
                .error
                .iter()
                .filter(|r| r.0 == rms)
                .fold((0.0, 0.0), |a, r| (a.0 + r.1, a.1 + r.2));
            (rms, 10.0 * (e / p.max(1e-300)).log10())
        })
        .collect();
    Some(Fit {
        model,
        split: format!("H: {}; G: {}", describe(&h), describe(&g)),
        bla: linear,
        bla_error_db,
        splits_tried: ranked.len(),
        error_db,
        shapes: shapes(records, reference),
        monotone,
    })
}
