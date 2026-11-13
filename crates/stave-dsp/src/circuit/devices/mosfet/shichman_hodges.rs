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

/// A level-1 MOSFET: SPICE's `.model NAME NMOS(LEVEL=1 …)` / `PMOS(…)`
/// at its DC parameters, with the element's `W/L` folded into `beta`.
///
/// # Models
///
/// A long-channel MOSFET's static drain current: threshold (`VTO`),
/// transconductance (`KP·W/(L − 2·LD)`), body effect (`GAMMA`, `PHI`),
/// channel-length modulation (`LAMBDA`), and ohmic drain and source
/// resistances (`RD`, `RS`), realised as resistors by a circuit method.
/// No subthreshold conduction (EKV's job), no capacitances, and no bulk
/// junction diodes (reverse-biased in any sane bias: picoamps). `VTO` is
/// the card's value, negative for an enhancement PMOS.
///
/// # Derivation
///
/// The module's equations, as ngspice's `mos1load.c` evaluates them.
///
/// # Verified
///
/// Against ngspice on the same cards, through `Circuit<Dk>`:
/// `crates/stave-test/tests/transistors_match_ngspice.rs::mosfet_family_matches_ngspice` (BSS138
/// Id against Vds for five gate voltages, the body effect exercised by a
/// source above the bulk: budget 1e-7 normalized RMS, measured 2.6e-8)
/// and `crates/stave-test/tests/transistors_match_ngspice.rs::mosfet_booster_matches_ngspice` (a
/// VN10KM common-source booster: 1e-6 at 4×, measured 4.7e-7).
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
    };

    /// VN10KM, N-channel enhancement: Siliconix's level-1 card (`VTO=1.4
    /// KP=1.55E-01 PHI=0.75 LAMBDA=2.9E-04 RD=7.60E-01 RS=7.60E-01`) at
    /// the default W = L.
    pub const VN10KM: Self = Self {
        threshold_volts: 1.4,
        beta: 0.155,
        channel_modulation: 2.9e-4,
        surface_potential: 0.75,
        drain_ohms: 0.76,
        source_ohms: 0.76,
        ..Self::DEFAULT_N
    };

    /// BSS138, N-channel enhancement: Diodes Inc.'s level-1 card
    /// (`DI_BSS138`: `VTO=1.2 KP=7.50m GAMMA=1.49 PHI=.75 LAMBDA=1.25m
    /// RD=0.196 RS=0.196 L=100U W=100U`).
    pub const BSS138: Self = Self {
        threshold_volts: 1.2,
        beta: 7.5e-3,
        channel_modulation: 1.25e-3,
        body_effect: 1.49,
        surface_potential: 0.75,
        drain_ohms: 0.196,
        source_ohms: 0.196,
        ..Self::DEFAULT_N
    };

    /// CD4007's N device: Analog Devices' teaching card (`Level=1
    /// Kp=111u Vto=2.0 Lambda=0.01 Phi=.6 Gamma=0`) at its 30 µm / 10 µm
    /// channel.
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
