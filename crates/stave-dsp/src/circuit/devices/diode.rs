//! The diode family: the [`Diode`] trait and its model, the Shockley
//! junction `i = Is·(exp(v/(n·Vt)) − 1)` with SPICE's reverse breakdown
//! and series resistance, and parts as constants of its parameters
//! ([`Shockley::N4148`], [`Shockley::N34A`], the LEDs, the Zeners).

pub use super::capacitance::Depletion;

/// The diode family: a junction's current and conductance at a voltage,
/// and how a Newton step on that voltage is bounded. Every diode model
/// implements it, so a circuit solver takes any.
pub trait Diode: Copy {
    /// `(current, conductance)` at `volts` (anode to cathode).
    fn current_and_conductance(&self, volts: f64) -> (f64, f64);

    /// Bound a Newton step from `old` to `new` without moving the
    /// solution (SPICE's `pnjlim` for a junction).
    fn limit_step(&self, new: f64, old: f64) -> f64;
}

impl Diode for Shockley {
    #[inline]
    fn current_and_conductance(&self, volts: f64) -> (f64, f64) {
        Shockley::current_and_conductance(self, volts)
    }

    #[inline]
    fn limit_step(&self, new: f64, old: f64) -> f64 {
        Shockley::limit_step(self, new, old)
    }
}

/// The Shockley junction, as SPICE's level-1 diode card holds it: `IS`,
/// `N`, `RS`, and reverse breakdown `BV`, `IBV`, `NBV`.
///
/// # Models
///
/// A pn (or point-contact, or LED) junction's static I–V curve: the
/// forward exponential, reverse saturation, and reverse breakdown
/// (avalanche or Zener):
///
/// ```text
///     i(v) = Is·(exp(v/(N·Vt)) − 1)                       forward (everywhere without breakdown)
///     i(v) = −Is·(1 + (3·N·Vt/(e·v))³)        v < −3·N·Vt, reverse (with breakdown)
///     i(v) = −Is·exp(−(xbv + v)/(NBV·Vt))     v < −xbv,    breakdown
/// ```
///
/// the last two only when [`breakdown`](Self::breakdown) is set, and [`series_ohms`](Self::series_ohms) in series (a circuit
/// method places it as a resistor; the junction methods here are the
/// junction alone). Charge storage is the card's too: the depletion
/// capacitance (`CJO`, `VJ`, `M`, `FC`, as [`Depletion`]) and the
/// transit-time diffusion charge `TT·i(v)`, which a circuit method
/// integrates ([`Junction::charge`]). Sidewall capacitance, recombination
/// current (`ISR`), high injection (`IKF`) and temperature are not
/// modelled: a netlist lists what its cards ask for in
/// [`Netlist::unmodelled`](crate::circuit::netlist::Netlist::unmodelled).
///
/// # Derivation
///
/// The ideal-diode equation (Shockley 1949) as ngspice's `dioload.c`
/// evaluates it, and its breakdown as `diotemp.c` sets it up (tag
/// ngspice-42), region for region: the knee `xbv` solves
/// `IBV = Is·(exp((BV − xbv)/(NBV·Vt)) − 1 + xbv/Vt)` (`xbv = BV` when
/// `IBV < Is·BV/Vt`), so the current at `−BV` is `IBV` when `Is` is small
/// (a germanium junction's large `Is` leaves it less).
/// [`junction`](Self::junction) solves `xbv` once. ngspice's `gmin`
/// (a picosiemens across the junction) is a convergence aid, not part of
/// the model, and is left out. Parts are the published `.model` cards
/// named on each constant.
///
/// # Verified
///
/// `crates/stave-test/tests/dk_matches_ngspice.rs::diode_iv_curves_match_ngspice`:
/// each part's I–V curve (a voltage ramp through a resistor, forward
/// conduction and reverse breakdown) through `Circuit<Dk>` against ngspice
/// on the same card, normalized RMS 2e-8 to 5e-7 by part, and every part constant equal
/// to its card as the netlist parser reads it. Unit tests beside this file
/// check the partials and the breakdown knee. Its charge:
/// `crates/stave-test/tests/capacitance_matches_ngspice.rs::junction_capacitance_matches_ngspice`
/// (the 1N4148's `C(v)` from −10 V to +0.3 V: 1e-5, measured 3.1e-6),
/// `crates/stave-test/tests/capacitance_matches_ngspice.rs::small_signal_responses_match_ngspice`
/// (a reverse-biased 1N4148's pole: 1e-5 dB, 1e-4°) and
/// `crates/stave-test/tests/capacitance_matches_ngspice.rs::diode_clipper_at_10_khz_matches_ngspice`
/// (a clipper's recovery at 10 kHz: 1e-3 at 16×, measured 3.7e-4).
///
/// Against the parts (DSP.md R13):
/// `crates/stave-test/tests/matches_datasheet.rs::diodes::n4148`
/// (Vishay's 1N4148 forward curve: 37.5 mV at worst against 35.1 mV, a
/// known gap at 20 mA) and
/// `crates/stave-test/tests/matches_datasheet.rs::diodes::n34a` (the
/// 1N34A within MCC's 1.00 V maximum at 5 mA).
///
/// # Cost
///
/// One `exp_m1` per evaluation, one more with breakdown;
/// the datasheet's `"Circuit<Dk> diode clipper x10"` and
/// `"DiodeClipper (WDF, Newton)"`.
///
/// # Aliasing
///
/// None of its own (memoryless); a clipping circuit made of it aliases,
/// so run it oversampled.
///
/// # Valid ranges
///
/// Any voltage (the exponentials are evaluated without overflow inside a
/// circuit, where [`limit_step`](Self::limit_step) bounds every Newton
/// step); the cards are fitted to their datasheets' forward curves at
/// room temperature, so 27 °C ([`THERMAL_VOLTS_27C`]) is the valid one.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Shockley {
    pub saturation_amps: f64,
    pub emission: f64,
    pub thermal_volts: f64,
    /// Bulk resistance in series with the junction, ohms (SPICE `RS`).
    pub series_ohms: f64,
    /// Reverse breakdown, or `None` for a junction that never breaks down.
    pub breakdown: Option<Breakdown>,
    /// `CJO`, `VJ`, `M`, `FC`: the junction's depletion capacitance.
    pub capacitance: Depletion,
    /// `TT`: the transit time (s), whose diffusion charge is `TT·i(v)`.
    pub transit_seconds: f64,
}

/// Reverse breakdown as SPICE's diode card sets it.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Breakdown {
    /// `BV`: the reverse voltage at which the breakdown current is `IBV`,
    /// volts (positive).
    pub volts: f64,
    /// `IBV`: the current at `BV`, amps (ngspice's default is 1 mA).
    pub amps: f64,
    /// `NBV`: the breakdown exponential's emission coefficient (ngspice's
    /// default is the card's `N`).
    pub emission: f64,
}

/// kT/q at 27 °C (300.15 K), SPICE's default temperature, from the SI-exact
/// Boltzmann constant and elementary charge.
pub const THERMAL_VOLTS_27C: f64 = 1.380_649e-23 * 300.15 / 1.602_176_634e-19;

impl Shockley {
    /// SPICE's default card, `D()`: `IS` = 1e-14, `N` = 1, no series
    /// resistance or breakdown.
    pub const DEFAULT: Self = Self {
        saturation_amps: 1e-14,
        emission: 1.0,
        thermal_volts: THERMAL_VOLTS_27C,
        series_ohms: 0.0,
        breakdown: None,
        capacitance: Depletion::NONE,
        transit_seconds: 0.0,
    };

    /// 1N4148, LTspice's `standard.dio` (onsemi):
    /// `D(Is=2.52n Rs=.568 N=1.752 Cjo=4p M=.4 tt=20n)`. The card gives no
    /// breakdown.
    pub const N4148: Self = Self {
        saturation_amps: 2.52e-9,
        emission: 1.752,
        series_ohms: 0.568,
        capacitance: Depletion {
            zero_bias_farads: 4e-12,
            grading: 0.4,
            ..Depletion::NONE
        },
        transit_seconds: 20e-9,
        ..Self::DEFAULT
    };

    /// 1N914, LTspice's `standard.dio` (onsemi): the same die and the same
    /// card as the [`N4148`](Self::N4148).
    pub const N914: Self = Self::N4148;

    /// 1N4007 (a power rectifier): `D(Is=7.03n N=1.8 Rs=34m)`.
    pub const N4007: Self = Self {
        saturation_amps: 7.03e-9,
        emission: 1.8,
        series_ohms: 0.034,
        ..Self::DEFAULT
    };

    /// 1N34A (germanium point contact), the card the community-extended
    /// LTspice `standard.dio` (Bordodynov) gives and KiCad's and Qspice's
    /// libraries copy: `D(IS=2.6u RS=6.5 N=1.6 CJO=0.8p EG=0.67 BV=25
    /// IBV=0.003)`. No manufacturer publishes one.
    pub const N34A: Self = Self {
        saturation_amps: 2.6e-6,
        emission: 1.6,
        series_ohms: 6.5,
        breakdown: Some(Breakdown {
            volts: 25.0,
            amps: 3e-3,
            emission: 1.6,
        }),
        capacitance: Depletion {
            zero_bias_farads: 0.8e-12,
            ..Depletion::NONE
        },
        ..Self::DEFAULT
    };

    /// A red indicator LED (GaAsP, Vf ≈ 1.85 V at 20 mA): the generic
    /// `LedRed` card of the community-extended LTspice `standard.dio`
    /// (Bordodynov), relabelled HLMP-6000 (Avago red, 5 mm) in KSKelvin's
    /// Qspice library: `D(IS=93.2P RS=42M N=3.73 BV=4 IBV=10U CJO=2.97P
    /// VJ=.75 M=.333 TT=4.32U)`.
    pub const LED_RED: Self = Self {
        saturation_amps: 93.2e-12,
        emission: 3.73,
        series_ohms: 42e-3,
        breakdown: Some(Breakdown {
            volts: 4.0,
            amps: 10e-6,
            emission: 3.73,
        }),
        capacitance: Depletion {
            zero_bias_farads: 2.97e-12,
            potential_volts: 0.75,
            grading: 0.333,
            ..Depletion::NONE
        },
        transit_seconds: 4.32e-6,
        ..Self::DEFAULT
    };

    /// A green indicator LED (Vf ≈ 2.3 V at 20 mA): the same library's
    /// generic `LedGreen` card, `D(IS=93.2P RS=42M N=4.61 BV=4 IBV=10U …)`.
    pub const LED_GREEN: Self = Self {
        emission: 4.61,
        breakdown: Some(Breakdown {
            volts: 4.0,
            amps: 10e-6,
            emission: 4.61,
        }),
        ..Self::LED_RED
    };

    /// A blue indicator LED (InGaN, Vf ≈ 3.7 V at 20 mA): the same
    /// library's generic `LedBLUE` card, `D(IS=93.2P RS=42M N=7.47 BV=5
    /// IBV=10U …)`.
    pub const LED_BLUE: Self = Self {
        emission: 7.47,
        breakdown: Some(Breakdown {
            volts: 5.0,
            amps: 10e-6,
            emission: 7.47,
        }),
        ..Self::LED_RED
    };

    /// BZX84C6V2L, a 6.2 V Zener, LTspice's `standard.dio` (onsemi):
    /// `D(Is=1.5n Rs=.5 Cjo=185p nbv=3 bv=6.2 Ibv=1m)` — every static
    /// parameter of the card.
    pub const BZX84C6V2L: Self = Self {
        saturation_amps: 1.5e-9,
        emission: 1.0,
        series_ohms: 0.5,
        breakdown: Some(Breakdown {
            volts: 6.2,
            amps: 1e-3,
            emission: 3.0,
        }),
        capacitance: Depletion {
            zero_bias_farads: 185e-12,
            ..Depletion::NONE
        },
        ..Self::DEFAULT
    };

    /// 1N750, a 4.7 V Zener, LTspice's `standard.dio` (onsemi):
    /// `D(Is=.88f Rs=.25 Cjo=175p M=.55 nbv=1.7 bv=4.7 Vj=.75 Isr=1.86n Nr=2
    /// Ibv=20.245m Ibvl=1.96m Nbvl=15 Tbv1=-21.3u)`, its static
    /// parameters less the recombination current (`Isr`, `Nr`) and the
    /// low-current breakdown (`Ibvl`, `Nbvl`, which ngspice does not
    /// read either): the breakdown knee is the card's, the forward curve
    /// below 10 µA is not.
    pub const N750: Self = Self {
        saturation_amps: 0.88e-15,
        emission: 1.0,
        series_ohms: 0.25,
        breakdown: Some(Breakdown {
            volts: 4.7,
            amps: 20.245e-3,
            emission: 1.7,
        }),
        capacitance: Depletion {
            zero_bias_farads: 175e-12,
            potential_volts: 0.75,
            grading: 0.55,
            ..Depletion::NONE
        },
        ..Self::DEFAULT
    };

    /// The SPICE card `D1N4148` under its old name.
    #[deprecated(note = "renamed N4148 (DSP.md naming rule 3)")]
    pub const D1N4148: Self = Self::N4148;

    /// The SPICE card `D1N4007` under its old name.
    #[deprecated(note = "renamed N4007 (DSP.md naming rule 3)")]
    pub const D1N4007: Self = Self::N4007;

    pub fn knee_volts(&self) -> f64 {
        self.emission * self.thermal_volts
    }

    /// Above this voltage a Newton step is compressed (SPICE's `vcrit`).
    pub fn critical_volts(&self) -> f64 {
        critical(self.knee_volts(), self.saturation_amps)
    }

    /// The junction ready to evaluate: the breakdown knee solved once
    /// (ngspice's `diotemp.c` iteration), so each evaluation costs one
    /// exponential, two with breakdown. Circuit methods hold this.
    pub fn junction(&self) -> Junction {
        let knee = self.knee_volts();
        let breakdown = self.breakdown.map(|b| {
            let (is, vt) = (self.saturation_amps, self.thermal_volts);
            let bknee = b.emission * vt;
            let xbv = if b.amps < is * b.volts / vt {
                b.volts
            } else {
                let mut xbv = b.volts - bknee * (1.0 + b.amps / is).ln();
                for _ in 0..50 {
                    let next = b.volts - bknee * (b.amps / is + 1.0 - xbv / vt).ln();
                    let done = (next - xbv).abs() <= 1e-14 * b.volts;
                    xbv = next;
                    if done {
                        break;
                    }
                }
                xbv
            };
            JunctionBreakdown {
                xbv,
                knee: bknee,
                critical: critical(bknee, is),
            }
        });
        Junction {
            saturation_amps: self.saturation_amps,
            knee,
            critical: critical(knee, self.saturation_amps),
            breakdown,
            capacitance: self.capacitance,
            transit_seconds: self.transit_seconds,
        }
    }

    /// SPICE's `pnjlim`: a step from `old` to `new` that climbs past the
    /// critical voltage is taken logarithmically, so a Newton iterate can
    /// never jump far up the exponential; past the breakdown voltage the
    /// same holds in mirror for the breakdown exponential. Steps
    /// elsewhere are untouched.
    pub fn limit_step(&self, new: f64, old: f64) -> f64 {
        self.junction().limit_step(new, old)
    }

    /// `limit_step` for an antiparallel pair: either polarity climbs an
    /// exponential, so negative steps are limited in mirror. (A pair
    /// conducts forward long before either junction breaks down, so the
    /// pair methods leave breakdown out.)
    pub fn limit_pair_step(&self, new: f64, old: f64) -> f64 {
        let knee = self.knee_volts();
        let critical = self.critical_volts();
        if new < 0.0 {
            -pnjlim(-new, -old, knee, critical)
        } else {
            pnjlim(new, old, knee, critical)
        }
    }

    pub fn current(&self, volts: f64) -> f64 {
        self.current_and_conductance(volts).0
    }

    pub fn conductance(&self, volts: f64) -> f64 {
        self.current_and_conductance(volts).1
    }

    /// `(current, conductance)`, the pair a Newton residual needs at
    /// every iterate. With breakdown this solves the knee each call: a
    /// solver holds a [`junction`](Self::junction) instead.
    #[inline]
    pub fn current_and_conductance(&self, volts: f64) -> (f64, f64) {
        if self.breakdown.is_none() {
            let knee = self.knee_volts();
            let em1 = (volts / knee).exp_m1();
            return (
                self.saturation_amps * em1,
                self.saturation_amps / knee * (em1 + 1.0),
            );
        }
        self.junction().current_and_conductance(volts)
    }

    /// The antiparallel pair's `(current, conductance)` from one
    /// exponential and its reciprocal.
    #[inline]
    pub fn pair_current_and_conductance(&self, volts: f64) -> (f64, f64) {
        let knee = self.knee_volts();
        let e = (volts / knee).exp();
        let inv = 1.0 / e;
        (
            self.saturation_amps * (e - inv),
            self.saturation_amps / knee * (e + inv),
        )
    }

    /// An antiparallel pair: odd in `volts`.
    pub fn pair_current(&self, volts: f64) -> f64 {
        2.0 * self.saturation_amps * (volts / self.knee_volts()).sinh()
    }

    pub fn pair_conductance(&self, volts: f64) -> f64 {
        2.0 * self.saturation_amps / self.knee_volts() * (volts / self.knee_volts()).cosh()
    }

    /// The pair's mean current over `[a, b]`: the divided difference of
    /// its antiderivative `2·Is·N·Vt·cosh(v/(N·Vt))`, formed as
    /// `2·Is·sinh(v̄/(N·Vt))·sinhc(Δ/(2·N·Vt))` so `a ≈ b` loses nothing
    /// (stateful ADAA, [`methods::adaa`](crate::circuit::methods::adaa)).
    pub fn pair_mean_current(&self, a: f64, b: f64) -> f64 {
        let knee = self.knee_volts();
        let (mid, half) = (0.5 * (a + b), 0.5 * (b - a) / knee);
        2.0 * self.saturation_amps * (mid / knee).sinh() * sinhc(half)
    }

    /// The pair's secant conductance `Δi/Δv` over `[a, b]` (the
    /// conductance where `a = b`).
    pub fn pair_secant(&self, a: f64, b: f64) -> f64 {
        let knee = self.knee_volts();
        let (mid, half) = (0.5 * (a + b), 0.5 * (b - a) / knee);
        2.0 * self.saturation_amps / knee * (mid / knee).cosh() * sinhc(half)
    }
}

/// A [`Shockley`] junction prepared for evaluation (its breakdown knee
/// solved): what circuit methods hold.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Junction {
    saturation_amps: f64,
    knee: f64,
    critical: f64,
    breakdown: Option<JunctionBreakdown>,
    capacitance: Depletion,
    transit_seconds: f64,
}

#[derive(Copy, Clone, Debug, PartialEq)]
struct JunctionBreakdown {
    /// ngspice's knee `xbv`: below `−xbv` the current is the breakdown
    /// exponential.
    xbv: f64,
    knee: f64,
    critical: f64,
}

impl Junction {
    /// True when the junction stores charge (`CJO` or `TT` nonzero).
    pub fn stores_charge(&self) -> bool {
        !self.capacitance.is_none() || self.transit_seconds != 0.0
    }

    /// `(q, C)`: the stored charge and its capacitance `dq/dv` at `volts`,
    /// given the junction's `(current, conductance)` there — depletion
    /// plus the diffusion charge `TT·i` (ngspice's `dioload.c`, whose
    /// diffusion charge follows the whole junction current, breakdown
    /// included).
    #[inline]
    pub fn charge(&self, volts: f64, current: f64, conductance: f64) -> (f64, f64) {
        let (q, c) = self.capacitance.charge(volts);
        (q + self.transit_seconds * current, c + self.transit_seconds * conductance)
    }

    /// `(current, conductance)` at `volts`: one exponential. With
    /// breakdown, ngspice's three regions — forward above `−3·N·Vt`, its
    /// cubic reverse form down to `−xbv`, the breakdown exponential below.
    #[inline]
    pub fn current_and_conductance(&self, volts: f64) -> (f64, f64) {
        let is = self.saturation_amps;
        if let Some(b) = &self.breakdown {
            if volts < -3.0 * self.knee {
                if volts >= -b.xbv {
                    let arg = 3.0 * self.knee / (volts * std::f64::consts::E);
                    let arg3 = arg * arg * arg;
                    return (-is * (1.0 + arg3), is * 3.0 * arg3 / volts);
                }
                let e = (-(b.xbv + volts) / b.knee).exp();
                return (-is * e, is * e / b.knee);
            }
        }
        let em1 = (volts / self.knee).exp_m1();
        (is * em1, is / self.knee * (em1 + 1.0))
    }

    /// The antiderivative of the current, `∫₀ᵛ i dv`, continuous across
    /// the breakdown regions: `Is·(N·Vt·expm1(v/(N·Vt)) − v)` forward.
    pub fn antiderivative(&self, volts: f64) -> f64 {
        let (is, knee) = (self.saturation_amps, self.knee);
        let forward = |v: f64| is * (knee * (v / knee).exp_m1() - v);
        let Some(b) = &self.breakdown else {
            return forward(volts);
        };
        let edge = -3.0 * knee;
        if volts >= edge {
            return forward(volts);
        }
        // The reverse cubic, −Is·(1 + c³/v³), from the forward region's edge.
        let c3 = (3.0 * knee / std::f64::consts::E).powi(3);
        let cubic = |v: f64| -is * v + 0.5 * is * c3 / (v * v);
        let at_edge = forward(edge) - cubic(edge);
        if volts >= -b.xbv {
            return cubic(volts) + at_edge;
        }
        // The breakdown exponential, −Is·exp(−(xbv + v)/NBV·Vt).
        let breakdown = |v: f64| is * b.knee * (-(b.xbv + v) / b.knee).exp();
        let at_knee = cubic(-b.xbv) + at_edge - breakdown(-b.xbv);
        breakdown(volts) + at_knee
    }

    /// The mean current over `[a, b]`, `ΔI/Δv` of
    /// [`antiderivative`](Self::antiderivative); forward, in closed form
    /// without cancellation (`expm1`), so `a ≈ b` is exact
    /// (stateful ADAA, [`methods::adaa`](crate::circuit::methods::adaa)).
    pub fn mean_current(&self, a: f64, b: f64) -> f64 {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let delta = hi - lo;
        if self.forward_only(lo) {
            let is = self.saturation_amps;
            return is * ((lo / self.knee).exp() * exprel(delta / self.knee) - 1.0);
        }
        if delta <= 1e-9 * (1.0 + hi.abs()) {
            return self.current_and_conductance(0.5 * (lo + hi)).0;
        }
        (self.antiderivative(hi) - self.antiderivative(lo)) / delta
    }

    /// The secant conductance `Δi/Δv` over `[a, b]`, the conductance
    /// where `a = b`.
    pub fn secant(&self, a: f64, b: f64) -> f64 {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let delta = hi - lo;
        if self.forward_only(lo) {
            let g = self.saturation_amps / self.knee;
            return g * (lo / self.knee).exp() * exprel(delta / self.knee);
        }
        if delta <= 1e-9 * (1.0 + hi.abs()) {
            return self.current_and_conductance(0.5 * (lo + hi)).1;
        }
        (self.current_and_conductance(hi).0 - self.current_and_conductance(lo).0) / delta
    }

    /// Whether everything above `volts` is the forward exponential.
    fn forward_only(&self, volts: f64) -> bool {
        self.breakdown.is_none() || volts >= -3.0 * self.knee
    }

    /// SPICE's `pnjlim` on whichever exponential the step climbs
    /// (ngspice's `dioload.c`: the breakdown side within ten knees of
    /// `−xbv`).
    #[inline]
    pub fn limit_step(&self, new: f64, old: f64) -> f64 {
        if let Some(b) = &self.breakdown {
            if new < f64::min(0.0, -b.xbv + 10.0 * b.knee) {
                let (new_r, old_r) = (-(new + b.xbv), -(old + b.xbv));
                return -(pnjlim(new_r, old_r, b.knee, b.critical) + b.xbv);
            }
        }
        pnjlim(new, old, self.knee, self.critical)
    }
}

impl Diode for Junction {
    #[inline]
    fn current_and_conductance(&self, volts: f64) -> (f64, f64) {
        Junction::current_and_conductance(self, volts)
    }

    #[inline]
    fn limit_step(&self, new: f64, old: f64) -> f64 {
        Junction::limit_step(self, new, old)
    }
}

use super::junction::{exprel, sinhc};

/// SPICE's `vcrit` for an exponential `scale·exp(v/knee)`.
fn critical(knee: f64, scale: f64) -> f64 {
    knee * (knee / (std::f64::consts::SQRT_2 * scale)).ln()
}

/// SPICE's `pnjlim` (Nagel, SPICE2, 1975) on one exponential.
fn pnjlim(new: f64, old: f64, knee: f64, critical: f64) -> f64 {
    if new <= critical || (new - old).abs() <= 2.0 * knee {
        return new;
    }
    if old > 0.0 {
        let arg = 1.0 + (new - old) / knee;
        if arg > 0.0 {
            old + knee * arg.ln()
        } else {
            critical
        }
    } else {
        knee * (new / knee).ln()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PARTS: [Shockley; 9] = [
        Shockley::N4148,
        Shockley::N914,
        Shockley::N4007,
        Shockley::N34A,
        Shockley::LED_RED,
        Shockley::LED_GREEN,
        Shockley::LED_BLUE,
        Shockley::BZX84C6V2L,
        Shockley::N750,
    ];

    #[test]
    fn a_pair_is_two_opposed_diodes() {
        let d = Shockley::N4148;
        for v in [-0.6, -0.1, 0.0, 0.3, 0.7] {
            let pair = d.current(v) - d.current(-v);
            assert!((d.pair_current(v) - pair).abs() < 1e-15 + 1e-9 * pair.abs());
        }
    }

    #[test]
    fn the_conductance_is_the_derivative_of_the_current() {
        for d in PARTS {
            for v in [-30.0, -6.5, -6.2, -4.8, -1.0, 0.0, 0.3, 0.7, 2.0] {
                let (_, g) = d.current_and_conductance(v);
                let h = 1e-6;
                let numeric = (d.current(v + h) - d.current(v - h)) / (2.0 * h);
                assert!(
                    (g - numeric).abs() <= 1e-5 * numeric.abs() + 1e-15,
                    "{d:?} at {v}: {g} vs {numeric}"
                );
            }
        }
    }

    /// At `−BV` a Zener carries `IBV` (ngspice's knee is fitted to it)
    /// when `Is` is small; the 1N34A's large `Is` leaves it less, as in
    /// ngspice.
    #[test]
    fn a_zener_carries_ibv_at_bv() {
        for d in [Shockley::BZX84C6V2L, Shockley::N750, Shockley::LED_BLUE] {
            let b = d.breakdown.unwrap();
            let i = -d.current(-b.volts);
            assert!((i / b.amps - 1.0).abs() < 1e-3, "{d:?}: {i}");
        }
        assert!(
            Shockley::N4148.current(-50.0) > -1e-8,
            "no breakdown without BV"
        );
        let germanium = -Shockley::N34A.current(-25.0);
        assert!((1e-4..3e-3).contains(&germanium), "{germanium}");
    }

    /// The LEDs' forward voltages at 20 mA, junction plus series drop.
    #[test]
    fn leds_light_at_their_colours_forward_voltage() {
        for (d, vf) in [
            (Shockley::LED_RED, 1.85),
            (Shockley::LED_GREEN, 2.29),
            (Shockley::LED_BLUE, 3.70),
        ] {
            let junction = d.knee_volts() * (0.02 / d.saturation_amps).ln_1p();
            let total = junction + 0.02 * d.series_ohms;
            assert!((total - vf).abs() < 0.02, "{total}");
        }
    }

    /// Stateful ADAA's divided differences: the mean current is the
    /// integral of the current (a fine midpoint rule), across the
    /// breakdown regions too, and `a ≈ b` is the current itself.
    #[test]
    fn the_mean_current_is_the_integral_of_the_current() {
        for d in [Shockley::N4148, Shockley::BZX84C6V2L, Shockley::N34A] {
            let j = d.junction();
            for (a, b) in [(-0.2, 0.75), (-7.0, -5.5), (-9.0, 0.6), (0.4, 0.4 + 1e-12)] {
                let n = 200_000;
                let h = (b - a) / n as f64;
                let integral: f64 = (0..n)
                    .map(|k| j.current_and_conductance(a + (k as f64 + 0.5) * h).0)
                    .sum::<f64>()
                    / n as f64;
                let scale = integral.abs().max(1e-12);
                let mean = j.mean_current(a, b);
                assert!((mean - integral).abs() < 1e-6 * scale, "{d:?} {a}..{b}: {mean} vs {integral}");
                assert!((j.mean_current(b, a) - mean).abs() <= 1e-12 * scale);
                let secant = j.secant(a, b);
                let exact = if (b - a).abs() < 1e-9 {
                    j.current_and_conductance(a).1
                } else {
                    (j.current_and_conductance(b).0 - j.current_and_conductance(a).0) / (b - a)
                };
                assert!((secant - exact).abs() < 1e-6 * exact.abs().max(1e-12), "{a}..{b}");
            }
        }
        let pair = Shockley::N4148;
        let n = 100_000;
        let (a, b) = (-0.3, 0.8);
        let h = (b - a) / n as f64;
        let integral: f64 =
            (0..n).map(|k| pair.pair_current(a + (k as f64 + 0.5) * h)).sum::<f64>() / n as f64;
        assert!((pair.pair_mean_current(a, b) - integral).abs() < 1e-6 * integral.abs());
    }

    #[test]
    fn limiting_never_moves_a_small_step_and_tames_a_big_one() {
        let z = Shockley::BZX84C6V2L;
        assert_eq!(z.limit_step(-6.25, -6.24), -6.25);
        let tamed = z.limit_step(-40.0, -6.2);
        assert!(tamed > -7.0, "{tamed}");
        let forward = z.limit_step(20.0, 0.6);
        assert!(forward < 1.0, "{forward}");
    }
}
