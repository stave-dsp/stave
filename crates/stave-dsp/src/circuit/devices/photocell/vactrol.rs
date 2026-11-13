//! A running photocell: the cell's two carrier populations advanced one
//! sample at a time by TR-BDF2, its light history, and the LED driver.

use super::najnudel::{Cell, Najnudel};
use crate::circuit::devices::diode::Shockley;
use crate::circuit::methods::wright_omega::wright_omega;

/// TR-BDF2's split, 2 − √2: the trapezoidal stage covers γ·T, the BDF2
/// stage the rest; L-stable and second order (Bank et al. 1985).
const GAMMA: f64 = 2.0 - std::f64::consts::SQRT_2;
/// Newton iterations per stage, at most; two or three suffice at audio
/// rates (the residual is quadratic in the charges).
const ITERATIONS: u32 = 12;

/// A vactrol (or a bare LDR) running at a sample rate: LED current in,
/// cell resistance out.
///
/// # Models
/// A photoconductive cell (CdS) with its LED: resistance against light
/// (the part's transfer curve), the asymmetric attack and release
/// (milliseconds on, tens of milliseconds off), the program-dependent
/// release (slower after more light) and the slow light-history
/// adaptation (a cell kept dark is more sensitive). Parts:
/// [`Najnudel::VTL5C3`], [`Najnudel::NSL32`].
///
/// # Derivation
/// Najnudel, Müller, Hélie & Roze, "Power-Balanced Dynamic Modeling of
/// Vactrols: Application to a VTL5C3/2" (DAFx-23), eqs. 4, 7 and 8: two
/// carrier charges recombining through one defect level, the
/// photoresistance their conductance's inverse between the light and
/// dark resistances. Light is a carrier-generation (flow) source, as in
/// the homogeneous Iverson model the paper builds on, not the paper's
/// power-balanced coupling (which turns a dark cell on as √t where the
/// datasheets' cells rise roughly as t). Advanced by TR-BDF2 (L-stable,
/// second order) with Newton on each stage's 2×2 system; the LED's
/// current is held over each sample. The light history is a first-order
/// state of the light level ([`History`](super::History)). Parameters fitted to the
/// datasheets by `scripts/vactrol-fit.py`.
///
/// # Verified
/// `crates/stave-test/tests/photocell_matches_datasheet.rs` (resistance
/// against LED current, VTL5C3 dark- and light-adapted and NSL-32, the
/// response times, the LED's input characteristic) and
/// `crates/stave-dsp/src/circuit/devices/photocell/tests.rs::the_cell_converges_with_rate`
/// (TR-BDF2 against a step of a quarter the size, second order).
///
/// # Cost
/// Datasheet `"Vactrol (VTL5C3), 10 mA"` and `"Vactrol (VTL5C3), tremolo"`.
///
/// # Aliasing
/// None: the output is a resistance that moves at the cell's speed
/// (a few hundred hertz at most); a circuit it sets carries the audio.
///
/// # Valid ranges
/// LED current 0 to the part's maximum (40 mA VTL5C3, 25 mA NSL-32);
/// any sample rate from 8 kHz up (the cell's fastest rate, under a
/// millisecond, is resolved at every rate by the L-stable step).
#[derive(Clone, Debug)]
pub struct Vactrol {
    part: Najnudel,
    /// Hole and electron charge, coulombs (C± = 1 F).
    holes: f64,
    electrons: f64,
    /// 0 dark adapted, 1 light adapted.
    adapted: f64,
    step: f64,
    history_alpha: f64,
    ohms: f64,
    /// Newton iterations that hit the cap (reported, never silent).
    capped: u64,
}

impl Vactrol {
    /// Dark, dark adapted, prepared at 48 kHz.
    pub fn new(part: Najnudel) -> Self {
        let mut v = Self {
            part,
            holes: 0.0,
            electrons: 0.0,
            adapted: 0.0,
            step: 0.0,
            history_alpha: 0.0,
            ohms: part.cell.dark_ohms,
            capped: 0,
        };
        v.prepare(48_000.0);
        v
    }

    pub fn part(&self) -> &Najnudel {
        &self.part
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.step = 1.0 / f64::from(sample_rate);
        self.history_alpha = -(-self.step / self.part.history.seconds).exp_m1();
    }

    /// Dark (no carriers); the light history kept.
    pub fn reset(&mut self) {
        self.holes = 0.0;
        self.electrons = 0.0;
        self.ohms = self.part.cell.dark_ohms;
    }

    /// Set the light history: 0 for a cell stored dark (the datasheets'
    /// "dark adapted", 24 hours unlit), 1 for one lit for a day at the
    /// adapting level (VTL5C3: 40 mA).
    pub fn set_adaptation(&mut self, adapted: f64) {
        self.adapted = adapted.clamp(0.0, 1.0);
        self.ohms = self.resistance();
    }

    /// The light history, 0 (dark adapted) to 1 (light adapted).
    pub fn adaptation(&self) -> f64 {
        self.adapted
    }

    /// The cell at rest under `amps` of LED current, with the history
    /// held: what the datasheet's transfer curve plots.
    pub fn settle(&mut self, amps: f64) -> f64 {
        let generation = self.part.generation(amps);
        let [h, e] = self.part.cell.rest(generation);
        self.holes = h;
        self.electrons = e;
        self.ohms = self.resistance();
        self.ohms
    }

    /// The cell's resistance now.
    pub fn ohms(&self) -> f64 {
        self.ohms
    }

    /// Newton iterations that hit their cap so far.
    pub fn capped(&self) -> u64 {
        self.capped
    }

    /// One sample with `amps` through the LED (negative is dark: the LED
    /// blocks); returns the cell's resistance.
    pub fn step(&mut self, amps: f64) -> f64 {
        let generation = self.part.generation(amps.max(0.0));
        self.illuminate(generation)
    }

    /// One sample of a bare cell lit by `generation` amperes of
    /// photo-generated carriers (the part's coupling maps LED current to
    /// it; any light source can); returns the resistance.
    pub fn illuminate(&mut self, generation: f64) -> f64 {
        let generation = generation.max(0.0);
        let cell = self.part.cell;
        let h = self.step;
        let y0 = [self.holes, self.electrons];
        let f0 = cell.rates(y0, generation);
        // Trapezoid over γh: y − (γh/2)·f(y) = y0 + (γh/2)·f(y0).
        let c1 = 0.5 * GAMMA * h;
        let r1 = [y0[0] + c1 * f0[0], y0[1] + c1 * f0[1]];
        let yg = self.implicit(&cell, generation, c1, r1, y0);
        // BDF2 to h: y − c2·f(y) = a·yγ − b·y0.
        let a = 1.0 / (GAMMA * (2.0 - GAMMA));
        let b = (1.0 - GAMMA).powi(2) / (GAMMA * (2.0 - GAMMA));
        let c2 = (1.0 - GAMMA) / (2.0 - GAMMA) * h;
        let r2 = [a * yg[0] - b * y0[0], a * yg[1] - b * y0[1]];
        let y1 = self.implicit(&cell, generation, c2, r2, yg);
        self.holes = y1[0];
        self.electrons = y1[1];
        // The light history drifts toward the light it sees.
        let target = generation / (generation + self.part.history.reference_generation);
        self.adapted += self.history_alpha * (target - self.adapted);
        self.ohms = self.resistance();
        self.ohms
    }

    /// Solve `y − c·f(y) = r` by Newton from `guess`, charges kept
    /// non-negative.
    fn implicit(&mut self, cell: &Cell, generation: f64, c: f64, r: [f64; 2], guess: [f64; 2]) -> [f64; 2] {
        let mut y = guess;
        for i in 0..ITERATIONS {
            let f = cell.rates(y, generation);
            let j = cell.jacobian(y);
            let g = [y[0] - c * f[0] - r[0], y[1] - c * f[1] - r[1]];
            // A = I − c·J
            let (a00, a01, a10, a11) = (1.0 - c * j[0][0], -c * j[0][1], -c * j[1][0], 1.0 - c * j[1][1]);
            let det = a00 * a11 - a01 * a10;
            let d0 = (a11 * g[0] - a01 * g[1]) / det;
            let d1 = (a00 * g[1] - a10 * g[0]) / det;
            let next = [(y[0] - d0).max(0.0), (y[1] - d1).max(0.0)];
            let scale = 1e-30 + next[0].abs().max(next[1].abs());
            let moved = (next[0] - y[0]).abs().max((next[1] - y[1]).abs());
            y = next;
            if moved <= 1e-13 * scale || !det.is_finite() {
                return y;
            }
            if i + 1 == ITERATIONS {
                self.capped += 1;
            }
        }
        y
    }

    /// The resistance of the present carriers under the light history.
    fn resistance(&self) -> f64 {
        let cell = &self.part.cell;
        let g = cell.photoconductance(self.holes, self.electrons);
        let ratio = self.part.history.ratio(g).powf(self.adapted);
        cell.ohms(g / ratio)
    }
}

/// The LED's electrical side: a drive voltage through `series_ohms`
/// into the LED's junction and bulk resistance, solved in closed form
/// (Wright omega), so a control voltage becomes the LED current a
/// [`Vactrol`] takes.
///
/// ```
/// use stave_dsp::circuit::devices::photocell::{LedDriver, Najnudel};
///
/// // 5 V through 160 ohm into the VTL5C3's LED: about 20 mA.
/// let driver = LedDriver::new(Najnudel::VTL5C3.led, 160.0);
/// let amps = driver.amps(5.0);
/// assert!((0.019..0.022).contains(&amps), "{amps}");
/// assert_eq!(driver.amps(-5.0), 0.0); // reverse: the LED blocks
/// ```
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct LedDriver {
    pub led: Shockley,
    pub series_ohms: f64,
}

impl LedDriver {
    pub fn new(led: Shockley, series_ohms: f64) -> Self {
        Self { led, series_ohms }
    }

    /// The LED current at `volts` of drive: `v = i·R + n·Vt·ln(1 + i/Is)`,
    /// `R` the series resistor and the LED's bulk resistance. Reverse
    /// drive gives none (breakdown is not modelled here).
    pub fn amps(&self, volts: f64) -> f64 {
        let r = self.series_ohms + self.led.series_ohms;
        let is = self.led.saturation_amps;
        let nvt = self.led.emission * self.led.thermal_volts;
        if volts <= 0.0 || r <= 0.0 {
            return if volts > 0.0 { is * (volts / nvt).exp_m1() } else { 0.0 };
        }
        // (i + Is)·R/(n·Vt) = ω((v + Is·R)/(n·Vt) + ln(Is·R/(n·Vt)))
        let x = (volts + is * r) / nvt + (is * r / nvt).ln();
        (nvt / r * wright_omega(x) - is).max(0.0)
    }
}
