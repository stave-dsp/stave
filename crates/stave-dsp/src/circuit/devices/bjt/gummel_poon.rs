//! The Gummel–Poon BJT (Gummel & Poon 1970) as SPICE implements it: the
//! transport currents divided by the normalised base charge `qb`, which
//! carries the Early effect and high-injection β roll-off, plus the
//! non-ideal base–emitter and base–collector leakage junctions.
//!
//! ```text
//!     Ibe = Is·(exp(vbe/(Nf·Vt)) − 1)     Ibc = Is·(exp(vbc/(Nr·Vt)) − 1)
//!     q1  = 1 / (1 − vbc/Vaf − vbe/Var)
//!     q2  = Ibe/Ikf + Ibc/Ikr
//!     qb  = q1·(1 + √(1 + 4·q2)) / 2
//!     Ic  = (Ibe − Ibc)/qb − Ibc/Br − Isc·(exp(vbc/(Nc·Vt)) − 1)
//!     Ib  = Ibe/Bf + Ibc/Br + Ise·(exp(vbe/(Ne·Vt)) − 1) + Isc·(exp(vbc/(Nc·Vt)) − 1)
//! ```

use super::{Bjt, BjtCurrents, Polarity};
use crate::circuit::devices::diode::THERMAL_VOLTS_27C;
use crate::circuit::devices::junction;

/// A Gummel–Poon transistor: SPICE's `.model NAME NPN(…)` / `PNP(…)`
/// card at its DC parameters. A parameter SPICE treats as infinite when
/// left at zero (`VAF`, `VAR`, `IKF`, `IKR`) is `f64::INFINITY` here.
///
/// # Models
///
/// A silicon or germanium BJT's static currents: forward and reverse
/// transport, Early effect (`VAF`, `VAR`), high-injection roll-off
/// (`IKF`, `IKR`), base–emitter and base–collector leakage (`ISE`/`NE`,
/// `ISC`/`NC`), and its ohmic base, emitter and collector resistances,
/// which a circuit method realises as resistors to the intrinsic
/// junctions (constant `RB`: the card's `RBM`/`IRB` current crowding is
/// not modelled). Junction and diffusion capacitances (`CJE`, `CJC`,
/// `TF`, `TR`), excess phase and temperature scaling are not modelled;
/// the netlist parser lists them in `Netlist::unmodelled`.
///
/// # Derivation
///
/// The module's equations, as ngspice's `bjtload.c` evaluates them
/// (including its reverse-junction cubic below three knees), so a deck
/// solves to the same numbers in both.
///
/// # Verified
///
/// Against ngspice on the same cards, through `Circuit<Dk>`:
/// `crates/stave-test/tests/transistors_match_ngspice.rs::bjt_family_matches_ngspice` (2N3904 Ic
/// against Vce for five base currents: budget 2e-6 normalized RMS,
/// measured 8e-7), `crates/stave-test/tests/transistors_match_ngspice.rs::bjt_family_pnp_matches_ngspice`
/// (AC128: 5e-7, measured 1.4e-7),
/// `crates/stave-test/tests/transistors_match_ngspice.rs::ce_stage_matches_ngspice` (1e-5 at
/// 4×, measured 5.2e-6, falling as h²),
/// `crates/stave-test/tests/transistors_match_ngspice.rs::ce_stage_ac_matches_ngspice` (2e-6
/// dB), `crates/stave-test/tests/transistors_match_ngspice.rs::fuzz_face_matches_ngspice`
/// (4e-5 at 4×, measured 2.0e-5) and
/// `crates/stave-test/tests/transistors_match_ngspice.rs::rangemaster_matches_ngspice` (1e-3 at
/// 4×, measured 2.9e-4, falling as h²).
/// `crates/stave-dsp/src/circuit/devices/bjt/tests.rs::every_part_s_partials_match_finite_differences`
/// holds the partials to finite differences.
///
/// Against the parts (DSP.md R13):
/// `crates/stave-test/tests/matches_datasheet.rs::transistors::n2n3904_gummel_poon`
/// (onsemi's hFE, ON-voltage and saturation figures; inside the table's
/// hFE limits, but its roll-off, high-current VBE and saturation edge are
/// known gaps) and
/// `crates/stave-test/tests/matches_datasheet.rs::transistors::ac128_output`,
/// `crates/stave-test/tests/matches_datasheet.rs::transistors::ac128_hfe`,
/// `crates/stave-test/tests/matches_datasheet.rs::transistors::ac128_transfer`,
/// `crates/stave-test/tests/matches_datasheet.rs::transistors::ac128_input`
/// (Mullard's typical AC128, 12–45% of the peak reading: known gaps; the
/// card is a measured vintage part).
///
/// # Cost
///
/// Three to five exponentials an evaluation; in a circuit, the
/// datasheet's `"Circuit<Dk> fuzz face"` (two AC128s) and
/// `"Circuit<Dk> CE stage x0.2"`.
///
/// # Aliasing
///
/// None of its own: a memoryless device in a circuit that aliases as
/// its waveforms do; run hard-driven circuits oversampled.
///
/// # Valid ranges
///
/// Where the card was fitted: small-signal currents up to tens of
/// milliamps (germanium parts up to a few hundred), below breakdown
/// (not modelled), at the card's temperature.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GummelPoon {
    pub polarity: Polarity,
    /// `IS`: transport saturation current (A).
    pub saturation_amps: f64,
    /// `BF`: ideal forward β.
    pub forward_beta: f64,
    /// `BR`: ideal reverse β.
    pub reverse_beta: f64,
    /// `NF`: forward emission coefficient.
    pub forward_emission: f64,
    /// `NR`: reverse emission coefficient.
    pub reverse_emission: f64,
    /// `VAF`: forward Early voltage (V).
    pub forward_early_volts: f64,
    /// `VAR`: reverse Early voltage (V).
    pub reverse_early_volts: f64,
    /// `IKF`: forward high-injection knee (A).
    pub forward_knee_amps: f64,
    /// `IKR`: reverse high-injection knee (A).
    pub reverse_knee_amps: f64,
    /// `ISE`: base–emitter leakage saturation current (A).
    pub emitter_leakage_amps: f64,
    /// `NE`: base–emitter leakage emission coefficient.
    pub emitter_leakage_emission: f64,
    /// `ISC`: base–collector leakage saturation current (A).
    pub collector_leakage_amps: f64,
    /// `NC`: base–collector leakage emission coefficient.
    pub collector_leakage_emission: f64,
    /// `RB`: base resistance (Ω).
    pub base_ohms: f64,
    /// `RE`: emitter resistance (Ω).
    pub emitter_ohms: f64,
    /// `RC`: collector resistance (Ω).
    pub collector_ohms: f64,
    /// kT/q at the card's temperature (V).
    pub thermal_volts: f64,
}

impl GummelPoon {
    /// SPICE's defaults: `.model Q NPN` with no parameters.
    pub const DEFAULT_NPN: Self = Self {
        polarity: Polarity::N,
        saturation_amps: 1e-16,
        forward_beta: 100.0,
        reverse_beta: 1.0,
        forward_emission: 1.0,
        reverse_emission: 1.0,
        forward_early_volts: f64::INFINITY,
        reverse_early_volts: f64::INFINITY,
        forward_knee_amps: f64::INFINITY,
        reverse_knee_amps: f64::INFINITY,
        emitter_leakage_amps: 0.0,
        emitter_leakage_emission: 1.5,
        collector_leakage_amps: 0.0,
        collector_leakage_emission: 2.0,
        base_ohms: 0.0,
        emitter_ohms: 0.0,
        collector_ohms: 0.0,
        thermal_volts: THERMAL_VOLTS_27C,
    };

    /// 2N3904, silicon NPN: Fairchild's datasheet card (`Is=6.734f
    /// Bf=416.4 Br=.7371 Vaf=74.03 Ikf=66.78m Ise=6.734f Ne=1.259 Nc=2
    /// Rb=10 Rc=1`), at 27 °C.
    pub const N2N3904: Self = Self {
        saturation_amps: 6.734e-15,
        forward_beta: 416.4,
        reverse_beta: 0.7371,
        forward_early_volts: 74.03,
        forward_knee_amps: 66.78e-3,
        emitter_leakage_amps: 6.734e-15,
        emitter_leakage_emission: 1.259,
        base_ohms: 10.0,
        collector_ohms: 1.0,
        ..Self::DEFAULT_NPN
    };

    /// 2N5088, high-β silicon NPN: Fairchild's datasheet card
    /// (`Is=5.911f Bf=1.122K Br=1.271 Vaf=62.37 Ikf=14.92m Ise=5.911f
    /// Ne=1.394 Nc=2 Rb=10 Rc=1.61`), at 27 °C.
    pub const N2N5088: Self = Self {
        saturation_amps: 5.911e-15,
        forward_beta: 1122.0,
        reverse_beta: 1.271,
        forward_early_volts: 62.37,
        forward_knee_amps: 14.92e-3,
        emitter_leakage_amps: 5.911e-15,
        emitter_leakage_emission: 1.394,
        base_ohms: 10.0,
        collector_ohms: 1.61,
        ..Self::DEFAULT_NPN
    };

    /// 2N3392, silicon NPN (the Moog 904-A ladder's part): National
    /// Semiconductor's card (`IS=12.03f BF=254.8 BR=4.379 VAF=37.37
    /// VAR=12.5 IKF=0.1072 ISE=4.958p NE=1.971 ISC=0 RB=10 RC=1 RE=0.1`,
    /// MFG=NSC, as aggregated in the KiCad-Spice-Library's `BJT.lib`), at
    /// 27 °C; its capacitances and transit times are not modelled.
    pub const N2N3392: Self = Self {
        saturation_amps: 12.03e-15,
        forward_beta: 254.8,
        reverse_beta: 4.379,
        forward_early_volts: 37.37,
        reverse_early_volts: 12.5,
        forward_knee_amps: 0.1072,
        emitter_leakage_amps: 4.958e-12,
        emitter_leakage_emission: 1.971,
        base_ohms: 10.0,
        emitter_ohms: 0.1,
        collector_ohms: 1.0,
        ..Self::DEFAULT_NPN
    };

    /// AC128, germanium PNP, extracted from vintage parts by Holmes,
    /// Holters & van Walstijn ("Comparison of germanium bipolar junction
    /// transistor models for real-time circuit simulation", DAFx-17,
    /// table 1), at their Vt = 25.5 mV.
    pub const AC128: Self = Self {
        polarity: Polarity::P,
        saturation_amps: 20.66e-6,
        forward_beta: 229.6,
        reverse_beta: 14.66,
        forward_emission: 1.133,
        reverse_emission: 1.140,
        forward_early_volts: 19.68,
        reverse_early_volts: 88.28,
        forward_knee_amps: 463.0e-3,
        reverse_knee_amps: 241.5e-3,
        emitter_leakage_amps: 2.190e-6,
        emitter_leakage_emission: 1.796,
        collector_leakage_amps: 7.546e-6,
        collector_leakage_emission: 1.364,
        base_ohms: 1.885,
        emitter_ohms: 306.4e-3,
        collector_ohms: 1.727e-6,
        thermal_volts: 25.5e-3,
    };

    /// OC44, germanium PNP, from the same extraction as [`Self::AC128`]
    /// (Holmes, Holters & van Walstijn, DAFx-17), at Vt = 25.5 mV.
    pub const OC44: Self = Self {
        polarity: Polarity::P,
        saturation_amps: 1.423e-6,
        forward_beta: 307.0,
        reverse_beta: 20.27,
        forward_emission: 1.022,
        reverse_emission: 1.025,
        forward_early_volts: 8.167,
        reverse_early_volts: 14.84,
        forward_knee_amps: 43.82e-3,
        reverse_knee_amps: 611.7e-3,
        emitter_leakage_amps: 30.54e-9,
        emitter_leakage_emission: 1.316,
        collector_leakage_amps: 213.5e-9,
        collector_leakage_emission: 1.258,
        base_ohms: 32.83,
        emitter_ohms: 968.7e-3,
        collector_ohms: 989.9e-6,
        thermal_volts: 25.5e-3,
    };

    /// `area` devices in parallel, as SPICE's area factor: currents
    /// scale up, resistances down.
    pub fn with_area(self, area: f64) -> Self {
        Self {
            saturation_amps: self.saturation_amps * area,
            forward_knee_amps: self.forward_knee_amps * area,
            reverse_knee_amps: self.reverse_knee_amps * area,
            emitter_leakage_amps: self.emitter_leakage_amps * area,
            collector_leakage_amps: self.collector_leakage_amps * area,
            base_ohms: self.base_ohms / area,
            emitter_ohms: self.emitter_ohms / area,
            collector_ohms: self.collector_ohms / area,
            ..self
        }
    }

    /// The N twin's currents at N-sense junction voltages.
    #[inline]
    fn intrinsic(&self, vbe: f64, vbc: f64) -> BjtCurrents {
        let vt = self.thermal_volts;
        let is = self.saturation_amps;
        let (ibe, gbe) = junction::current_and_conductance(vbe, is, self.forward_emission * vt);
        let (ibc, gbc) = junction::current_and_conductance(vbc, is, self.reverse_emission * vt);
        let (iben, gben) = leakage(vbe, self.emitter_leakage_amps, self.emitter_leakage_emission * vt);
        let (ibcn, gbcn) =
            leakage(vbc, self.collector_leakage_amps, self.collector_leakage_emission * vt);
        let (inv_vaf, inv_var) = (1.0 / self.forward_early_volts, 1.0 / self.reverse_early_volts);
        let (oik, oikr) = (1.0 / self.forward_knee_amps, 1.0 / self.reverse_knee_amps);
        let q1 = 1.0 / (1.0 - inv_vaf * vbc - inv_var * vbe);
        let (qb, dqb_dvbe, dqb_dvbc) = if oik == 0.0 && oikr == 0.0 {
            (q1, q1 * q1 * inv_var, q1 * q1 * inv_vaf)
        } else {
            let q2 = oik * ibe + oikr * ibc;
            let arg = (1.0 + 4.0 * q2).max(0.0);
            let sq = if arg != 0.0 { arg.sqrt() } else { 1.0 };
            let qb = q1 * (1.0 + sq) / 2.0;
            (
                qb,
                q1 * (qb * inv_var + oik * gbe / sq),
                q1 * (qb * inv_vaf + oikr * gbc / sq),
            )
        };
        let (br, bf) = (self.reverse_beta, self.forward_beta);
        let transport = (ibe - ibc) / qb;
        BjtCurrents {
            collector: transport - ibc / br - ibcn,
            base: ibe / bf + iben + ibc / br + ibcn,
            collector_per_vbe: gbe / qb - transport * dqb_dvbe / qb,
            collector_per_vbc: -gbc / qb - transport * dqb_dvbc / qb - gbc / br - gbcn,
            base_per_vbe: gbe / bf + gben,
            base_per_vbc: gbc / br + gbcn,
        }
    }
}

/// A leakage junction, absent at zero saturation current (as SPICE).
#[inline]
fn leakage(volts: f64, is: f64, knee: f64) -> (f64, f64) {
    if is == 0.0 {
        (0.0, 0.0)
    } else {
        junction::current_and_conductance(volts, is, knee)
    }
}

impl Bjt for GummelPoon {
    fn polarity(&self) -> Polarity {
        self.polarity
    }

    #[inline]
    fn currents(&self, vbe: f64, vbc: f64) -> BjtCurrents {
        let s = self.polarity.sign();
        self.intrinsic(s * vbe, s * vbc).mirrored(self.polarity)
    }

    #[inline]
    fn limit_step(&self, new: [f64; 2], old: [f64; 2]) -> [f64; 2] {
        let s = self.polarity.sign();
        let vt = self.thermal_volts;
        let is = self.saturation_amps;
        let knees = [self.forward_emission * vt, self.reverse_emission * vt];
        [0, 1].map(|j| s * junction::limit_step(s * new[j], s * old[j], is, knees[j]))
    }
}
