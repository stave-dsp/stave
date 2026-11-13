//! The square-law MOSFET (Shichman & Hodges 1968), SPICE's MOSFET
//! level 1, with the body effect and channel-length modulation:
//!
//! ```text
//!     Von  = Vto + γ·(√(φ − vbs) − √φ)          (N sense)
//!     vgst = vgs − Von                           (no current at or below 0)
//!     Id   = β·(1 + λ·vds)·vds·(vgst − vds/2)    0 ≤ vds < vgst  (linear)
//!     Id   = β/2·(1 + λ·vds)·vgst²               vds ≥ vgst      (saturation)
//! ```
//!
//! For vbs > 0 the root is SPICE's linear continuation
//! `√φ − vbs/(2√φ)`, floored at zero. The channel is symmetric: for
//! vds < 0 drain and source swap roles.

use super::{Mosfet, MosfetCurrents, Polarity};
use crate::circuit::devices::capacitance::Depletion;

/// A level-1 MOSFET: SPICE's `.model NAME NMOS(LEVEL=1 …)` / `PMOS(…)`
/// at its DC parameters, with the element's `W/L` folded into `beta`.
///
/// # Models
///
/// A long-channel MOSFET's static drain current: threshold (`VTO`),
/// transconductance (`KP·W/(L − 2·LD)`), body effect (`GAMMA`, `PHI`),
/// channel-length modulation (`LAMBDA`), and ohmic drain and source
/// resistances (`RD`, `RS`), realised as resistors by a circuit method.
/// Its capacitances as level 1 has them without an oxide thickness: the
/// gate overlaps (`CGSO·W`, `CGDO·W`, `CGBO·Leff`, constant) and the bulk
/// junctions' depletion charge (`CBD`, `CBS`, or `CJ` over `AD`/`AS`,
/// with `PB`, `MJ`, `FC`: [`bulk_charges`](Self::bulk_charges)). Meyer's
/// bias-dependent gate capacitance (from `TOX`), junction sidewalls
/// (`CJSW`), subthreshold conduction (EKV's job) and the bulk junction
/// diodes' currents (reverse-biased in any sane bias: picoamps) are not
/// modelled. `VTO` is the card's value, negative for an enhancement
/// PMOS.
///
/// # Derivation
///
/// The module's equations, as ngspice's `mos1load.c` evaluates them,
/// its bulk-junction charge and overlap capacitances included (tag
/// ngspice-42; with no `TOX` its Meyer capacitances are zero).
///
/// # Verified
///
/// Against ngspice on the same cards, through `Circuit<Dk>`:
/// `crates/stave-test/tests/transistors_match_ngspice.rs::mosfet_family_matches_ngspice` (BSS138
/// Id against Vds for five gate voltages, the body effect exercised by a
/// source above the bulk: budget 1e-7 normalized RMS, measured 2.6e-8)
/// and `crates/stave-test/tests/transistors_match_ngspice.rs::mosfet_booster_matches_ngspice` (a
/// VN10KM common-source booster with its capacitances: 5e-6 at 16×,
/// measured 2.2e-6, falling as h²; static, 1e-6 at 4×). Its charge on its
/// own: `crates/stave-test/tests/capacitance_matches_ngspice.rs::junction_capacitance_matches_ngspice`
/// (the bulk's `C(v)`, 1e-5, measured 2.5e-6) and
/// `crates/stave-test/tests/capacitance_matches_ngspice.rs::small_signal_responses_match_ngspice`
/// (the booster to 100 MHz: 1e-5 dB, 1e-4°, measured 7.3e-8 dB).
/// `crates/stave-dsp/src/circuit/devices/mosfet/tests.rs::every_part_s_partials_match_finite_differences`
/// holds the partials to finite differences.
///
/// # Cost
///
/// A square root and a few products an evaluation; no datasheet line of
/// its own.
///
/// # Aliasing
///
/// None of its own (a memoryless device).
///
/// # Valid ranges
///
/// Above threshold, long channel, below breakdown; the card's fit.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ShichmanHodges {
    pub polarity: Polarity,
    /// `VTO` (V), as the card writes it.
    pub threshold_volts: f64,
    /// `KP·W/Leff` (A/V²).
    pub beta: f64,
    /// `LAMBDA` (1/V).
    pub channel_modulation: f64,
    /// `GAMMA` (√V).
    pub body_effect: f64,
    /// `PHI`: surface potential (V).
    pub surface_potential: f64,
    /// `RD` (Ω).
    pub drain_ohms: f64,
    /// `RS` (Ω).
    pub source_ohms: f64,
    /// `CGSO·W·M`: the gate–source overlap capacitance (F).
    pub gate_source_overlap_farads: f64,
    /// `CGDO·W·M`: the gate–drain overlap capacitance (F).
    pub gate_drain_overlap_farads: f64,
    /// `CGBO·Leff·M`: the gate–bulk overlap capacitance (F).
    pub gate_bulk_overlap_farads: f64,
    /// `CBD` (or `CJ·AD`), `PB`, `MJ`, `FC`: the bulk–drain junction.
    pub bulk_drain_capacitance: Depletion,
    /// `CBS` (or `CJ·AS`), `PB`, `MJ`, `FC`: the bulk–source junction.
    pub bulk_source_capacitance: Depletion,
}

/// SPICE's level-1 bulk junction: `PB` = 0.8 V, `MJ` = ½, `FC` = ½.
pub(crate) const NO_BULK_CAPACITANCE: Depletion = Depletion {
    zero_bias_farads: 0.0,
    potential_volts: 0.8,
    grading: 0.5,
    forward_coefficient: 0.5,
};

/// A bulk junction of `farads` at zero bias, at `PB` = 0.8 V and `MJ` =
/// 0.46 (both Siliconix/Diodes cards below).
const fn bulk(farads: f64) -> Depletion {
    Depletion {
        zero_bias_farads: farads,
        grading: 0.46,
        ..NO_BULK_CAPACITANCE
    }
}

impl ShichmanHodges {
    /// SPICE's level-1 defaults (`KP = 2e-5`, `PHI = 0.6`) at the default
    /// 100 µm × 100 µm channel.
    pub const DEFAULT_N: Self = Self {
        polarity: Polarity::N,
        threshold_volts: 0.0,
        beta: 2e-5,
        channel_modulation: 0.0,
        body_effect: 0.0,
        surface_potential: 0.6,
        drain_ohms: 0.0,
        source_ohms: 0.0,
        gate_source_overlap_farads: 0.0,
        gate_drain_overlap_farads: 0.0,
        gate_bulk_overlap_farads: 0.0,
        bulk_drain_capacitance: NO_BULK_CAPACITANCE,
        bulk_source_capacitance: NO_BULK_CAPACITANCE,
    };

    /// VN10KM, N-channel enhancement: Siliconix's level-1 card (`VTO=1.4
    /// KP=1.55E-01 PHI=0.75 LAMBDA=2.9E-04 RD=7.60E-01 RS=7.60E-01
    /// CBD=3.28E-11 CBS=3.94E-11 PB=0.80 MJ=.46 CGSO=2.40E-08
    /// CGDO=2.00E-08 CGBO=3.36E-07`) at the default 100 µm × 100 µm
    /// channel; its `IS` (the bulk diodes' current) is not modelled.
    pub const VN10KM: Self = Self {
        threshold_volts: 1.4,
        beta: 0.155,
        channel_modulation: 2.9e-4,
        surface_potential: 0.75,
        drain_ohms: 0.76,
        source_ohms: 0.76,
        gate_source_overlap_farads: 2.4e-12,
        gate_drain_overlap_farads: 2.0e-12,
        gate_bulk_overlap_farads: 33.6e-12,
        bulk_drain_capacitance: bulk(32.8e-12),
        bulk_source_capacitance: bulk(39.4e-12),
        ..Self::DEFAULT_N
    };

    /// BSS138, N-channel enhancement: Diodes Inc.'s level-1 card
    /// (`DI_BSS138`: `VTO=1.2 KP=7.50m GAMMA=1.49 PHI=.75 LAMBDA=1.25m
    /// RD=0.196 RS=0.196 PB=0.800 MJ=0.460 CBD=18.4p CBS=22.0p CGSO=28.8n
    /// CGDO=24.0n CGBO=247n L=100U W=100U`); its `IS` is not modelled.
    pub const BSS138: Self = Self {
        threshold_volts: 1.2,
        beta: 7.5e-3,
        channel_modulation: 1.25e-3,
        body_effect: 1.49,
        surface_potential: 0.75,
        drain_ohms: 0.196,
        source_ohms: 0.196,
        gate_source_overlap_farads: 2.88e-12,
        gate_drain_overlap_farads: 2.4e-12,
        gate_bulk_overlap_farads: 24.7e-12,
        bulk_drain_capacitance: bulk(18.4e-12),
        bulk_source_capacitance: bulk(22.0e-12),
        ..Self::DEFAULT_N
    };

    /// CD4007's N device: Analog Devices' teaching card (`Level=1
    /// Kp=111u Vto=2.0 Lambda=0.01 Phi=.6 Gamma=0`) at its 30 µm / 10 µm
    /// channel. Its capacitance is Meyer's, from `Tox=1200n`: not
    /// modelled.
    pub const CD4007_N: Self = Self {
        threshold_volts: 2.0,
        beta: 111e-6 * 3.0,
        channel_modulation: 0.01,
        ..Self::DEFAULT_N
    };

    /// CD4007's P device: Analog Devices' teaching card (`Level=1
    /// Kp=55u Vto=-1.5 Lambda=0.04 Phi=.6 Gamma=0`) at its 60 µm / 10 µm
    /// channel.
    pub const CD4007_P: Self = Self {
        polarity: Polarity::P,
        threshold_volts: -1.5,
        beta: 55e-6 * 6.0,
        channel_modulation: 0.04,
        ..Self::DEFAULT_N
    };

    /// Which bulk junctions store charge: (bulk–drain, bulk–source).
    pub fn stored_charges(&self) -> [bool; 2] {
        [!self.bulk_drain_capacitance.is_none(), !self.bulk_source_capacitance.is_none()]
    }

    /// `[(q, C); 2]`: the bulk–drain and bulk–source junctions' charges
    /// and capacitances at the intrinsic `vbd`, `vbs` (`mos1load.c`).
    #[inline]
    pub fn bulk_charges(&self, vbd: f64, vbs: f64) -> [(f64, f64); 2] {
        let s = self.polarity.sign();
        let (qd, cd) = self.bulk_drain_capacitance.charge(s * vbd);
        let (qs, cs) = self.bulk_source_capacitance.charge(s * vbs);
        [(s * qd, cd), (s * qs, cs)]
    }

    /// The channel at `vds ≥ 0` (N sense): `(Id, ∂/∂vgs, ∂/∂vds, ∂/∂vbs)`.
    #[inline]
    fn forward(&self, vgs: f64, vds: f64, vbs: f64) -> [f64; 4] {
        let phi = self.surface_potential;
        let root_phi = phi.sqrt();
        let (sarg, dsarg) = if vbs <= 0.0 {
            let s = (phi - vbs).sqrt();
            (s, -0.5 / s)
        } else {
            let s = root_phi - vbs / (2.0 * root_phi);
            if s > 0.0 {
                (s, -0.5 / root_phi)
            } else {
                (0.0, 0.0)
            }
        };
        let gamma = self.body_effect;
        let von = self.polarity.sign() * self.threshold_volts + gamma * (sarg - root_phi);
        let vgst = vgs - von;
        if vgst <= 0.0 {
            return [0.0; 4];
        }
        let (beta, lambda) = (self.beta, self.channel_modulation);
        let betap = beta * (1.0 + lambda * vds);
        let (id, gm, gds) = if vgst <= vds {
            let half = 0.5 * vgst * vgst;
            (betap * half, betap * vgst, lambda * beta * half)
        } else {
            let cpart = vds * (vgst - 0.5 * vds);
            (betap * cpart, betap * vds, betap * (vgst - vds) + lambda * beta * cpart)
        };
        [id, gm, gds, -gm * gamma * dsarg]
    }
}

impl Mosfet for ShichmanHodges {
    fn polarity(&self) -> Polarity {
        self.polarity
    }

    #[inline]
    fn currents(&self, vgs: f64, vds: f64, vbs: f64) -> MosfetCurrents {
        let s = self.polarity.sign();
        let (vgs, vds, vbs) = (s * vgs, s * vds, s * vbs);
        let [id, gm, gds, gmbs] = if vds >= 0.0 {
            self.forward(vgs, vds, vbs)
        } else {
            let [id, fg, fd, fb] = self.forward(vgs - vds, -vds, vbs - vds);
            [-id, -fg, fg + fd + fb, -fb]
        };
        MosfetCurrents {
            drain: s * id,
            drain_per_vgs: gm,
            drain_per_vds: gds,
            drain_per_vbs: gmbs,
        }
    }
}
