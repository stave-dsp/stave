//! The Shichman–Hodges JFET (Shichman & Hodges 1968), SPICE's JFET
//! level 1: a square-law channel with channel-length modulation, and two
//! gate junctions.
//!
//! ```text
//!     vgst = vgs − Vto                         (no current at or below 0)
//!     Id   = β·(1 + λ·vds)·vds·(2·vgst − vds)  0 ≤ vds < vgst  (linear)
//!     Id   = β·(1 + λ·vds)·vgst²               vds ≥ vgst      (saturation)
//!     Ig   = Is·(exp(vgs/(N·Vt)) − 1) + Is·(exp(vgd/(N·Vt)) − 1)
//! ```
//!
//! The channel is symmetric: for vds < 0 drain and source swap roles.

use super::{Jfet, JfetCurrents, Polarity};
use crate::circuit::devices::capacitance::Depletion;
use crate::circuit::devices::diode::THERMAL_VOLTS_27C;
use crate::circuit::devices::junction;

/// A Shichman–Hodges JFET: SPICE's `.model NAME NJF(…)` / `PJF(…)` card
/// at its DC parameters.
///
/// # Models
///
/// A JFET's static drain current (square law, `VTO`, `BETA`, `LAMBDA`),
/// its gate junctions (`IS`, `N`), and its ohmic drain and source
/// resistances (`RD`, `RS`), which a circuit method realises as
/// resistors, and the gate junctions' depletion charge (`CGS`, `CGD`
/// with `PB` and `FC`, grading ½ as in SPICE's level 1:
/// [`gate_charges`](Self::gate_charges)). The doping-tail parameter `B`
/// of ngspice's Sydney extension (here always 1, the plain square law),
/// PSpice's `ISR`/`NR`/`ALPHA`/`VK`/`M` and temperature scaling are not
/// modelled. `VTO` is the card's value: negative for a
/// depletion device of either channel, as SPICE writes it.
///
/// # Derivation
///
/// The module's equations, as ngspice's `jfetload.c` evaluates them at
/// `B = 1`, gate junctions and their charge storage included (tag
/// ngspice-42).
///
/// # Verified
///
/// Against ngspice on the same cards, through `Circuit<Dk>`:
/// `crates/stave-test/tests/transistors_match_ngspice.rs::jfet_family_matches_ngspice` (J201 Id
/// against Vds for five gate voltages, through its 112 Ω and 101 Ω RD/RS:
/// budget 1e-7 normalized RMS, measured 1.6e-8) and
/// `crates/stave-test/tests/transistors_match_ngspice.rs::jfet_booster_matches_ngspice` (a J201
/// common-source booster with its gate capacitances: 1.5e-5 at 4×,
/// measured 1.1e-5; static, 6.8e-6). Its charge on its own:
/// `crates/stave-test/tests/capacitance_matches_ngspice.rs::junction_capacitance_matches_ngspice`
/// (the gate's `C(v)`, 1e-5, measured 3.9e-6) and
/// `crates/stave-test/tests/capacitance_matches_ngspice.rs::small_signal_responses_match_ngspice`
/// (the booster to 100 MHz: 3e-5 dB, 3e-4°).
/// `crates/stave-dsp/src/circuit/devices/jfet/tests.rs::a_p_channel_is_its_n_twin_mirrored`
/// holds the P channel to the N mirror.
///
/// # Cost
///
/// Two exponentials an evaluation; in a circuit, the datasheet's
/// `"Circuit<Dk> JFET booster"`.
///
/// # Aliasing
///
/// None of its own (a memoryless device).
///
/// # Valid ranges
///
/// Below breakdown (not modelled); the square law is the long-channel
/// approximation the cards were fitted with.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ShichmanHodges {
    pub polarity: Polarity,
    /// `VTO`: pinch-off voltage (V), negative for depletion.
    pub threshold_volts: f64,
    /// `BETA`: transconductance parameter (A/V²); Idss = β·Vto².
    pub beta: f64,
    /// `LAMBDA`: channel-length modulation (1/V).
    pub channel_modulation: f64,
    /// `IS`: gate junction saturation current (A).
    pub saturation_amps: f64,
    /// `N`: gate junction emission coefficient.
    pub emission: f64,
    /// `RD` (Ω).
    pub drain_ohms: f64,
    /// `RS` (Ω).
    pub source_ohms: f64,
    /// kT/q (V).
    pub thermal_volts: f64,
    /// `CGS`, `PB`, `FC`: the gate–source junction's depletion
    /// capacitance (grading ½).
    pub gate_source_capacitance: Depletion,
    /// `CGD`, `PB`, `FC`: the gate–drain junction's.
    pub gate_drain_capacitance: Depletion,
}

/// SPICE's JFET gate junction: `PB` = 1 V, grading ½, `FC` = ½.
const NO_GATE_CAPACITANCE: Depletion = Depletion::NONE;

/// A gate junction of `farads` at zero bias, at SPICE's `PB` and `FC`.
const fn gate(farads: f64) -> Depletion {
    Depletion {
        zero_bias_farads: farads,
        ..NO_GATE_CAPACITANCE
    }
}

impl ShichmanHodges {
    /// SPICE's defaults: `.model J NJF` with no parameters.
    pub const DEFAULT_N: Self = Self {
        polarity: Polarity::N,
        threshold_volts: -2.0,
        beta: 1e-4,
        channel_modulation: 0.0,
        saturation_amps: 1e-14,
        emission: 1.0,
        drain_ohms: 0.0,
        source_ohms: 0.0,
        thermal_volts: THERMAL_VOLTS_27C,
        gate_source_capacitance: NO_GATE_CAPACITANCE,
        gate_drain_capacitance: NO_GATE_CAPACITANCE,
    };

    /// J201, N-channel: Siliconix's card (07-14-1990: `VTO=-0.90
    /// BETA=7.00E-04 LAMBDA=.035 RD=1.12E+02 RS=1.01E+02 IS=1.58E-16
    /// PB=1.00 FC=.5 CGS=3.00E-12 CGD=1.00E-12`).
    pub const J201: Self = Self {
        threshold_volts: -0.90,
        beta: 7.0e-4,
        channel_modulation: 0.035,
        saturation_amps: 1.58e-16,
        drain_ohms: 112.0,
        source_ohms: 101.0,
        gate_source_capacitance: gate(3e-12),
        gate_drain_capacitance: gate(1e-12),
        ..Self::DEFAULT_N
    };

    /// 2N5457, N-channel: Siliconix's card (07-22-1990: `VTO=-3.00
    /// BETA=1.51E-03 LAMBDA=5.16M RD=4.15E+01 RS=6.83E+01 IS=4.74E-15
    /// PB=1.00 FC=.5 CGS=1.35E-11 CGD=4.00E-12`).
    pub const N2N5457: Self = Self {
        threshold_volts: -3.0,
        beta: 1.51e-3,
        channel_modulation: 5.16e-3,
        saturation_amps: 4.74e-15,
        drain_ohms: 41.5,
        source_ohms: 68.3,
        gate_source_capacitance: gate(13.5e-12),
        gate_drain_capacitance: gate(4e-12),
        ..Self::DEFAULT_N
    };

    /// 2N5458, N-channel (the ProCo RAT's output follower): Siliconix's
    /// card (`VTO=-4.00 BETA=2.34E-03 LAMBDA=5.16M RD=3.80E+01
    /// RS=6.52E+01 IS=6.74E-15`).
    pub const N2N5458: Self = Self {
        threshold_volts: -4.0,
        beta: 2.34e-3,
        channel_modulation: 5.16e-3,
        saturation_amps: 6.74e-15,
        drain_ohms: 38.0,
        source_ohms: 65.2,
        ..Self::DEFAULT_N
    };

    /// 2N5460, P-channel: Motorola's card (05-06-1991: `VTO=-3.2
    /// BETA=1.7M LAMBDA=5.63M RD=4.94E+01 RS=4.45E+01 IS=1.95F PB=1 FC=.5
    /// CGS=9.0P CGD=4.0P`).
    pub const N2N5460: Self = Self {
        polarity: Polarity::P,
        threshold_volts: -3.2,
        beta: 1.7e-3,
        channel_modulation: 5.63e-3,
        saturation_amps: 1.95e-15,
        drain_ohms: 49.4,
        source_ohms: 44.5,
        gate_source_capacitance: gate(9e-12),
        gate_drain_capacitance: gate(4e-12),
        ..Self::DEFAULT_N
    };

    /// J175, P-channel: Siliconix's card (`VTO=-4.90 BETA=3.6M
    /// LAMBDA=6.89M RD=14 RS=14.6 IS=3.51F PB=1 FC=.5 CGS=12.5P
    /// CGD=16.5P`).
    pub const J175: Self = Self {
        polarity: Polarity::P,
        threshold_volts: -4.9,
        beta: 3.6e-3,
        channel_modulation: 6.89e-3,
        saturation_amps: 3.51e-15,
        drain_ohms: 14.0,
        source_ohms: 14.6,
        gate_source_capacitance: gate(12.5e-12),
        gate_drain_capacitance: gate(16.5e-12),
        ..Self::DEFAULT_N
    };

    /// `area` devices in parallel (SPICE's area factor).
    pub fn with_area(self, area: f64) -> Self {
        Self {
            beta: self.beta * area,
            saturation_amps: self.saturation_amps * area,
            drain_ohms: self.drain_ohms / area,
            source_ohms: self.source_ohms / area,
            gate_source_capacitance: self.gate_source_capacitance.scaled(area),
            gate_drain_capacitance: self.gate_drain_capacitance.scaled(area),
            ..self
        }
    }

    /// Which gate junctions store charge: (gate–source, gate–drain).
    pub fn stored_charges(&self) -> [bool; 2] {
        [!self.gate_source_capacitance.is_none(), !self.gate_drain_capacitance.is_none()]
    }

    /// `[(q, C); 2]`: the gate–source and gate–drain junctions' charges
    /// and capacitances at the intrinsic `vgs`, `vgd` (`jfetload.c`).
    #[inline]
    pub fn gate_charges(&self, vgs: f64, vgd: f64) -> [(f64, f64); 2] {
        let s = self.polarity.sign();
        let (qs, cs) = self.gate_source_capacitance.charge(s * vgs);
        let (qd, cd) = self.gate_drain_capacitance.charge(s * vgd);
        [(s * qs, cs), (s * qd, cd)]
    }

    /// The channel at `vds ≥ 0`: `(Id, ∂Id/∂vgs, ∂Id/∂vds)`.
    #[inline]
    fn forward(&self, vgs: f64, vds: f64) -> (f64, f64, f64) {
        let vgst = vgs - self.threshold_volts;
        if vgst <= 0.0 {
            return (0.0, 0.0, 0.0);
        }
        let (beta, lambda) = (self.beta, self.channel_modulation);
        let betap = beta * (1.0 + lambda * vds);
        if vgst >= vds {
            let cpart = vds * (2.0 * vgst - vds);
            (
                betap * cpart,
                betap * 2.0 * vds,
                betap * 2.0 * (vgst - vds) + beta * lambda * cpart,
            )
        } else {
            let cpart = vgst * vgst;
            (betap * cpart, betap * 2.0 * vgst, lambda * beta * cpart)
        }
    }

    /// The channel at any `vds`: `(Id, ∂Id/∂vgs, ∂Id/∂vds)`, the source
    /// and drain swapping roles below zero.
    #[inline]
    pub(crate) fn channel(&self, vgs: f64, vds: f64) -> (f64, f64, f64) {
        if vds >= 0.0 {
            self.forward(vgs, vds)
        } else {
            let (id, gm, gds) = self.forward(vgs - vds, -vds);
            (-id, -gm, gm + gds)
        }
    }

    /// The N twin's currents at N-sense voltages.
    #[inline]
    fn intrinsic(&self, vgs: f64, vgd: f64) -> JfetCurrents {
        let knee = self.emission * self.thermal_volts;
        let (igs, ggs) = junction::current_and_conductance(vgs, self.saturation_amps, knee);
        let (igd, ggd) = junction::current_and_conductance(vgd, self.saturation_amps, knee);
        let (id, gm, gds) = self.channel(vgs, vgs - vgd);
        JfetCurrents {
            drain: id - igd,
            gate: igs + igd,
            drain_per_vgs: gm + gds,
            drain_per_vgd: -gds - ggd,
            gate_per_vgs: ggs,
            gate_per_vgd: ggd,
        }
    }
}

impl Jfet for ShichmanHodges {
    fn polarity(&self) -> Polarity {
        self.polarity
    }

    #[inline]
    fn currents(&self, vgs: f64, vgd: f64) -> JfetCurrents {
        let s = self.polarity.sign();
        let c = self.intrinsic(s * vgs, s * vgd);
        JfetCurrents {
            drain: s * c.drain,
            gate: s * c.gate,
            ..c
        }
    }

    #[inline]
    fn limit_step(&self, new: [f64; 2], old: [f64; 2]) -> [f64; 2] {
        let s = self.polarity.sign();
        let knee = self.emission * self.thermal_volts;
        let is = self.saturation_amps;
        [0, 1].map(|j| s * junction::limit_step(s * new[j], s * old[j], is, knee))
    }
}
