//! The operational transconductance amplifier family: the [`Ota`] trait
//! and its model, [`BipolarPair`], with parts as constants
//! ([`BipolarPair::CA3080`], [`BipolarPair::LM13700`]).
//!
//! A netlist places one with
//! [`ElementKind::Ota`](crate::circuit::netlist::ElementKind::Ota): an
//! output current into the output node, and the bias pin a junction to
//! the negative supply whose current the chip mirrors as `Iabc`, so a
//! resistor from a control voltage into the pin sets the gain exactly as
//! on a schematic.

#[cfg(test)]
mod tests;

use crate::circuit::devices::diode::THERMAL_VOLTS_27C;

/// The output current, the bias pin's current, and their partials, at a
/// differential input `v(+) − v(−)` and a bias-pin voltage (to the
/// negative supply).
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct OtaCurrents {
    /// Into the output node.
    pub output: f64,
    /// Into the bias pin (the mirrored `Iabc`).
    pub bias: f64,
    pub output_per_input_volt: f64,
    pub output_per_bias_volt: f64,
    pub bias_per_bias_volt: f64,
}

/// The OTA family: an output current set by the differential input and
/// the bias current. Every OTA model implements it.
pub trait Ota: Copy {
    fn currents(&self, input: f64, bias_volts: f64) -> OtaCurrents;

    /// Bound a Newton step on the bias pin's junction voltage (SPICE's
    /// `pnjlim`).
    fn limit_bias_step(&self, new: f64, old: f64) -> f64;
}

/// An OTA whose input stage is an ideal bipolar differential pair biased
/// by a mirrored `Iabc`: `Iout = Iabc·tanh((v₊ − v₋ + Vos)/(2·Vt))`.
///
/// # Models
///
/// The CA3080 and the LM13700's amplifier (without its linearising
/// diodes or output buffer): the pair's tanh transfer, `gm = Iabc/(2·Vt)`
/// at the origin, and the bias pin as the mirror's input junctions
/// (one diode drop above the negative supply for the CA3080, two for the
/// LM13700). Output resistance, output voltage compliance and the
/// bandwidth (about 2 MHz) are not modelled; the output is an ideal
/// current source.
///
/// # Derivation
///
/// The differential pair (Gray & Meyer, *Analysis and Design of Analog
/// Integrated Circuits*, §3.5): `Iout = Iabc·tanh(vd/(2·Vt))`. The bias
/// pin is `junctions` series junctions of saturation current
/// `bias_saturation_amps`, fitted to the datasheet's amplifier bias
/// voltage at `Iabc` = 500 µA, and the output mirrors the pin's current
/// (ratio 1):
///
/// ```text
///     Iabc = Is·(exp(Vabc/(junctions·Vt)) − 1)
/// ```
///
/// # Verified
///
/// `crates/stave-test/tests/dk_matches_ngspice.rs::ota_transfer_matches_ngspice`
/// (the CA3080's output current across ±100 mV at about 10, 100 and
/// 500 µA: normalized RMS 2.6e-8, budget 1e-7) and
/// `crates/stave-test/tests/dk_matches_ngspice.rs::ota_vca_matches_ngspice`
/// (an LM13700 VCA under a control sweep: 4.1e-6 at 4×, budget 1e-5),
/// each the element against ngspice's behavioural form of the same
/// equations;
/// `crates/stave-dsp/src/circuit/devices/ota/tests.rs::the_bias_pin_sits_where_the_datasheet_says`
/// (`gm` within 1% of the datasheets' 9600 µS at 500 µA).
///
/// # Cost
///
/// The datasheet's `"Circuit<Dk> OTA VCA (LM13700) x15"`: two controls in
/// the DK solve.
///
/// # Aliasing
///
/// The tanh is smooth but saturating: an OTA driven past ±50 mV makes
/// harmonics, so run it oversampled when the input is hot.
///
/// # Valid ranges
///
/// Bias currents up to the datasheets' 2 mA maximum; the output within
/// its supply (no compliance limit is modelled).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct BipolarPair {
    pub name: &'static str,
    /// The bias pin's junctions' saturation current, amps.
    pub bias_saturation_amps: f64,
    /// How many junctions the bias pin sits above the negative supply.
    pub junctions: f64,
    /// Input offset voltage, added to `v(+) − v(−)`.
    pub offset_volts: f64,
    pub thermal_volts: f64,
}

impl BipolarPair {
    /// RCA/Intersil CA3080, datasheet FN475.5 (April 2001), typical at
    /// ±15 V, 25 °C: amplifier bias voltage 0.71 V at `Iabc` = 500 µA (one
    /// junction, saturation current 500 µA·exp(−0.71/Vt)), input offset
    /// 0.4 mV; gm 9600 µS at 500 µA (`gm = Iabc/(2·Vt)` gives 9666).
    pub const CA3080: Self = Self {
        name: "CA3080",
        bias_saturation_amps: 5.990_399e-16,
        junctions: 1.0,
        offset_volts: 0.4e-3,
        thermal_volts: THERMAL_VOLTS_27C,
    };

    /// Texas Instruments LM13700, datasheet SNOSBW2F (November 2015),
    /// typical at ±15 V, 25 °C: the bias pin sits "2 diode drops above
    /// the negative supply" (−13.6 V at −15 V in its example: 1.4 V, two
    /// junctions), input offset 0.4 mV; gm 9600 µS at 500 µA.
    pub const LM13700: Self = Self {
        name: "LM13700",
        bias_saturation_amps: 8.817_884e-16,
        junctions: 2.0,
        offset_volts: 0.4e-3,
        thermal_volts: THERMAL_VOLTS_27C,
    };

    pub const ALL: [Self; 2] = [Self::CA3080, Self::LM13700];

    fn knee(&self) -> f64 {
        self.junctions * self.thermal_volts
    }

    /// The bias-pin voltage that draws `amps`.
    pub fn bias_volts_at(&self, amps: f64) -> f64 {
        self.knee() * (amps / self.bias_saturation_amps).ln_1p()
    }

    /// Transconductance at the origin for a bias current of `amps`.
    pub fn transconductance(&self, amps: f64) -> f64 {
        amps / (2.0 * self.thermal_volts)
    }
}

impl Ota for BipolarPair {
    #[inline]
    fn currents(&self, input: f64, bias_volts: f64) -> OtaCurrents {
        let knee = self.knee();
        let e = (bias_volts / knee).exp();
        let iabc = self.bias_saturation_amps * (e - 1.0);
        let diabc = self.bias_saturation_amps * e / knee;
        let two_vt = 2.0 * self.thermal_volts;
        let t = ((input + self.offset_volts) / two_vt).tanh();
        OtaCurrents {
            output: iabc * t,
            bias: iabc,
            output_per_input_volt: iabc * (1.0 - t * t) / two_vt,
            output_per_bias_volt: diabc * t,
            bias_per_bias_volt: diabc,
        }
    }

    fn limit_bias_step(&self, new: f64, old: f64) -> f64 {
        let junction = crate::circuit::devices::diode::Shockley {
            saturation_amps: self.bias_saturation_amps,
            emission: self.junctions,
            thermal_volts: self.thermal_volts,
            ..crate::circuit::devices::diode::Shockley::DEFAULT
        };
        junction.limit_step(new, old)
    }
}
