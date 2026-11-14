//! Bucket-brigade delay lines: a chain of capacitors that a two-phase
//! clock passes charge along, one bucket per clock edge, so the delay is
//! `stages / (2 · clock)` and the signal leaves as a staircase at the
//! clock rate.
//!
//! The chip is a circuit claim — a schematic part with a datasheet — so
//! it lives in `circuit`, beside the netlist devices, though no netlist
//! method can solve it: its clocked transfer is a sampling process, not
//! a differential equation. [`HoltersParker`] runs it as Holters and
//! Parker do (DAFx-18): the line as a fixed number of buckets at the
//! (varying) clock rate, with the resampling between host rate and clock
//! done by the chip's own anti-alias and reconstruction filters, held as
//! partial fractions ([`Filter`]) of their circuits. Parts are
//! constants of [`Chip`]: [`Chip::MN3007`], [`Chip::MN3008`],
//! [`Chip::MN3205`]. The full application circuit, with an NE570
//! compander around the line, is
//! [`topologies::bucket_brigade`](crate::circuit::topologies::bucket_brigade).

mod filter;
mod holters_parker;

pub use filter::{Filter, SallenKey, MAX_POLES};
pub use holters_parker::HoltersParker;

use crate::units::{Db, Hz, Volts};
use crate::units::Seconds;

/// A bucket-brigade chip's parameters, as its datasheet gives them.
///
/// `stages` is the number of buckets (the delay is `stages / (2·clock)`);
/// the clock range is the datasheet's recommended one; `nominal` is the
/// level (RMS) the signal-to-noise figure is taken against, and
/// `signal_to_noise` the sheet's typical S/N.
///
/// The Panasonic sheets do not say what level their S/N is quoted at.
/// Taken against each sheet's input signal swing (the level for 2.5%
/// THD), the noise comes to 150–160 µV, which is what the MN3007's and
/// MN3205's `Vno–fcp` curves show (about −73 and −71 dBm, A-weighted, at
/// a 100 kHz clock) and under every sheet's `Vno` maximum; against the
/// THD test level it would be a third of that, below both curves. So
/// `nominal` is the input swing.
///
/// `feedthrough` is the clock's residue at the output pin. The sheets do
/// not tabulate it (they claim only "clock component cancellation
/// capability" between the two outputs): the 3 mV default is an
/// estimate, to be set from a measurement of a given unit.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Chip {
    pub stages: u32,
    pub clock_min: Hz,
    pub clock_max: Hz,
    pub nominal: Volts,
    pub signal_to_noise: Db,
    pub feedthrough: Volts,
}

impl Chip {
    /// Panasonic MN3007 data sheet (MN3000 series, pp. 34–36): 1024
    /// stages, 10–100 kHz clock (5.12–51.2 ms); S/N 80 dB typical, input
    /// signal swing 1.5 V RMS (fcp 40 kHz, 2.5% THD), noise ≤ 0.3 mV RMS
    /// A-weighted at fcp 100 kHz. Chorus and flanger (the Boss CE-2).
    pub const MN3007: Chip = Chip {
        stages: 1024,
        clock_min: Hz::new(10_000.0),
        clock_max: Hz::new(100_000.0),
        nominal: Volts::new(1.5),
        signal_to_noise: Db::new(80.0),
        feedthrough: Volts::new(0.003),
    };

    /// Panasonic MN3008 data sheet (MN3000 series, pp. 38–40): 2048
    /// stages, 10–100 kHz clock (10.24–102.4 ms); S/N 78 dB typical,
    /// input signal swing 1.2 V RMS (fcp 40 kHz, 2.5% THD), noise ≤ 0.4 mV
    /// RMS A-weighted at fcp 100 kHz. Short echo and doubling.
    pub const MN3008: Chip = Chip {
        stages: 2048,
        clock_min: Hz::new(10_000.0),
        clock_max: Hz::new(100_000.0),
        nominal: Volts::new(1.2),
        signal_to_noise: Db::new(78.0),
        feedthrough: Volts::new(0.003),
    };

    /// Panasonic MN3205 data sheet (MN3200 series, pp. 70–72): 4096
    /// stages on a 5 V supply, 10–100 kHz clock (20.48–204.8 ms); S/N
    /// 67 dB typical, input signal swing 0.36 V RMS (2.5% THD), output
    /// noise ≤ 0.35 mV RMS A-weighted at fcp 100 kHz. Analog echo (Boss
    /// DM-3 and later delays).
    pub const MN3205: Chip = Chip {
        stages: 4096,
        clock_min: Hz::new(10_000.0),
        clock_max: Hz::new(100_000.0),
        nominal: Volts::new(0.36),
        signal_to_noise: Db::new(67.0),
        feedthrough: Volts::new(0.003),
    };

    /// The delay at `clock`, `stages / (2·clock)`.
    pub fn delay(&self, clock: Hz) -> Seconds {
        Seconds::new(f64::from(self.stages) / (2.0 * f64::from(clock.get())))
    }

    /// The clock for a delay of `seconds`, `stages / (2·seconds)`.
    pub fn clock_for(&self, delay: Seconds) -> Hz {
        let seconds = delay.get();
        Hz::new((f64::from(self.stages) / (2.0 * seconds)) as f32)
    }

    /// The noise at the output pin, RMS volts: `nominal` less the S/N.
    pub fn noise_volts(&self) -> f64 {
        self.nominal.get() * 10f64.powf(-f64::from(self.signal_to_noise.get()) / 20.0)
    }
}

#[cfg(test)]
mod tests;
