//! An analog BBD delay as built: an NE570 compressor, a fourth-order
//! Sallen–Key anti-alias filter, the bucket-brigade chip, a fourth-order
//! Sallen–Key reconstruction filter and the NE570's expander — the
//! signal path of a compandored chorus, flanger or echo, without the
//! dry/wet mix and regeneration a pedal wraps around it.
//!
//! The filters are data: [`INPUT_FILTER`] and [`OUTPUT_FILTER`] are the
//! part values of `crates/stave-test/spice/bbd_input_filter.cir` and
//! `bbd_output_filter.cir`, which ngspice and `Circuit<Dk>` both solve.

use crate::circuit::devices::bbd::{Chip, Filter, HoltersParker, SallenKey};
use crate::circuit::devices::compander::{self, Compander, Mode};
use crate::kernel::node::{Stage, Tail};
use crate::units::{kohm, nf, pf, Hz, Seconds};

/// The anti-alias filter: two buffered unity-gain Sallen–Key sections
/// on E12 parts, a fourth-order Butterworth-like lowpass at about 8 kHz
/// (−1.9 dB at 8 kHz, −31 dB at 20 kHz).
pub const INPUT_FILTER: [SallenKey; 2] = [
    SallenKey { r1: kohm(10.0), r2: kohm(10.0), c1: nf(2.2), c2: nf(1.8) },
    SallenKey { r1: kohm(10.0), r2: kohm(10.0), c1: nf(5.6), c2: pf(680.0) },
];

/// The reconstruction filter: the same circuit about 6.6 kHz (−3 dB at
/// 7 kHz, −38 dB at 20 kHz), lower to bury the clock's images.
pub const OUTPUT_FILTER: [SallenKey; 2] = [
    SallenKey { r1: kohm(10.0), r2: kohm(10.0), c1: nf(2.7), c2: nf(2.2) },
    SallenKey { r1: kohm(10.0), r2: kohm(10.0), c1: nf(6.8), c2: pf(820.0) },
];

/// A compandored bucket-brigade delay line: compressor, anti-alias
/// filter, BBD, reconstruction filter, expander.
///
/// # Models
/// The signal path of a compandored analog delay (the arrangement of the
/// Boss DM-2/DM-3 and CE-2 class, on generic parts): an NE570 compressor
/// ([`Compander`]), the BBD ([`Chip`], MN3007 by default) between the
/// fourth-order Sallen–Key filters [`INPUT_FILTER`] and
/// [`OUTPUT_FILTER`] ([`HoltersParker`]), and the NE570's expander.
/// Wet only: mix and regeneration are the effect's.
///
/// # Derivation
/// The filters' partial fractions from their part values
/// ([`Filter::sallen_key`]), the same parts as the ngspice decks; the
/// line and compander as their own models document. Levels in volts.
///
/// # Verified
/// `crates/stave-test/tests/bbd_matches_ngspice.rs::filters_match_ngspice`
/// (each filter's partial fractions against ngspice `.ac` on its deck,
/// within 1e-6 dB, 20 Hz–100 kHz, and `Circuit<Dk>` on the same deck
/// within 1e-6 dB),
/// `crates/stave-dsp/src/circuit/devices/bbd/tests.rs::compander_tracks_through_the_line`
/// (with the compander, a 1 kHz tone from −40 to 0 dB re unity level
/// leaves at the line's gain squared — the expander doubles the line's
/// loss in dB — within 0.05 dB; measured 0.021) and
/// `crates/stave-dsp/src/circuit/devices/bbd/tests.rs::compander_lowers_the_line_noise`
/// (idle noise falls by more than 60 dB with the compander in: −91 dBV
/// to below −170).
///
/// # Cost
/// `"BucketBrigade (MN3007, NE570, 40 kHz clock)"` on the datasheet.
///
/// # Aliasing
/// The chip's own (see [`HoltersParker`]); the compander's gain moves
/// at its rectifier's rate.
///
/// # Valid ranges
/// Clock within the chip's range; levels in volts, about the NE570's
/// 775 mV unity level; host rates from 44.1 kHz.
#[derive(Clone, Debug)]
pub struct BucketBrigade {
    compressor: Compander,
    line: HoltersParker,
    expander: Compander,
    compander: bool,
}

impl Default for BucketBrigade {
    fn default() -> Self {
        Self::new(Chip::MN3007)
    }
}

impl BucketBrigade {
    /// `chip` between this circuit's filters, with an NE570 compander.
    pub fn new(chip: Chip) -> Self {
        let input = Filter::sallen_key(&INPUT_FILTER).unwrap_or(Filter::JUNO_60_INPUT);
        let output = Filter::sallen_key(&OUTPUT_FILTER).unwrap_or(Filter::JUNO_60_OUTPUT);
        Self {
            compressor: Compander::new(compander::Chip::NE570, Mode::Compress),
            line: HoltersParker::new(chip, input, output),
            expander: Compander::new(compander::Chip::NE570, Mode::Expand),
            compander: true,
        }
    }

    /// The line, for its clock, feedthrough and noise.
    pub fn line(&mut self) -> &mut HoltersParker {
        &mut self.line
    }

    /// The BBD clock (clamped to the chip's range).
    pub fn set_clock(&mut self, clock: Hz) {
        self.line.set_clock(clock);
    }

    /// The delay (sets the clock to `stages / (2·delay)`, clamped).
    pub fn set_delay(&mut self, delay: Seconds) {
        self.line.set_delay(delay);
    }

    /// Whether the compander is in circuit.
    pub fn set_compander(&mut self, on: bool) {
        self.compander = on;
    }

    /// One sample in `f64`, volts in and out.
    #[inline]
    pub fn tick_f64(&mut self, x: f64) -> f64 {
        if self.compander {
            let c = self.compressor.tick_f64(x);
            let d = self.line.tick_f64(c);
            self.expander.tick_f64(d)
        } else {
            self.line.tick_f64(x)
        }
    }
}

impl Stage for BucketBrigade {
    fn prepare(&mut self, sample_rate: f32) {
        self.compressor.prepare(sample_rate);
        self.line.prepare(sample_rate);
        self.expander.prepare(sample_rate);
    }

    fn reset(&mut self) {
        self.compressor.reset();
        self.line.reset();
        self.expander.reset();
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.tick_f64(f64::from(x)) as f32
    }

    fn tail(&self) -> Tail {
        self.line.tail()
    }
}
