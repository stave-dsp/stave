//! Graphic-EQ attenuation filters: the per-line filter that holds a
//! reverberation time per octave band (Schlecht & Habets 2017; Prawda,
//! Schlecht & Välimäki 2019).
//!
//! A loop of `L` samples decays at `T60(ω)` when its filter's gain is
//! `A_dB(ω) = L·γ_dB(ω)`, `γ_dB = −60/(fs·T60)` (Jot's condition).
//! The filter is Prawda's: a broadband gain at the median band target,
//! ten octave peak-notch biquads (31.25 Hz … 16 kHz) and a first-order
//! high shelf holding the top band's gain above 16 kHz. Its band gains
//! are a least-squares fit at the band centres and the geometric
//! midpoints between them, through the interaction matrix of the
//! peak filters at a prototype gain (Välimäki & Liski's accurate cascade
//! graphic equaliser), then refined by evaluating the real cascade and
//! correcting by the same pseudo-inverse a fixed number of times, so the
//! nonlinearity of the peak filters' dB response is fitted, not
//! approximated.
//!
//! Everything is fixed-size: a design allocates nothing, so `set_t60`
//! may run on the audio thread (about 10 µs per line).

use super::t60::BANDS;

/// Control points: the band centres and the midpoints between them.
pub(crate) const POINTS: usize = 2 * BANDS - 1;
/// Sections: ten peaks and the shelf.
const SECTIONS: usize = BANDS + 1;
/// The peak filters' bandwidth in octaves (RBJ's, prewarped so a band near
/// Nyquist keeps it): wide enough that the cascade is
/// smooth between octave centres (Q 0.8 at low frequencies),
/// narrow enough that the interaction matrix stays
/// well conditioned.
const OCTAVES: f64 = 1.7;
/// The prototype gain the interaction matrix is taken at, dB.
const PROTOTYPE_DB: f64 = 6.0;
/// Refinements of the band gains against the real cascade.
const REFINEMENTS: usize = 4;
/// Prawda et al.'s shelf corner, Hz (capped at 0.45·fs).
const SHELF_HZ: f64 = 20_200.0;

/// One second-order section, transposed direct form II, in `f64`: the
/// 31.5 Hz band's poles sit within 1e-3 of `z = 1` at 192 kHz, where
/// `f32` coefficients would misplace them by a tenth of a dB.
#[derive(Copy, Clone, Debug, Default)]
struct Section {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Section {
    const IDENTITY: Self = Self {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
        z1: 0.0,
        z2: 0.0,
    };

    #[inline]
    fn tick(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    fn set(&mut self, c: [f64; 5]) {
        [self.b0, self.b1, self.b2, self.a1, self.a2] = c;
    }
}

/// The RBJ peaking biquad `[b0, b1, b2, a1, a2]` (normalised) at `w`
/// rad/sample with gain `db`. Its dB response is odd in `db`: a cut is
/// the exact mirror of the boost.
fn peak(w: f64, db: f64) -> [f64; 5] {
    let a = 10f64.powf(db / 40.0);
    let alpha = w.sin() * (core::f64::consts::LN_2 / 2.0 * OCTAVES * w / w.sin()).sinh();
    let cos = w.cos();
    let a0 = 1.0 + alpha / a;
    [
        (1.0 + alpha * a) / a0,
        -2.0 * cos / a0,
        (1.0 - alpha * a) / a0,
        -2.0 * cos / a0,
        (1.0 - alpha / a) / a0,
    ]
}

/// The first-order high shelf (bilinear, prewarped to `w`): unity at DC,
/// `db` at Nyquist.
fn shelf(w: f64, db: f64) -> [f64; 5] {
    let g = 10f64.powf(db / 20.0);
    let k = (w / 2.0).tan();
    [
        (g + k) / (1.0 + k),
        (k - g) / (1.0 + k),
        0.0,
        (k - 1.0) / (1.0 + k),
        0.0,
    ]
}

/// A section's gain in dB at the point whose `(cos ω, cos 2ω)` is `c`.
fn gain_db(s: &[f64; 5], c: (f64, f64)) -> f64 {
    let [b0, b1, b2, a1, a2] = *s;
    let num = b0 * b0 + b1 * b1 + b2 * b2 + 2.0 * (b0 * b1 + b1 * b2) * c.0 + 2.0 * b0 * b2 * c.1;
    let den = 1.0 + a1 * a1 + a2 * a2 + 2.0 * (a1 + a1 * a2) * c.0 + 2.0 * a2 * c.1;
    10.0 * (num / den).max(1e-300).log10()
}

/// The rate-dependent half of the design: frequencies, the interaction
/// matrix's pseudo-inverse. Built once per sample rate.
#[derive(Clone, Debug)]
pub(crate) struct Design {
    /// Band centres, rad/sample.
    centres: [f64; BANDS],
    /// Bands below 0.45·fs (the rest stay flat).
    active: [bool; BANDS],
    /// Control points, rad/sample, with `(cos ω, cos 2ω)`.
    points: [f64; POINTS],
    cos: [(f64, f64); POINTS],
    shelf_w: f64,
    /// The interaction matrix: band `m`'s dB gain at point `k` per dB
    /// of command gain, at the prototype gain.
    b: [[f64; BANDS]; POINTS],
}

impl Design {
    pub(crate) fn new(sample_rate: f32) -> Self {
        let fs = f64::from(sample_rate);
        let tau = core::f64::consts::TAU;
        let mut centres = [0.0; BANDS];
        let mut active = [false; BANDS];
        for k in 0..BANDS {
            let f = 1_000.0 * 2f64.powi(k as i32 - 5);
            centres[k] = tau * f / fs;
            active[k] = f < 0.45 * fs;
        }
        let mut points = [0.0; POINTS];
        for k in 0..POINTS {
            let f = 1_000.0 * 2f64.powf(k as f64 / 2.0 - 5.0);
            points[k] = (tau * f / fs).min(0.49 * tau);
        }
        let cos = points.map(|w| (w.cos(), (2.0 * w).cos()));
        // B[k][m]: band m's normalised dB gain at point k.
        let mut b = [[0.0f64; BANDS]; POINTS];
        for m in (0..BANDS).filter(|&m| active[m]) {
            let s = peak(centres[m], PROTOTYPE_DB);
            for k in 0..POINTS {
                b[k][m] = gain_db(&s, cos[k]) / PROTOTYPE_DB;
            }
        }
        Self {
            centres,
            active,
            points,
            cos,
            shelf_w: (tau * SHELF_HZ / fs).min(0.45 * tau),
            b,
        }
    }

    /// The weighted pseudo-inverse `(BᵀWB + λI)⁻¹BᵀW`, `W = diag(w)`;
    /// the Tikhonov term makes an inactive band's zero column solve to
    /// a zero gain.
    fn pinv(&self, w: &[f64; POINTS]) -> [[f64; POINTS]; BANDS] {
        let b = &self.b;
        let mut n = [[0.0f64; BANDS]; BANDS];
        for i in 0..BANDS {
            for j in 0..BANDS {
                n[i][j] = (0..POINTS).map(|k| w[k] * b[k][i] * b[k][j]).sum::<f64>();
            }
            n[i][i] += 1e-9;
        }
        let inv = invert(n);
        let mut pinv = [[0.0f64; POINTS]; BANDS];
        for i in 0..BANDS {
            for k in 0..POINTS {
                pinv[i][k] = w[k] * (0..BANDS).map(|j| inv[i][j] * b[k][j]).sum::<f64>();
            }
        }
        pinv
    }

    /// The control points, rad/sample: band `k`'s centre is point `2k`.
    pub(crate) fn points(&self) -> &[f64; POINTS] {
        &self.points
    }

    /// Fit `eq` to `target` (dB at each control point; every value ≤ 0).
    pub(crate) fn fit(&self, target: &[f64; POINTS], eq: &mut GraphicEq) {
        let mut centres: [f64; BANDS] = core::array::from_fn(|k| target[2 * k]);
        let active = BANDS - self.active.iter().filter(|a| !**a).count();
        centres[..active].sort_by(f64::total_cmp);
        let g0 = if active % 2 == 1 {
            centres[active / 2]
        } else {
            0.5 * (centres[active / 2 - 1] + centres[active / 2])
        };
        // The top active band's gain, relative to the broadband gain.
        let top = (0..BANDS).rev().find(|&k| self.active[k]).unwrap_or(0);
        let shelf = shelf(self.shelf_w, target[2 * top] - g0);
        let shelf_db = self.cos.map(|c| gain_db(&shelf, c));
        let mut gains = [0.0f64; BANDS];
        let mut sections = [[0.0f64; 5]; BANDS];
        let mut residual: [f64; POINTS] = core::array::from_fn(|k| target[k] - g0 - shelf_db[k]);
        // Prawda et al.'s weighting: the error relative to the target,
        // since a dB error near 0 dB moves the T60 most.
        let weights = target.map(|t| 1.0 / t.abs().max(1e-3));
        let pinv = self.pinv(&weights);
        for pass in 0..=REFINEMENTS {
            for m in 0..BANDS {
                gains[m] += (0..POINTS).map(|k| pinv[m][k] * residual[k]).sum::<f64>();
                sections[m] = if self.active[m] {
                    peak(self.centres[m], gains[m])
                } else {
                    [1.0, 0.0, 0.0, 0.0, 0.0]
                };
            }
            if pass == REFINEMENTS {
                break;
            }
            for k in 0..POINTS {
                let actual = g0
                    + shelf_db[k]
                    + sections
                        .iter()
                        .map(|s| gain_db(s, self.cos[k]))
                        .sum::<f64>();
                residual[k] = target[k] - actual;
            }
        }
        for (section, c) in eq.sections.iter_mut().zip(sections) {
            section.set(c);
        }
        eq.sections[BANDS].set(shelf);
        eq.gain = 10f64.powf(g0 / 20.0);
    }
}

/// Invert a symmetric positive-definite matrix by Gauss–Jordan with
/// partial pivoting.
fn invert(mut a: [[f64; BANDS]; BANDS]) -> [[f64; BANDS]; BANDS] {
    let mut inv = [[0.0f64; BANDS]; BANDS];
    for (i, row) in inv.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    for col in 0..BANDS {
        let pivot = (col..BANDS)
            .max_by(|&x, &y| a[x][col].abs().total_cmp(&a[y][col].abs()))
            .unwrap_or(col);
        a.swap(col, pivot);
        inv.swap(col, pivot);
        let d = a[col][col];
        for j in 0..BANDS {
            a[col][j] /= d;
            inv[col][j] /= d;
        }
        for r in 0..BANDS {
            if r != col {
                let f = a[r][col];
                for j in 0..BANDS {
                    a[r][j] -= f * a[col][j];
                    inv[r][j] -= f * inv[col][j];
                }
            }
        }
    }
    inv
}

/// One line's attenuation filter: a broadband gain and eleven sections.
#[derive(Clone, Debug)]
pub(crate) struct GraphicEq {
    gain: f64,
    sections: [Section; SECTIONS],
}

impl Default for GraphicEq {
    fn default() -> Self {
        Self {
            gain: 1.0,
            sections: [Section::IDENTITY; SECTIONS],
        }
    }
}

impl GraphicEq {
    #[inline]
    pub(crate) fn tick(&mut self, x: f32) -> f32 {
        let mut y = f64::from(x) * self.gain;
        for s in &mut self.sections {
            y = s.tick(y);
        }
        crate::kernel::denormal::flush(y as f32)
    }

    pub(crate) fn reset(&mut self) {
        for s in &mut self.sections {
            (s.z1, s.z2) = (0.0, 0.0);
        }
    }

    /// The designed gain in dB at `w` rad/sample.
    #[cfg(test)]
    pub(crate) fn response_db(&self, w: f64) -> f64 {
        let c = (w.cos(), (2.0 * w).cos());
        20.0 * self.gain.log10()
            + self
                .sections
                .iter()
                .map(|s| gain_db(&[s.b0, s.b1, s.b2, s.a1, s.a2], c))
                .sum::<f64>()
    }
}

/// The per-sample dB target `L·γ_dB(ω)` at every control point for a
/// loop of `loop_samples` holding `t60` (seconds per band) at
/// `sample_rate`; midpoints take the mean of their neighbours' dB (the
/// geometric mean of the gains). An infinite band is lossless (0 dB).
pub(crate) fn loop_target(
    t60: &[f32; BANDS],
    loop_samples: f64,
    sample_rate: f32,
) -> [f64; POINTS] {
    let fs = f64::from(sample_rate);
    let band = |k: usize| {
        let t = f64::from(t60[k]);
        if t.is_infinite() {
            0.0
        } else {
            -60.0 * loop_samples / (fs * t.max(1e-6))
        }
    };
    core::array::from_fn(|k| {
        if k % 2 == 0 {
            band(k / 2)
        } else {
            0.5 * (band(k / 2) + band(k / 2 + 1))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prawda et al.'s Promenadi Hall profile (their Table 1).
    const PROMENADI: [f32; BANDS] = [3.0, 2.8, 2.68, 2.55, 2.47, 2.5, 2.3, 1.89, 1.4, 1.2];

    /// The designed filter meets its dB target for loops from 10 to
    /// 150 ms, at every host rate, for real rooms (Prawda et al.'s hall,
    /// a studio, a flat 1 s): the T60 it implies is within 2 % of the
    /// profile at every band centre, 31.5 Hz to 16 kHz (Prawda et al.
    /// reach 1.6 % to 8 kHz and 4.4 % at 16 kHz), and 3 % at the
    /// midpoints; a steep profile (neighbours 67 % apart) within 5 %
    /// everywhere. The design half of the accuracy budget; the measured
    /// decay is the reverb tests'.
    #[test]
    fn the_fit_meets_the_target_at_every_control_point() {
        const STUDIO: [f32; BANDS] = [1.2, 1.1, 1.0, 0.9, 0.85, 0.8, 0.7, 0.55, 0.4, 0.3];
        const STEEP: [f32; BANDS] = [0.3, 0.5, 0.8, 1.2, 2.0, 2.0, 1.5, 1.0, 0.6, 0.4];
        let mut worst = [0.0f64; 4];
        let mut steep = 0.0f64;
        let mut over = Vec::new();
        for rate in [44_100.0f32, 48_000.0, 96_000.0, 192_000.0] {
            let design = Design::new(rate);
            for ms in [10.0, 50.0, 100.0, 150.0] {
                let l = f64::from(rate) * ms / 1_000.0;
                for (name, profile) in [
                    ("hall", PROMENADI),
                    ("studio", STUDIO),
                    ("flat", [1.0; BANDS]),
                    ("steep", STEEP),
                ] {
                    let target = loop_target(&profile, l, rate);
                    let mut eq = GraphicEq::default();
                    design.fit(&target, &mut eq);
                    for k in 0..POINTS {
                        let got = eq.response_db(design.points()[k]);
                        // The T60 this gain implies, over the target's.
                        let error = (target[k] / got - 1.0).abs();
                        let (slot, budget) = match k {
                            _ if name == "steep" => {
                                steep = steep.max(error);
                                (3, 0.05)
                            }
                            18 => (1, 0.02),
                            k if k % 2 == 0 => (0, 0.02),
                            _ => (2, 0.03),
                        };
                        worst[slot] = worst[slot].max(error);
                        if error >= budget {
                            over.push(format!(
                                "{name} {rate} Hz, {ms} ms, point {k}: {got:.4} dB for {:.4}",
                                target[k]
                            ));
                        }
                    }
                }
            }
        }
        println!(
            "worst implied T60 error: {:.2} % at centres to 8 kHz, {:.2} % at 16 kHz, {:.2} % at midpoints; steep profile {:.1} %",
            worst[0] * 100.0,
            worst[1] * 100.0,
            worst[2] * 100.0,
            steep * 100.0
        );
        assert!(over.is_empty(), "over budget:\n{}", over.join("\n"));
    }

    /// An all-infinite profile designs the identity (lossless).
    #[test]
    fn an_infinite_profile_is_lossless() {
        let design = Design::new(48_000.0);
        let target = loop_target(&[f32::INFINITY; BANDS], 3_000.0, 48_000.0);
        let mut eq = GraphicEq::default();
        design.fit(&target, &mut eq);
        for w in [0.001, 0.01, 0.1, 1.0, 3.0] {
            assert!(eq.response_db(w).abs() < 1e-5, "{w}: {}", eq.response_db(w));
        }
    }
}
