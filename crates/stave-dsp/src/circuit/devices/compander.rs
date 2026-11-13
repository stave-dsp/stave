//! The NE570-family compandor: a full-wave averaging rectifier driving a
//! ΔG gain cell, wired as a 2:1 compressor (the cell in an op-amp's
//! feedback, the rectifier on the output) or a 1:2 expander (the cell in
//! the signal path, the rectifier on the input). Analog delays put the
//! compressor before the BBD and the expander after it, so the line's
//! noise falls by the compression in dB.

use crate::kernel::denormal;
use crate::kernel::node::{Stage, Tail};
use crate::units::{Db, Seconds, Volts};

/// A compandor half's parameters, as its datasheet gives them.
///
/// `unity` is the RMS level of a sine at which the half has unity gain;
/// `rectifier` is the averaging time constant (`10 kΩ · C_RECT` on the
/// NE570); `range` bounds the compressor's gain below unity level (the
/// chip's dynamic range, set by its offsets).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Chip {
    pub unity: Volts,
    pub rectifier: Seconds,
    pub range: Db,
}

impl Chip {
    /// NE570 (Signetics/Philips; ON Semi NE570/D): unity gain at 0 dBm
    /// (775 mV RMS) with the internal resistors, the rectifier's 10 kΩ
    /// with the datasheet's 1 µF (10 ms), and 80 dB of range below
    /// unity level.
    pub const NE570: Chip = Chip {
        unity: Volts::new(0.775),
        rectifier: Seconds::new(0.010),
        range: Db::new(80.0),
    };
}

/// Which half of the compandor.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    /// 2:1 in dB about unity level: the rectifier reads the output.
    Compress,
    /// 1:2 in dB about unity level: the rectifier reads the input.
    Expand,
}

/// The mean of a full-wave rectified sine, `2√2/π`, times its RMS.
const AVERAGE_OF_RMS: f64 = 2.0 * core::f64::consts::SQRT_2 / core::f64::consts::PI;

/// One half of an NE570 compandor: a gain cell driven by a full-wave
/// averaging rectifier.
///
/// # Models
/// The NE570/571's ΔG cell, whose gain is proportional to the rectifier's
/// averaged current (datasheet block diagram and gain equations): as an
/// expander, `G = avg|x| / avg_unity`; as a compressor, the cell in the
/// feedback of the output amplifier, `G = avg_unity / avg|y|`, so in steady
/// state `avg|y|² = avg_unity·avg|x|` — the 2:1 law. The rectifier is the
/// datasheet's averaging filter, `10 kΩ · C_RECT`.
///
/// # Derivation
/// The rectifier a one-pole average of `|·|` at the time constant
/// (impulse-invariant); the compressor's loop closed on the previous
/// sample's rectifier state (the cell responds to the rectifier current,
/// which the averaging capacitor holds); levels in volts, `avg_unity` the
/// full-wave mean of a sine at `unity` RMS.
///
/// # Verified
/// `crates/stave-dsp/src/circuit/devices/compander/tests.rs::compressor_halves_the_level_in_db`,
/// `crates/stave-dsp/src/circuit/devices/compander/tests.rs::expander_doubles_the_level_in_db` and
/// `crates/stave-dsp/src/circuit/devices/compander/tests.rs::compressor_then_expander_is_unity`:
/// steady-state 1 kHz sines from −60 to +10 dB re unity level land on
/// the datasheet's gain law within 0.05 dB (compress 2:1, expand 1:2),
/// and the pair in series returns the input level within 0.05 dB; the
/// rectifier's attack reaches 63 % of a level step in one time constant
/// within 5 %
/// (`crates/stave-dsp/src/circuit/devices/compander/tests.rs::rectifier_settles_in_its_time_constant`).
///
/// # Cost
/// `"Compander (NE570)"` on the datasheet.
///
/// # Aliasing
/// The gain moves at the rectifier's rate, so the ripple it puts on a
/// low tone (the chip's own distortion) sits at harmonics of the input,
/// well below Nyquist for the default 10 ms; no oversampling expected.
///
/// # Valid ranges
/// Levels in volts; compressor gain bounded by `range` below unity level;
/// rectifier time constants of 1 ms up (shorter ones put audible ripple
/// on the gain, as on the chip).
#[derive(Clone, Debug)]
pub struct Compander {
    chip: Chip,
    mode: Mode,
    sample_rate: f32,
    alpha: f64,
    avg_unity: f64,
    floor: f64,
    average: f64,
}

impl Compander {
    /// A half of `chip` in `mode`, settled at unity level.
    pub fn new(chip: Chip, mode: Mode) -> Self {
        let mut c = Self {
            chip,
            mode,
            sample_rate: 48_000.0,
            alpha: 0.0,
            avg_unity: 0.0,
            floor: 0.0,
            average: 0.0,
        };
        c.update();
        c.reset();
        c
    }

    /// The parameters, for inspection.
    pub fn chip(&self) -> &Chip {
        &self.chip
    }

    /// The half.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    fn update(&mut self) {
        let tau = self.chip.rectifier.get().max(1e-6);
        self.alpha = 1.0 - (-1.0 / (tau * f64::from(self.sample_rate))).exp();
        self.avg_unity = self.chip.unity.get() * AVERAGE_OF_RMS;
        self.floor = self.avg_unity * 10f64.powf(-f64::from(self.chip.range.get()) / 20.0);
    }

    /// The cell's present gain.
    pub fn gain(&self) -> f64 {
        match self.mode {
            Mode::Compress => self.avg_unity / self.average.max(self.floor),
            Mode::Expand => self.average / self.avg_unity,
        }
    }

    /// One sample in `f64`, volts in and out.
    #[inline]
    pub fn tick_f64(&mut self, x: f64) -> f64 {
        let (y, sensed) = match self.mode {
            Mode::Compress => {
                let y = x * self.gain();
                (y, y)
            }
            Mode::Expand => {
                // The rectifier sees the input first: the cell follows it.
                let a = self.average + (x.abs() - self.average) * self.alpha;
                (x * a / self.avg_unity, x)
            }
        };
        self.average = denormal::flush64(self.average + (sensed.abs() - self.average) * self.alpha);
        y
    }
}

impl Stage for Compander {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.update();
        self.reset();
    }

    fn reset(&mut self) {
        // Settled at silence: the compressor at its full gain, the
        // expander closed.
        self.average = match self.mode {
            Mode::Compress => self.floor,
            Mode::Expand => 0.0,
        };
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.tick_f64(f64::from(x)) as f32
    }

    fn tail(&self) -> Tail {
        // The cell only scales: silence in is silence out.
        Tail::None
    }
}

#[cfg(test)]
#[path = "compander/tests.rs"]
mod tests;
