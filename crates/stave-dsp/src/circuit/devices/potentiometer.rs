//! Potentiometers and switches as hardware (DSP.md H6): the taper a
//! manufacturer prints, two gangs that never quite track, a worn wiper,
//! a switch's contact bounce. A circuit names these on its variable
//! parts ([`Variable`](crate::circuit::netlist::Variable)); the runtime
//! is [`circuit::variable`](crate::circuit::variable).
//!
//! ```
//! use stave_dsp::circuit::devices::potentiometer::LinearCubic;
//! use stave_dsp::circuit::netlist::Taper;
//!
//! // Alpha's 15A audio taper: 15 % of the track at half rotation.
//! let a = Taper::Curve(LinearCubic::ALPHA_15A);
//! assert!((a.fraction(0.5) - 0.157).abs() < 0.005);
//! // Its reverse ("C") mirrors it.
//! let c = Taper::Curve(LinearCubic::ALPHA_15A.reversed());
//! assert!((c.fraction(0.5) - (1.0 - a.fraction(0.5))).abs() < 1e-12);
//! ```

use crate::primitives::random::SplitMix64;
use crate::units::Db;

/// A taper as manufacturers make one: straight runs of one resistive
/// ink joined by smooth transitions, between points `(rotation,
/// fraction)`.
///
/// # Models
/// A potentiometer's law — the fraction of the track between terminal 1
/// and the wiper against rotation — as Holmes & van Walstijn's piecewise
/// linear–cubic function ("Potentiometer law modelling and identification
/// for application in physics-based virtual analogue circuits", DAFx-19,
/// §2.2.2): sections alternately straight and cubic, each cubic matching
/// the value and slope of the straight runs either side, so the law is
/// C¹ with the flat ends and the knee of a real carbon track. Parts:
/// Alpha's audio (A) tapers [`ALPHA_05A`](Self::ALPHA_05A) to
/// [`ALPHA_30A`](Self::ALPHA_30A), coded by their percentage at half
/// rotation; their reverse (C) curves by [`reversed`](Self::reversed).
///
/// # Derivation
/// The transition points are the paper's Table 2: its fit to Alpha's
/// published taper curves (seven sections, peak error 2 % of the
/// track). A reverse taper is the mirror `1 − f(1 − x)`, as Alpha's C
/// chart draws it (assumed exact).
///
/// # Verified
/// `crates/stave-test/tests/controls_match_datasheet.rs::alpha_tapers_hold_their_codes`
/// (every curve passes Table 2's points exactly and is C¹; at half
/// rotation within 0.8 % of the track of its code — measured 0.25–0.73 %
/// for 05A to 30A; monotone but for the paper's cubics' dips where a
/// short run meets a steep one, 7.7e-5 of the track at most, budget
/// 3e-4) and
/// `crates/stave-test/tests/controls_match_datasheet.rs::measured_alpha_pots_sit_on_their_curves`
/// (three Alpha 100 kΩ log pots and three linear ones the paper's
/// authors measured: the log units within 6.9–15.6 % of the track of
/// [`ALPHA_15A`](Self::ALPHA_15A), worst at 0.85 of rotation where
/// their upper knee comes earlier than the sheet's, as the paper notes —
/// a known gap of units against their sheet, budget 20 %; the linear
/// ones 5.1–7.9 % of the linear law).
///
/// # Cost
/// A few comparisons and one cubic per evaluation, at a knob move's
/// refresh; not a stage, so no datasheet line.
///
/// # Aliasing
/// None: a static law.
///
/// # Valid ranges
/// Rotation 0 to 1 (clamped).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct LinearCubic {
    points: &'static [[f64; 2]],
    reversed: bool,
}

/// Holmes & van Walstijn's transition rotations for the Alpha A family
/// (DAFx-19, Table 2): straight to 0.05, cubic to 0.30, straight to
/// 0.51, cubic to 0.70, straight to 0.92, cubic to 0.97, straight to 1.
const ALPHA_A_ROTATION: [f64; 8] = [0.0, 0.05, 0.30, 0.51, 0.70, 0.92, 0.97, 1.0];

/// Table 2's fractions at those rotations, one row per code.
const fn alpha_a(fractions: [f64; 8]) -> [[f64; 2]; 8] {
    let mut points = [[0.0; 2]; 8];
    let mut i = 0;
    while i < 8 {
        points[i] = [ALPHA_A_ROTATION[i], fractions[i]];
        i += 1;
    }
    points
}

const A05: [[f64; 2]; 8] = alpha_a([0.0, 0.003, 0.015, 0.057, 0.284, 0.954, 0.999, 1.0]);
const A10: [[f64; 2]; 8] = alpha_a([0.0, 0.003, 0.028, 0.111, 0.363, 0.959, 0.999, 1.0]);
const A15: [[f64; 2]; 8] = alpha_a([0.0, 0.003, 0.063, 0.162, 0.410, 0.958, 1.000, 1.0]);
const A20: [[f64; 2]; 8] = alpha_a([0.0, 0.004, 0.084, 0.210, 0.443, 0.952, 1.000, 1.0]);
const A25: [[f64; 2]; 8] = alpha_a([0.0, 0.002, 0.123, 0.259, 0.501, 0.954, 0.999, 1.0]);
const A30: [[f64; 2]; 8] = alpha_a([0.0, 0.004, 0.151, 0.311, 0.542, 0.965, 1.000, 1.0]);

impl LinearCubic {
    /// Alpha 05A (JIS G): 5 % at half rotation.
    pub const ALPHA_05A: Self = Self::new(&A05);
    /// Alpha 10A (JIS D): 10 %.
    pub const ALPHA_10A: Self = Self::new(&A10);
    /// Alpha 15A (JIS A), the common audio taper: 15 %.
    pub const ALPHA_15A: Self = Self::new(&A15);
    /// Alpha 20A: 20 %.
    pub const ALPHA_20A: Self = Self::new(&A20);
    /// Alpha 25A (JIS K): 25 %.
    pub const ALPHA_25A: Self = Self::new(&A25);
    /// Alpha 30A: 30 %.
    pub const ALPHA_30A: Self = Self::new(&A30);

    /// A law through `points` (rotation, fraction), rotations rising
    /// from 0 to 1, sections straight then cubic in turn.
    pub const fn new(points: &'static [[f64; 2]]) -> Self {
        Self { points, reversed: false }
    }

    /// The mirror image, `1 − f(1 − x)`: an A taper's C.
    pub const fn reversed(self) -> Self {
        Self {
            points: self.points,
            reversed: !self.reversed,
        }
    }

    pub fn points(&self) -> &'static [[f64; 2]] {
        self.points
    }

    /// The slope of the straight section `i` (`points[i]` to `points[i + 1]`).
    fn slope(&self, i: usize) -> f64 {
        let (a, b) = (self.points[i], self.points[i + 1]);
        (b[1] - a[1]) / (b[0] - a[0])
    }

    /// The law before any mirroring.
    fn forward(&self, x: f64) -> f64 {
        let p = self.points;
        let last = p.len() - 1;
        let i = p[..last].iter().rposition(|q| q[0] <= x).unwrap_or(0).min(last - 1);
        let (a, b) = (p[i], p[i + 1]);
        let h = b[0] - a[0];
        if h <= 0.0 {
            return a[1];
        }
        if i % 2 == 0 {
            return a[1] + (b[1] - a[1]) * (x - a[0]) / h;
        }
        // Cubic Hermite: the neighbours' slopes at each end.
        let (m0, m1) = (self.slope(i - 1), if i + 1 < last { self.slope(i + 1) } else { 0.0 });
        let t = (x - a[0]) / h;
        let (t2, t3) = (t * t, t * t * t);
        (2.0 * t3 - 3.0 * t2 + 1.0) * a[1] + (t3 - 2.0 * t2 + t) * h * m0 + (-2.0 * t3 + 3.0 * t2) * b[1]
            + (t3 - t2) * h * m1
    }

    /// The track fraction at `rotation` (clamped to 0–1).
    pub fn fraction(&self, rotation: f64) -> f64 {
        let x = rotation.clamp(0.0, 1.0);
        if self.points.len() < 2 {
            return x;
        }
        if self.reversed {
            1.0 - self.forward(1.0 - x)
        } else {
            self.forward(x)
        }
    }

    /// The rotation whose fraction is `fraction`, by bisection (the law
    /// is monotone): off the audio path, where a deck's written position
    /// is read back.
    pub fn rotation(&self, fraction: f64) -> f64 {
        let f = fraction.clamp(0.0, 1.0);
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if self.fraction(mid) < f {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    }
}

/// How far one gang of a dual pot strays from the other: a level error
/// in dB that grows with attenuation.
///
/// # Models
/// Dual-gang tracking ("gang error"), as datasheets of dual audio pots
/// state it: the second gang's level differs from the first by at most
/// `spec_db` down to a stated attenuation (commonly a few dB to −40 or
/// −60 dB). Each unit's error is drawn once, uniformly within the spec,
/// and grows in proportion to the attenuation (in dB): two inks' small
/// resistivity mismatch matters most where the track is thinnest.
///
/// # Derivation
/// A tolerance model, not a fit: the unit's gang applies
/// `10^(δ(a)/20)` to the first gang's fraction, `δ(a) = e·a/a_ref`
/// for attenuation `a = −20·log₁₀ f` (dB), `e` uniform in `±spec_db`
/// from the unit's seed; the fraction is held below 1.
///
/// # Verified
/// `crates/stave-test/tests/controls_match_datasheet.rs::gangs_track_within_their_spec`
/// (10 000 seeded units: every one within the spec from 0 to the
/// reference attenuation, the spread uniform, each seed the same unit
/// twice) and
/// `crates/stave-test/tests/controls_match_datasheet.rs::a_dual_gang_volume_mismatches_its_channels`
/// (two `Circuit<Dk>` volume pots, one a drawn second gang: their level
/// difference is the model's to 1e-6 dB at every position).
///
/// # Cost
/// One `log10` and one `exp` at a knob move's refresh; no datasheet
/// line.
///
/// # Aliasing
/// None: a static law.
///
/// # Valid ranges
/// Any spec; fractions 0–1.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Tracking {
    /// This unit's level error at the reference attenuation, dB.
    pub error_db: f64,
    /// The attenuation the error is specified at, dB (positive).
    pub reference_db: f64,
}

impl Tracking {
    /// A perfect gang.
    pub const EXACT: Self = Self {
        error_db: 0.0,
        reference_db: 40.0,
    };

    /// A unit drawn from `seed` within `±spec_db` at `reference_db` of
    /// attenuation.
    pub fn draw(spec: Db, reference: Db, seed: u64) -> Self {
        let spec_db = f64::from(spec.get());
        let reference_db = f64::from(reference.get());
        let mut r = SplitMix64::new(seed);
        Self {
            error_db: 2.0 * r.next_centred() * spec_db.abs(),
            reference_db: reference_db.abs().max(1e-6),
        }
    }

    /// The gang's fraction where the first gang's is `fraction`.
    #[inline]
    pub fn fraction(&self, fraction: f64) -> f64 {
        if self.error_db == 0.0 || fraction <= 0.0 {
            return fraction;
        }
        let attenuation = -20.0 * fraction.log10();
        let db = self.error_db * attenuation / self.reference_db;
        (fraction * 10f64.powf(db / 20.0)).min(1.0)
    }
}

/// A wiper that crackles as it moves.
///
/// # Models
/// Scratch: a worn or dirty carbon track's contact hopping between
/// grains as the wiper slides, heard as noise while a knob turns and
/// silence when it stops. The wiper's effective position jitters by
/// band-limited noise of rms `scratch × speed` (speed in rotations per
/// second), so a still knob is clean and a fast sweep crackles most.
///
/// # Derivation
/// A behavioural law placed on the circuit's pot: Gaussian noise from
/// the unit's seed through a one-pole at `corner_hz` (the crackle's
/// bandwidth), scaled to unit rms and by the wiper's speed, added to the
/// track fraction (held within 0–1). No published measurement is
/// claimed; the law's shape (zero at rest, proportional to speed) is the
/// hardware's.
///
/// # Verified
/// `crates/stave-test/tests/controls_match_datasheet.rs::a_scratchy_wiper_crackles_only_while_turning`
/// (jitter rms `scratch × speed` within 2 % at 0.25 and 1 rotation/s,
/// measured 0.1 %; the circuit's output identical to a clean pot's,
/// sample for sample, once the knob stops).
///
/// # Cost
/// A refresh of the circuit's matrices every sample while the knob moves
/// (as a glide already does); no datasheet line of its own.
///
/// # Aliasing
/// None of its own: the jitter is band-limited to `corner_hz`.
///
/// # Valid ranges
/// `scratch` from 0 (clean) to about 0.05; corners below a quarter of
/// the sample rate.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Wiper {
    pub scratch: f64,
    pub corner_hz: f64,
    pub seed: u64,
}

impl Wiper {
    pub const fn new(scratch: f64, seed: u64) -> Self {
        Self {
            scratch,
            corner_hz: 2_000.0,
            seed,
        }
    }
}

/// The most bounces a contact makes.
pub const MAX_BOUNCES: usize = 16;

/// A mechanical contact's chatter on closing.
///
/// # Models
/// Contact bounce: a closing switch's contacts part and remake several
/// times within a few milliseconds before they settle (datasheets of
/// toggle and footswitches state a maximum bounce time, typically 1 to
/// 10 ms); opening is clean.
///
/// # Derivation
/// `count` re-openings at times drawn uniformly over `seconds` from the
/// unit's seed (each a sample long at least), the contact closed at the
/// end; each open–closed edge is the switch's full resistance step, so
/// the circuit hears every one.
///
/// # Verified
/// `crates/stave-test/tests/controls_match_datasheet.rs::switches_pop_bounce_and_soften`
/// (in a circuit, the output jumps once per drawn edge plus the first
/// make, all within `seconds`; each seed the same chatter). The same test
/// holds a hard switch's pop to the step the open circuit's DC predicts
/// (0.07 %, budget 0.5 %), a 1 MΩ bleeder's reduction of it (−20.0 dB),
/// and a 10 ms ramp's (−21.7 dB of click, budget −18).
///
/// # Cost
/// A refresh per edge; no datasheet line.
///
/// # Aliasing
/// Each edge is a step in a resistance, so the circuit's response to it
/// aliases as any step does; run it oversampled where that matters.
///
/// # Valid ranges
/// `count` up to [`MAX_BOUNCES`]; `seconds` above a sample.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Bounce {
    pub seconds: f64,
    pub count: u32,
    pub seed: u64,
}

impl Bounce {
    /// The re-opening times in samples at `sample_rate` for the `n`th
    /// closing (sorted, each pair an open then a close), into `out`;
    /// returns how many edges.
    pub fn edges(&self, sample_rate: f64, closing: u64, out: &mut [u32; 2 * MAX_BOUNCES]) -> usize {
        let mut r = SplitMix64::new(self.seed ^ closing.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let span = (self.seconds.max(0.0) * sample_rate).max(2.0);
        let n = (self.count as usize).min(MAX_BOUNCES);
        let mut opens = [0u32; MAX_BOUNCES];
        for o in opens.iter_mut().take(n) {
            *o = 1 + ((r.next_centred() + 0.5) * (span - 2.0)) as u32;
        }
        opens[..n].sort_unstable();
        let mut count = 0;
        let mut last_close = 0u32;
        for &open in &opens[..n] {
            let open = open.max(last_close + 1);
            // Each opening lasts a little: up to a fifth of the span, at
            // least a sample, never past the end.
            let length = 1 + ((r.next_centred() + 0.5) * 0.2 * span) as u32;
            let close = (open + length).min(span as u32);
            if open >= close {
                break;
            }
            out[count] = open;
            out[count + 1] = close;
            count += 2;
            last_close = close;
        }
        count
    }
}

#[cfg(test)]
mod tests;
