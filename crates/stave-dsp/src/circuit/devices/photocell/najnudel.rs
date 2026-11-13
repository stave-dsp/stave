//! A vactrol's parameters: the cell, the LED, the coupling between them
//! and the light history, with the parts as constants.

use crate::circuit::devices::diode::Shockley;

/// A photocell or vactrol part.
///
/// # Models
/// A vactrol: a CdS photoresistor and the LED that lights it, sealed
/// together. Parts: [`VTL5C3`](Self::VTL5C3) (PerkinElmer, type-3
/// material: steep slope, 10 MΩ dark, small light history; the opto
/// part most audio circuits use), [`NSL32`](Self::NSL32) (Advanced
/// Photonix NSL-32SR2: low on-resistance, fast).
///
/// # Derivation
/// The cell is Najnudel, Müller, Hélie & Roze's (DAFx-23, eqs. 4, 7, 8;
/// [`Cell`]); light enters as a carrier-generation current. Per part,
/// `scripts/vactrol-fit.py` fits, to the digitised datasheet tables
/// (`crates/stave-test/datasheet/vtl5c3_*.txt`, `nsl32_*.txt`): the LED
/// as a Shockley card to the input characteristic; the coupling
/// ([`Coupling`]) by inverting the cell's steady state at each reading
/// of the dark-adapted transfer curve; the light-history ratio
/// ([`History`]) from the light-adapted curve; and the cell's rates to
/// the response times. The paper's mobilities (Spear & Mort) and light
/// resistance are kept.
///
/// # Verified
/// `crates/stave-test/tests/photocell_matches_datasheet.rs::vtl5c3_matches_its_transfer_curves`
/// and its siblings: transfer curves within the reading plus 10% of
/// resistance, response times and LED as recorded there.
///
/// # Cost
/// Datasheet `"Vactrol (VTL5C3), 10 mA"`.
///
/// # Aliasing
/// None: parameters (the running [`Vactrol`](super::Vactrol) has none either).
///
/// # Valid ranges
/// The datasheets' ranges: LED current about 0.2–40 mA (VTL5C3),
/// 0.1–40 mA (NSL-32); below, the coupling extrapolates its steepest
/// slope, and the sheets warn that real units vary most there.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Najnudel {
    /// The LED, as a diode card (its bulk resistance as `series_ohms`).
    pub led: Shockley,
    pub coupling: Coupling,
    pub cell: Cell,
    pub history: History,
}

/// The photoresistor: two carrier populations and how they make a
/// resistance (Najnudel et al., DAFx-23, eqs. 4, 7, 8). Charges are in
/// coulombs with the paper's unit capacities (C± = 1 F), so a rate is
/// per coulomb-second.
///
/// ```text
///   dq⁻/dt = g − ν⁻·(q⁻ − q⁺)·q⁻          electrons, recombining with ionised defects
///   dq⁺/dt = g − ν⁺·(qτ + q⁺ − q⁻)·q⁺     holes, captured by bound defects
///   R = 1/(μ⁺·q⁺ + μ⁻·q⁻),   R_cell = Rd ∥ (R + Rℓ)
/// ```
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Cell {
    /// Rd, ohms: the cell unlit.
    pub dark_ohms: f64,
    /// Rℓ, ohms: the floor under full light.
    pub light_ohms: f64,
    /// μ⁺, 1/(V·s).
    pub hole_mobility: f64,
    /// μ⁻, 1/(V·s).
    pub electron_mobility: f64,
    /// qτ, coulombs: the defects' total charge.
    pub defects: f64,
    /// ν⁺, 1/(C·s): hole capture.
    pub hole_rate: f64,
    /// ν⁻, 1/(C·s): electron recombination.
    pub electron_rate: f64,
}

impl Cell {
    /// (dq⁺/dt, dq⁻/dt) at charges `y = [q⁺, q⁻]` under generation `g`.
    #[inline]
    pub fn rates(&self, y: [f64; 2], g: f64) -> [f64; 2] {
        let [p, m] = y;
        [
            g - self.hole_rate * (self.defects + p - m) * p,
            g - self.electron_rate * (m - p) * m,
        ]
    }

    /// ∂(rates)/∂(q⁺, q⁻).
    #[inline]
    pub fn jacobian(&self, y: [f64; 2]) -> [[f64; 2]; 2] {
        let [p, m] = y;
        let (hr, er) = (self.hole_rate, self.electron_rate);
        [
            [-hr * (self.defects + 2.0 * p - m), hr * p],
            [er * m, -er * (2.0 * m - p)],
        ]
    }

    /// μ⁺·q⁺ + μ⁻·q⁻, siemens.
    #[inline]
    pub fn photoconductance(&self, holes: f64, electrons: f64) -> f64 {
        self.hole_mobility * holes + self.electron_mobility * electrons
    }

    /// The cell's resistance at photoconductance `siemens` (eq. 8).
    #[inline]
    pub fn ohms(&self, siemens: f64) -> f64 {
        if siemens <= 0.0 || !siemens.is_finite() {
            return self.dark_ohms;
        }
        let r = 1.0 / siemens + self.light_ohms;
        self.dark_ohms * r / (self.dark_ohms + r)
    }

    /// The charges `[q⁺, q⁻]` at rest under generation `g`: with
    /// `d = q⁻ − q⁺`, the hole balance gives `q⁺ = g/(ν⁺·(qτ − d))` and
    /// the electron balance `ν⁻·d·(q⁺ + d) = g`, increasing in `d` on
    /// `(0, qτ)`: bisection to rounding.
    pub fn rest(&self, g: f64) -> [f64; 2] {
        if g <= 0.0 {
            return [0.0, 0.0];
        }
        let holes = |d: f64| g / (self.hole_rate * (self.defects - d));
        let excess = |d: f64| self.electron_rate * d * (holes(d) + d) - g;
        let (mut lo, mut hi) = (0.0, self.defects);
        for _ in 0..200 {
            let mid = 0.5 * (lo + hi);
            if mid <= lo || mid >= hi {
                break;
            }
            if excess(mid) > 0.0 {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        let d = 0.5 * (lo + hi);
        let p = holes(d);
        [p, p + d]
    }
}

/// A line in log-log: `ln y = ln_gain + slope·ln x`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Line {
    pub ln_gain: f64,
    pub slope: f64,
}

/// LED electrical power to the carrier-generation current it gives the
/// cell: the smooth minimum of two lines in log-log,
/// `ln g = m − w·ln(1 + e^(−|l₁ − l₂|/w))`, `m = min(l₁, l₂)`, steep
/// at low power (the LED's efficiency rising with current) and shallow
/// at high. Fitted per part; the paper's own law (a sum of two powers)
/// bends the other way and its printed coefficients cancel below 56 mW.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Coupling {
    pub low: Line,
    pub high: Line,
    /// w, in nepers: how gradually one line hands over to the other.
    pub softness: f64,
}

impl Coupling {
    /// Generation current (A) at `watts` of LED electrical power.
    pub fn generation(&self, watts: f64) -> f64 {
        if watts <= 0.0 || !watts.is_finite() {
            return 0.0;
        }
        let u = watts.ln();
        let l1 = self.low.ln_gain + self.low.slope * u;
        let l2 = self.high.ln_gain + self.high.slope * u;
        (l1.min(l2) - self.softness * (-(l1 - l2).abs() / self.softness).exp().ln_1p()).exp()
    }
}

/// The light-history effect: a cell that has lived in the light reads
/// higher resistance at a given light than one kept dark, by a ratio
/// RLH/RDH that is largest at low light (PerkinElmer's handbook: 1.5 at
/// 0.01 fc to 1.1 at 100 fc for type-3 material). The ratio is a power
/// law of the cell's photoconductance, `ln ratio = ln_ratio + slope·ln(G/G₁)`
/// (`G₁` its conductance at rest at 1 mA, dark adapted), clamped to
/// `0..=max_ln_ratio`; a running cell applies `ratio^a`, its adaptation
/// `a` drifting with time constant `seconds` toward `g/(g + g₁)` of the
/// light it sees (`g₁` the generation at 1 mA): 0 kept dark, 1 under
/// the datasheet's adapting light (0.97 at the VTL5C3's 40 mA).
///
/// The handbook says 24 hours achieves either extreme and publishes no
/// time constant; `seconds` is five hours (24 h ≈ 4.8 τ, so 99%), an
/// assumption.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct History {
    pub ln_ratio: f64,
    pub slope: f64,
    pub max_ln_ratio: f64,
    /// G₁, siemens.
    pub reference_siemens: f64,
    /// g₁, amperes.
    pub reference_generation: f64,
    pub seconds: f64,
}

impl History {
    /// None: the part's sheet publishes no light-history curve.
    pub const NONE: Self = Self {
        ln_ratio: 0.0,
        slope: 0.0,
        max_ln_ratio: 0.0,
        reference_siemens: 1.0,
        reference_generation: 1.0,
        seconds: 5.0 * 3600.0,
    };

    /// RLH/RDH at photoconductance `siemens`.
    pub fn ratio(&self, siemens: f64) -> f64 {
        if self.max_ln_ratio == 0.0 {
            return 1.0;
        }
        let ln = if siemens > 0.0 {
            self.ln_ratio + self.slope * (siemens / self.reference_siemens).ln()
        } else {
            self.max_ln_ratio
        };
        ln.clamp(0.0, self.max_ln_ratio).exp()
    }
}

impl Najnudel {
    /// PerkinElmer VTL5C3 (type-3 CdS; 10 MΩ dark, 2.5 ms turn-on, 35 ms
    /// maximum turn-off to 100 kΩ). LED: IS, N, RS fitted to the input
    /// characteristic (8.4 mV worst; N held at 1, the curve's knee is
    /// sharper than any N ≥ 1 allows). Coupling to the dark-adapted
    /// transfer curve (curve 1; 8% worst in generation, 5% in
    /// resistance), light history to curve 2 over curve 1 (1.15–1.33×,
    /// 3% worst; clamped at the handbook's 1.5), the rates to the
    /// response figure: turn-on to 63% in 4.25 ms at both 10 and 40 mA
    /// (the figure: 5.6 and 3.3 ms, so −24% and +31%), turn-off to
    /// 100 kΩ in 16.8 and 22.3 ms (the figure: 16.8 and 21.0). The hole
    /// rate sits at the fit's bound, 1e11: holes settle within a sample
    /// and the fit does not move above it.
    pub const VTL5C3: Self = Self {
        led: Shockley {
            saturation_amps: 1.997_658_428e-28,
            emission: 1.0,
            series_ohms: 2.957_960_942,
            ..Shockley::DEFAULT
        },
        coupling: Coupling {
            low: Line {
                ln_gain: 17.540_977_63,
                slope: 3.831_667_635,
            },
            high: Line {
                ln_gain: -2.526_226_409,
                slope: 0.872_197_226_7,
            },
            softness: 1.168_141_419,
        },
        cell: Cell {
            dark_ohms: 10e6,
            light_ohms: 2.0,
            hole_mobility: 4.0,
            electron_mobility: 35.0,
            defects: 6.478_176_758e-7,
            hole_rate: 1e11,
            electron_rate: 3.626_406_053e8,
        },
        history: History {
            ln_ratio: 0.238_676_840_9,
            slope: -0.031_057_276_65,
            max_ln_ratio: 0.405_465_108_1, // ln 1.5: the handbook's type 3 at 0.01 fc
            reference_siemens: 2.590_272_809e-5,
            reference_generation: 1.667_356_7e-4,
            seconds: 5.0 * 3600.0,
        },
    };

    /// Advanced Photonix NSL-32SR2 (5 MΩ dark typical, 5 ms rise, 5 ms
    /// decay to 100 kΩ). Its sheet draws no LED curve: the LED is
    /// [`Shockley::LED_RED`] (1.85 V at 20 mA, inside the 2.5 V maximum).
    /// Coupling to the transfer curve (five drawn vertices, 0.4%); the
    /// rates and defect charge to the printed decay, 6.2 ms against 5
    /// (+24%). The printed 5 ms rise (at an unstated current) is out of
    /// reach: no CdS cell turns off as fast as it turns on, and the fit
    /// holds the release, which is what a compressor or tremolo is
    /// heard by — the rise is 0.7 ms. Both rates sit at the fit's bound
    /// (1e11). No light-history curve is published: none is modelled.
    pub const NSL32: Self = Self {
        led: Shockley::LED_RED,
        coupling: Coupling {
            low: Line {
                ln_gain: 199.996_449_5,
                slope: 10.678_129_08,
            },
            high: Line {
                ln_gain: 2.063_918_687,
                slope: 0.107_396_502_3,
            },
            softness: 45.796_031_61,
        },
        cell: Cell {
            dark_ohms: 5e6,
            light_ohms: 2.0,
            hole_mobility: 4.0,
            electron_mobility: 35.0,
            defects: 2.692_631_913e-8,
            hole_rate: 1e11,
            electron_rate: 1e11,
        },
        history: History::NONE,
    };

    /// The LED's forward voltage at `amps`.
    pub fn led_volts(&self, amps: f64) -> f64 {
        let led = &self.led;
        if amps <= 0.0 {
            return 0.0;
        }
        led.emission * led.thermal_volts * (amps / led.saturation_amps).ln_1p() + amps * led.series_ohms
    }

    /// The generation current the cell sees with `amps` through the LED.
    pub fn generation(&self, amps: f64) -> f64 {
        self.coupling.generation(amps * self.led_volts(amps))
    }
}
