//! Plate reverberation: a thin metal plate under tension, driven at one
//! point and heard at one or two, as a bank of its modes.
//!
//! A plate rings in thousands of bending modes — an EMT 140's 2 m² of
//! 0.5 mm steel has about 1.3 a hertz, 13 000 below 10 kHz — and each
//! decays at a rate that rises with frequency. [`Plate`] computes them
//! from the plate's dimensions, material and tension ([`Geometry`]),
//! decays them by a two-parameter loss law ([`Loss`]), weights them by
//! the drive's and the pickups' positions ([`Point`]), and runs them as
//! Mathews–Smith phasor resonators, eight to a SIMD group.
//!
//! **Model choices.** The boundaries are simply supported, as in the
//! DAFx-26 parameter-estimation challenge's simulator (Ducceschi &
//! Gabrielli 2026) and Ducceschi & Webb (ICA 2016): a real plate hangs
//! from springs with free edges, but the simply supported plate's modes
//! are analytic and its modal density is the same, so the reverb's
//! density and decay — what is heard — match; the exact low-mode pattern
//! does not. The loss is the challenge's `σ = η0 + η1 Ω²` (Bilbao 2007's
//! two-T60 law). Arcas & Chaigne (2010) and Bilbao (2007) — the
//! thermoelastic and radiation losses of a steel plate, and the EMT 140's
//! measured decay — are paywalled and were not consulted; the EMT preset's
//! decay above 500 Hz is therefore a stated voicing choice.

mod geometry;

pub use geometry::{Geometry, Loss, Material, Output, Point};

use crate::physical::lanes::{self, Lanes};
use geometry::LN_1000;

use crate::kernel::node::{Stage, Tail};
use crate::units::{Hz, Seconds};

/// One mode of a [`Plate`], in SI units.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Mode {
    /// Half-waves along the width.
    pub m: u32,
    /// Half-waves along the height.
    pub n: u32,
    /// Undamped angular frequency `Ω`, rad/s.
    pub natural: f64,
    /// Decay rate `σ`, 1/s.
    pub sigma: f64,
    /// The modal shape at the drive point, 1/m.
    pub drive: f64,
    /// The modal shape at each pickup, 1/m.
    pub pickups: [f64; 2],
}

impl Mode {
    /// The undamped frequency, Hz.
    pub fn hz(&self) -> f64 {
        self.natural / core::f64::consts::TAU
    }

    /// The damped angular frequency `√(Ω² − σ²)` it rings at, rad/s.
    pub fn damped(&self) -> f64 {
        (self.natural * self.natural - self.sigma * self.sigma).max(0.0).sqrt()
    }

    /// Its T60.
    pub fn t60(&self) -> Seconds {
        Seconds::new(LN_1000 / self.sigma)
    }
}

/// A plate reverb: the modes of a thin, tensioned, simply supported
/// plate, driven at one point and heard at two.
///
/// # Models
/// The damped Kirchhoff–Love plate with in-plane tension as Ducceschi
/// & Gabrielli state it (DAFx-26 challenge, eq. 1): `ρh ∂²u/∂t² = −ρh K u − 2ρh(η0 + η1 K) ∂u/∂t + δ(x − xi) f`,
/// `K = −(T0/ρh)Δ + (D/ρh)ΔΔ`, simply supported on a rectangle, solved
/// exactly in its modes: mode `(m, n)` rings at
/// `Ω² = (T0 γ² + D γ⁴)/ρh`, `γ² = (mπ/Lx)² + (nπ/Ly)²`, decays at
/// `σ = η0 + η1 Ω²`, and is weighted by `Φ(xi)Φ(xo)/ρh` with
/// `Φ = 2/√(Lx Ly) sin(mπx/Lx) sin(nπy/Ly)`. Every mode up to the
/// bandwidth ([`set_bandwidth`](Self::set_bandwidth), 10 kHz as in the
/// challenge) is kept; overdamped ones (`σ ≥ Ω`) are dropped and counted.
///
/// # Derivation
/// Each mode's impulse response `Φ(xi)Φ(xo)/(ρh ω) e^(−σt) sin ωt`,
/// `ω = √(Ω² − σ²)`, sampled (impulse invariance: an input sample is a
/// force held for one period, so the response scales by `T`) is a
/// Mathews–Smith phasor `z ← r e^(jωT) z + x`, `r = e^(−σT)`: displacement
/// reads `Im z`; velocity `ω Re z − σ Im z` and acceleration
/// `(σ² − ω²) Im z − 2σω Re z` (the derivatives, exact; the Dirac at
/// `t = 0` they carry sums over modes to the drive point's delta, zero
/// at a pickup elsewhere). The bank runs eight modes to a group in f32,
/// block by block (`physical/lanes.rs`, shared with the membrane and the bar). Each pickup's output is scaled so its
/// impulse response carries unit energy (the modes' energies
/// `|w|²/2(1 − r²)` summed; [`scale`](Self::scale) is that factor, to
/// recover SI units).
///
/// # Verified
/// `crates/stave-dsp/src/physical/plate/tests.rs::modes_are_leissas_without_tension`
/// (tension-free modes against Leissa's closed form within 1e-12),
/// `crates/stave-dsp/src/physical/plate/tests.rs::mode_count_follows_weyls_law`
/// (counts within 0.5 % of `Sγ²/4π − Pγ/4π`),
/// `crates/stave-dsp/src/physical/plate/tests.rs::impulse_response_is_the_continuous_solution`
/// (the bank's output against the analytic sum, f64, every output kind,
/// within 1e-4 of its peak),
/// `crates/stave-dsp/src/physical/plate/tests.rs::modes_ring_where_the_theory_puts_them`
/// (the IR's spectral peaks at the damped frequencies within 0.01 %),
/// `crates/stave-dsp/src/physical/plate/tests.rs::block_and_tick_agree_bit_for_bit`,
/// `crates/stave-dsp/src/physical/plate/tests.rs::pickups_are_calibrated_to_unit_energy`,
/// and in stave-test
/// `crates/stave-test/tests/plate_matches_reference.rs::modes_match_the_dafx26_model`
/// (every mode of three plates — 1703, 2563 and 12 918 modes — against
/// the challenge document's eqs. 8, 11 and 13 written independently:
/// frequency and decay within 1e-12, the update's gain within 1e-9;
/// measured 8e-16),
/// `crates/stave-test/tests/plate_matches_reference.rs::octave_decay_follows_the_loss_law`
/// (ISO 3382 T30 per octave, 125 Hz–4 kHz, against the loss law's
/// prediction through the same octave filter: within 3 %, measured
/// 1.4 %),
/// `crates/stave-test/tests/plate_matches_reference.rs::emt140_damper_spans_its_published_range`
/// (T30 at 500 Hz within 5 % of the damper's setting at 0.8, 2.5 and
/// 5 s; measured 2.5 %).
///
/// # Cost
/// `"Plate (EMT 140, to 2.5 kHz)"`, `"Plate (EMT 140, to 5 kHz)"`,
/// `"Plate (EMT 140, to 10 kHz)"` and `"Plate (EMT 140, to 20 kHz)"` on
/// the datasheet (both pickups, a block at a time), and
/// `"Plate (EMT 140, to 5 kHz, mono tick)"`: linear in the mode count,
/// which grows linearly with the bandwidth and with the plate's area
/// times `√(ρh/D)`. A single `tick` streams every mode's state through
/// the cache once a sample; the block forms keep a group in registers
/// for up to 64 samples.
///
/// # Aliasing
/// None: linear, and every mode lies below `0.45·fs`.
///
/// # Valid ranges
/// Positive sides, thickness and material constants; tension ≥ 0;
/// drive and pickups inside `(0, 1)²` (on a nodal line a mode is not
/// heard); bandwidth up to `0.45·fs` (beyond it, `0.45·fs` is used and
/// [`bandwidth`](Self::bandwidth) says so). f32 state holds a T60 to
/// about 0.5 % at 20 s and 48 kHz.
#[derive(Clone, Debug)]
pub struct Plate {
    geometry: Geometry,
    loss: Loss,
    drive: Point,
    pickups: [Point; 2],
    output: Output,
    top: Hz,
    sample_rate: f32,
    modes: Vec<Mode>,
    overdamped: usize,
    /// `sin(kπ p)` for each axis and point, k = 1… (realtime moves).
    sines: Sines,
    scale: [f64; 2],
    lanes: Lanes,
}

/// Per-axis sine tables for the drive and both pickups.
#[derive(Clone, Debug, Default)]
struct Sines {
    x: [Vec<f64>; 3],
    y: [Vec<f64>; 3],
}

impl Plate {
    /// A plate of `geometry` losing energy by `loss`: driven at the
    /// challenge's input point (0.335, 0.467), heard at (0.61, 0.61) and
    /// (0.73, 0.29), velocity pickups, modes to 10 kHz. Allocates here
    /// and in `prepare`.
    pub fn new(geometry: Geometry, loss: Loss) -> Self {
        let mut p = Self {
            geometry,
            loss,
            drive: Point::new(0.335, 0.467),
            pickups: [Point::new(0.61, 0.61), Point::new(0.73, 0.29)],
            output: Output::Velocity,
            top: Hz::new(10_000.0),
            sample_rate: 48_000.0,
            modes: Vec::new(),
            overdamped: 0,
            sines: Sines::default(),
            scale: [1.0; 2],
            lanes: Lanes::default(),
        };
        p.build();
        p
    }

    /// An EMT 140: [`Geometry::EMT140`], its damper set for
    /// `t60_at_500` at 500 Hz ([`Loss::emt140`]; clamped into the passive
    /// range), driven near the centre and heard at two points off the
    /// plate's rational fractions (Hanks: never at an integer fraction of
    /// the sides, against flutter echo).
    pub fn emt140(t60_at_500: Seconds) -> Self {
        let loss = Loss::emt140(t60_at_500)
            .or_else(|| Loss::emt140(Seconds::new(5.0)))
            .unwrap_or(Loss::DAFX26);
        let mut p = Self::new(Geometry::EMT140, loss);
        p.drive = Point::new(0.447, 0.513);
        p.pickups = [Point::new(0.219, 0.673), Point::new(0.781, 0.339)];
        p.build();
        p
    }

    /// The plate.
    pub fn geometry(&self) -> &Geometry {
        &self.geometry
    }

    /// The loss in use.
    pub fn loss(&self) -> Loss {
        self.loss
    }

    /// The modes kept, by rising frequency.
    pub fn modes(&self) -> &[Mode] {
        &self.modes
    }

    /// How many modes below the bandwidth were dropped as overdamped.
    pub fn overdamped(&self) -> usize {
        self.overdamped
    }

    /// The highest mode frequency kept: as set, or `0.45·fs` when the
    /// setting is above it.
    pub fn bandwidth(&self) -> Hz {
        Hz::new(self.top.get().min(0.45 * self.sample_rate))
    }

    /// Keep modes up to `top` (the cost knob: the count is linear in
    /// it). Takes effect at the next `prepare` (it allocates).
    pub fn set_bandwidth(&mut self, top: Hz) {
        self.top = top;
    }

    /// The factor from SI units (m, m/s or m/s² per newton-sample of
    /// drive) to pickup `i`'s output.
    pub fn scale(&self, i: usize) -> f64 {
        self.scale[i.min(1)]
    }

    /// The loss (block rate: it recomputes every mode, no allocation;
    /// the set of modes is the one `prepare` enumerated, and a mode the
    /// new loss overdamps falls silent).
    pub fn set_loss(&mut self, loss: Loss) {
        self.loss = loss;
        for m in &mut self.modes {
            m.sigma = loss.sigma(m.natural);
        }
        self.update_coefficients();
    }

    /// Move the drive point (block rate, no allocation).
    pub fn set_drive(&mut self, at: Point) {
        self.drive = at;
        self.fill_sines(0);
        self.update_shapes();
    }

    /// Move pickup `i` (0 left, 1 right; block rate, no allocation).
    pub fn set_pickup(&mut self, i: usize, at: Point) {
        let i = i.min(1);
        self.pickups[i] = at;
        self.fill_sines(i + 1);
        self.update_shapes();
    }

    /// What the pickups read (block rate, no allocation).
    pub fn set_output(&mut self, output: Output) {
        self.output = output;
        self.update_coefficients();
    }

    /// One sample in, both pickups out.
    #[inline]
    pub fn tick_stereo(&mut self, x: f32) -> (f32, f32) {
        let (mut l, mut r) = ([0.0f32], [0.0f32]);
        self.lanes.run::<true>(&[x], &mut l, &mut r);
        (l[0], r[0])
    }

    /// A block in, both pickups out (`left` and `right` at least as long
    /// as `input`); bit-identical to [`tick_stereo`](Self::tick_stereo)
    /// sample by sample.
    pub fn process_stereo(&mut self, input: &[f32], left: &mut [f32], right: &mut [f32]) {
        let mut at = 0;
        while at < input.len() {
            let n = self.lanes.room().min(input.len() - at);
            let end = at + n;
            self.lanes.run::<true>(&input[at..end], &mut left[at..end], &mut right[at..end]);
            at = end;
        }
    }

    /// Enumerate the modes and size everything (allocates).
    fn build(&mut self) {
        let g = self.geometry;
        let top = core::f64::consts::TAU * f64::from(self.bandwidth().get());
        let (mx, my) = (g.index_bound(g.width, top), g.index_bound(g.height, top));
        self.modes.clear();
        self.overdamped = 0;
        for m in 1..=mx {
            for n in 1..=my {
                let natural = g.natural(m, n);
                if natural > top {
                    // Ω grows with n: the rest of this row is above too.
                    break;
                }
                let sigma = self.loss.sigma(natural);
                if sigma >= natural {
                    self.overdamped += 1;
                    continue;
                }
                self.modes.push(Mode { m, n, natural, sigma, drive: 0.0, pickups: [0.0; 2] });
            }
        }
        self.modes.sort_by(|a, b| a.natural.total_cmp(&b.natural).then(a.m.cmp(&b.m)));
        let (mx, my) = (mx as usize + 1, my as usize + 1);
        self.sines = Sines {
            x: [vec![0.0; mx], vec![0.0; mx], vec![0.0; mx]],
            y: [vec![0.0; my], vec![0.0; my], vec![0.0; my]],
        };
        for k in 0..3 {
            self.fill_sines(k);
        }
        self.lanes.allocate(self.modes.len());
        self.update_shapes();
    }

    /// Point `k`'s (0 drive, 1–2 pickups) sine tables.
    fn fill_sines(&mut self, k: usize) {
        let p = match k {
            0 => self.drive,
            _ => self.pickups[k - 1],
        };
        let pi = core::f64::consts::PI;
        for (i, s) in self.sines.x[k].iter_mut().enumerate() {
            *s = (i as f64 * pi * p.x).sin();
        }
        for (i, s) in self.sines.y[k].iter_mut().enumerate() {
            *s = (i as f64 * pi * p.y).sin();
        }
    }

    fn update_shapes(&mut self) {
        let norm = 2.0 / (self.geometry.width * self.geometry.height).sqrt();
        let s = &self.sines;
        for mode in &mut self.modes {
            let (m, n) = (mode.m as usize, mode.n as usize);
            let shape = |k: usize| norm * s.x[k][m] * s.y[k][n];
            mode.drive = shape(0);
            mode.pickups = [shape(1), shape(2)];
        }
        self.update_coefficients();
    }

    /// Rotations, weights and the pickups' calibration from the modes.
    fn update_coefficients(&mut self) {
        let t = 1.0 / f64::from(self.sample_rate);
        let mass = self.geometry.mass();
        let output = self.output;
        let mut energy = [0.0f64; 2];
        // The SI weights on (Re z, Im z); a mode a later loss overdamps
        // (it is not re-enumerated until `prepare`) falls silent.
        let weights = move |mode: &Mode, pickup: usize| -> (f64, f64) {
            let w = mode.damped();
            let s = mode.sigma;
            if w <= 0.0 {
                return (0.0, 0.0);
            }
            let a = t * mode.drive * mode.pickups[pickup] / (mass * w);
            match output {
                Output::Displacement => (0.0, a),
                Output::Velocity => (a * w, -a * s),
                Output::Acceleration => (-2.0 * a * s * w, a * (s * s - w * w)),
            }
        };
        for mode in &self.modes {
            let r2 = (-2.0 * mode.sigma * t).exp();
            for (p, e) in energy.iter_mut().enumerate() {
                let (wr, wi) = weights(mode, p);
                *e += (wr * wr + wi * wi) / (2.0 * (1.0 - r2));
            }
        }
        let scale = energy.map(|e| if e > 0.0 && e.is_finite() { 1.0 / e.sqrt() } else { 0.0 });
        for (i, mode) in self.modes.iter().enumerate() {
            let r = (-mode.sigma * t).exp();
            let (s, c) = (mode.damped() * t).sin_cos();
            self.lanes.set_rotation(i, (r * c) as f32, (r * s) as f32);
            let (lr, li) = weights(mode, 0);
            let (rr, ri) = weights(mode, 1);
            self.lanes.set_weights(
                i,
                [
                    (lr * scale[0]) as f32,
                    (li * scale[0]) as f32,
                    (rr * scale[1]) as f32,
                    (ri * scale[1]) as f32,
                ],
            );
        }
        self.scale = scale;
    }
}

impl Stage for Plate {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.build();
    }

    fn reset(&mut self) {
        self.lanes.reset();
    }

    /// The left pickup.
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let (mut l, mut r) = ([0.0f32], [0.0f32]);
        self.lanes.run::<false>(&[x], &mut l, &mut r);
        l[0]
    }

    /// The left pickup, block by block; bit-identical to `tick`.
    fn process(&mut self, block: &mut [f32]) {
        let mut input = [0.0f32; lanes::BLOCK];
        let mut unused = [0.0f32; 0];
        let mut at = 0;
        while at < block.len() {
            let n = self.lanes.room().min(block.len() - at);
            input[..n].copy_from_slice(&block[at..at + n]);
            self.lanes.run::<false>(&input[..n], &mut block[at..at + n], &mut unused);
            at += n;
        }
    }

    /// Two of the slowest mode's T60s (−120 dB).
    fn tail(&self) -> Tail {
        let slowest = self.modes.iter().map(|m| m.sigma).fold(f64::INFINITY, f64::min);
        if self.modes.is_empty() {
            Tail::None
        } else {
            Tail::from_time_constant(1.0 / slowest, self.sample_rate)
        }
    }
}

#[cfg(test)]
mod tests;
