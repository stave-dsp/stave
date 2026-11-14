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
/// chip's dynamic range, set by its offsets). The rest is where the part
/// stops being linear: `cell` is the peak input at which the ΔG cell
/// saturates (its input current `V/R2` reaching the cell's bias current
/// `I1`), `rectifier_limit` the peak input at which the rectifier does
/// (its input current `V/R1` at the sheet's limit), `swing` the output
/// amplifier's peak swing about its operating point and `slew_rate` its
/// slew rate, volts per second.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Chip {
    pub unity: Volts,
    pub rectifier: Seconds,
    pub range: Db,
    pub cell: Volts,
    pub rectifier_limit: Volts,
    pub swing: Volts,
    pub slew_rate: f64,
}

impl Chip {
    /// NE570 (onsemi NE570/D, Rev. 4, May 2006): unity gain level 0 dBm
    /// (775 mV RMS, ±1 dB) with the internal resistors; the rectifier's
    /// `τ = 10 kΩ · C_RECT` with the 2.2 µF of the sheet's Figure 3 test
    /// circuit (22 ms); 80 dB of range below unity level, the −80 dB end
    /// of the input range its Figure 2 transfer curve shows (+20 to
    /// −80 dB; "greater than 110 dB" of dynamic range overall).
    ///
    /// Its limits, at the sheet's `VCC` = 15 V: the ΔG cell saturates at
    /// +8.0 dBm (page 7; Figure 15's maximum signal level reads +7.75 dBm
    /// at 0 dB of gain) — its bias current `I1` = 140 µA through
    /// `R2` = 20 kΩ, 2.8 V peak. The rectifier saturates past 250 µA into
    /// `R1` = 10 kΩ (page 6): 2.5 V peak. The output amplifier is
    /// "equivalent to a 741" (page 8): the table's 0.5 V/µs output slew
    /// rate and, biased mid-supply (as the compressor's `R_DC` sets it),
    /// the LM741's 1 V short of each rail: ±6.5 V. The sheet's +13 dBm
    /// (3.5 V RMS, 4.95 V peak) into 300 Ω at its 20 mA sits inside that.
    pub const NE570: Chip = Chip {
        unity: Volts::new(0.775),
        rectifier: Seconds::new(0.022),
        range: Db::new(80.0),
        cell: Volts::new(2.8),
        rectifier_limit: Volts::new(2.5),
        swing: Volts::new(6.5),
        slew_rate: 0.5e6,
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
/// averaging rectifier, and the output amplifier around them.
///
/// # Models
/// The NE570/571's ΔG cell, whose gain is proportional to the rectifier's
/// averaged current (datasheet block diagram and gain equations): as an
/// expander, `G = avg|x| / avg_unity`; as a compressor, the cell in the
/// feedback of the output amplifier, `G = avg_unity / avg|y|`, so in steady
/// state `avg|y|² = avg_unity·avg|x|` — the 2:1 law. The rectifier is the
/// datasheet's averaging filter, `10 kΩ · C_RECT`.
///
/// Where the part stops being linear (the sheet's figures, [`Chip`]):
/// the cell is a two-quadrant multiplier, `I_out = I_G·I_in/I1`, whose
/// input current cannot exceed its bias, so its output saturates at
/// `I_G` once its input passes `cell` (Figure 15's maximum signal level,
/// `cell` times the gain); the rectifier's current saturates past
/// `rectifier_limit`; and the op-amp swings `±swing` at most, at
/// `slew_rate`. In the compressor the cell is the feedback: when the
/// output it would need passes `cell` — a hot onset after silence, with
/// the gain still at its 80 dB maximum — the loop opens and the output
/// slews to the rail in the input's direction, until the rectifier
/// (charging at its saturated current) has brought the gain down. That
/// is the chip's attack overshoot from rest, bounded by its swing.
///
/// # Derivation
/// The rectifier a one-pole average of `min(|·|, rectifier_limit)` at
/// the time constant (impulse-invariant); the compressor's loop closed
/// on the previous sample's rectifier state (the cell responds to the
/// rectifier current, which the averaging capacitor holds), its cell
/// saturation testing the output the loop would need, `G·x`, against
/// `cell`; the expander's cell input clamped to `±cell`; the output
/// clamped to `±swing` and moved at most `slew_rate/fs` a sample. Levels
/// in volts about the operating point, `avg_unity` the full-wave mean of
/// a sine at `unity` RMS.
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
/// (`crates/stave-dsp/src/circuit/devices/compander/tests.rs::rectifier_settles_in_its_time_constant`);
/// a hot onset after silence stays inside the swing at every rate from
/// 8 to 768 kHz, sits on the rail while the rectifier charges, for the
/// time its saturated current takes to bring the gain to `cell/x`
/// (`crates/stave-dsp/src/circuit/devices/compander/tests.rs::a_hot_onset_rides_the_rail_until_the_rectifier_catches_up`).
/// Against the sheet (`crates/stave-test/datasheet/ne570_figures.txt`,
/// `ne570_max_signal.txt`):
/// `crates/stave-test/tests/matches_datasheet.rs::compandors::ne570` —
/// the output's slew rate, its swing against the +13 dBm figure, and
/// Figure 15's maximum signal level against the gain, −40 to +5 dB.
///
/// # Cost
/// `"Compander (NE570)"` on the datasheet.
///
/// # Aliasing
/// The gain moves at the rectifier's rate, so the ripple it puts on a
/// low tone (the chip's own distortion) sits at harmonics of the input,
/// well below Nyquist for the default 22 ms; no oversampling expected.
/// Driven past `cell` or `swing` the output clips hard, as the chip
/// does, and that aliases at the host rate.
///
/// # Valid ranges
/// Levels in volts about the operating point; compressor gain bounded by
/// `range` below unity level; rectifier time constants of 1 ms up
/// (shorter ones put audible ripple on the gain, as on the chip). The
/// op-amp's gain-bandwidth (the 741's 1 MHz, which caps the
/// compressor's gain near 60 dB at 1 kHz), its load and the input
/// coupling capacitors are not modelled: a DC input holds its level.
#[derive(Clone, Debug)]
pub struct Compander {
    chip: Chip,
    mode: Mode,
    sample_rate: f32,
    alpha: f64,
    avg_unity: f64,
    floor: f64,
    slew: f64,
    average: f64,
    out: f64,
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
            slew: 0.0,
            average: 0.0,
            out: 0.0,
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
        let rate = f64::from(self.sample_rate);
        self.alpha = 1.0 - (-1.0 / (tau * rate)).exp();
        self.avg_unity = self.chip.unity.get() * AVERAGE_OF_RMS;
        self.floor = self.avg_unity * 10f64.powf(-f64::from(self.chip.range.get()) / 20.0);
        self.slew = self.chip.slew_rate / rate;
    }

    /// The cell's present gain.
    pub fn gain(&self) -> f64 {
        match self.mode {
            Mode::Compress => self.avg_unity / self.average.max(self.floor),
            Mode::Expand => self.average / self.avg_unity,
        }
    }

    /// The ΔG cell alone as a VCA at `gain`: its input saturating at
    /// `±cell`, as the sheet's Figure 15 draws its maximum signal level
    /// (the output amplifier's swing and slew are not applied).
    #[inline]
    pub fn vca(&self, x: f64, gain: f64) -> f64 {
        let cell = self.chip.cell.get();
        x.clamp(-cell, cell) * gain
    }

    /// The output amplifier: `target` within its swing, reached at most
    /// one slew step from the last output.
    #[inline]
    fn amplifier(&mut self, target: f64) -> f64 {
        let swing = self.chip.swing.get();
        let target = target.clamp(-swing, swing);
        self.out = self.out + (target - self.out).clamp(-self.slew, self.slew);
        self.out
    }

    /// One sample in `f64`, volts in and out.
    #[inline]
    pub fn tick_f64(&mut self, x: f64) -> f64 {
        let cell = self.chip.cell.get();
        let limit = self.chip.rectifier_limit.get();
        let (y, sensed) = match self.mode {
            Mode::Compress => {
                // The output the loop needs; past the cell's saturation
                // the feedback cannot supply it, and the op-amp runs to
                // its rail.
                let needed = x * self.gain();
                let target = if needed.abs() <= cell {
                    needed
                } else {
                    self.chip.swing.get().copysign(x)
                };
                let y = self.amplifier(target);
                (y, y)
            }
            Mode::Expand => {
                // The rectifier sees the input first: the cell follows it.
                let a = self.average + (x.abs().min(limit) - self.average) * self.alpha;
                let y = self.amplifier(self.vca(x, a / self.avg_unity));
                (y, x)
            }
        };
        self.average =
            denormal::flush64(self.average + (sensed.abs().min(limit) - self.average) * self.alpha);
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
        self.out = 0.0;
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.tick_f64(f64::from(x)) as f32
    }

    fn tail(&self) -> Tail {
        // The cell only scales; the output slews back from a rail.
        Tail::samples((2.0 * self.chip.swing.get() / self.slew.max(1e-12)).ceil() as u32)
    }
}

#[cfg(test)]
#[path = "compander/tests.rs"]
mod tests;
