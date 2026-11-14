//! Component tolerances (DSP.md E10; [`super::ageing`] handles drift): each part of a [`Netlist`] built off its marked value by a
//! seeded draw, so two built units differ as two pedals off one line
//! differ — and the same seed builds the same unit on every machine.
//!
//! A part is a resistor, capacitor or inductor, or a pot (a
//! [`Variable::Pot`]'s two legs, or a rheostat's one), whose track is one
//! part: both legs scale together, so its position stays put. A part's
//! draw depends only on the seed, the unit's number and the part's name
//! (hashed by SplitMix64), never on what else the netlist holds: adding
//! a part does not rebuild the others.
//!
//! ```
//! use stave_dsp::circuit::netlist::tolerance::{Tolerance, Tolerances};
//! use stave_dsp::circuit::netlist::Netlist;
//!
//! let rc = Netlist::parse_spice("* rc\nV1 in 0 AC 1\nR1 in out 1k\nC1 out 0 100n").unwrap();
//! let grades = Tolerances::new()
//!     .resistors(Tolerance::uniform(0.01))
//!     .capacitors(Tolerance::gaussian(0.10));
//! let unit = grades.unit(&rc, 7, 0).unwrap();
//! let r = unit.value("r1").unwrap();
//! assert!((990.0..=1010.0).contains(&r));
//! assert_eq!(unit, grades.unit(&rc, 7, 0).unwrap(), "seeded: the same unit");
//! assert_ne!(unit, grades.unit(&rc, 7, 1).unwrap(), "another unit differs");
//! ```

use super::{ElementKind, Netlist, NetlistError, Variable};
use crate::primitives::random::splitmix64;

/// How a part's value is distributed within its tolerance.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Law {
    /// Flat across ±tolerance: the conservative reading of a grade.
    Uniform,
    /// Normal with σ = tolerance / 3, truncated at ±tolerance (a graded
    /// batch: the band is three standard deviations).
    Gaussian,
}

/// A part's grade: its tolerance as a fraction of the marked value (0.05
/// for ±5 %) and how values fall within it.
///
/// # Derivation
/// A part is built at `nominal · (1 + δ)`, `δ` drawn by its [`Law`]
/// from a stream seeded by `(seed, unit, part name)`: uniform from 53
/// bits; Gaussian by Box–Muller, redrawn outside ±3σ.
///
/// # Verified
/// `crates/stave-dsp/src/circuit/netlist/tolerance/tests.rs::draws_have_their_laws_moments`
/// (10⁵ draws: mean, σ and the bounds of each law against their exact
/// values) and `crates/stave-measure/src/design/tests.rs::rc_spread_matches_its_exact_distribution`
/// (an RC filter's built units against the exact distribution of `RC`).
///
/// # Cost
/// Design time only: no audio-path cost.
///
/// # Valid ranges
/// `0 ≤ fraction < 1`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Tolerance {
    pub fraction: f64,
    pub law: Law,
}

impl Tolerance {
    /// A part built exactly to its value.
    pub const EXACT: Self = Self {
        fraction: 0.0,
        law: Law::Uniform,
    };

    pub const fn uniform(fraction: f64) -> Self {
        Self {
            fraction,
            law: Law::Uniform,
        }
    }

    pub const fn gaussian(fraction: f64) -> Self {
        Self {
            fraction,
            law: Law::Gaussian,
        }
    }

    /// The standard deviation of `δ`: `t/√3` uniform; for the Gaussian,
    /// σ = t/3 shrunk by its truncation at 3σ (`√(1 − 6φ(3)/(2Φ(3) − 1))`).
    pub fn sigma(&self) -> f64 {
        match self.law {
            Law::Uniform => self.fraction / 3f64.sqrt(),
            Law::Gaussian => self.fraction / 3.0 * TRUNCATED_SD,
        }
    }

    /// The factor `1 + δ` unit `unit` of the batch `seed` builds `part`
    /// with.
    pub fn factor(&self, seed: u64, unit: u64, part: &str) -> f64 {
        if self.fraction == 0.0 {
            return 1.0;
        }
        let mut stream = Stream::new(seed, unit, part);
        let delta = match self.law {
            Law::Uniform => 2.0 * stream.unit() - 1.0,
            Law::Gaussian => loop {
                // Box–Muller; 1 − u keeps the logarithm finite.
                let (u, v) = (1.0 - stream.unit(), stream.unit());
                let z = (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos();
                if z.abs() <= 3.0 {
                    break z / 3.0;
                }
            },
        };
        1.0 + self.fraction * delta
    }
}

/// `√(1 − 6φ(3)/(2Φ(3) − 1))`: a normal's standard deviation, in σ, once
/// truncated at ±3σ.
const TRUNCATED_SD: f64 = 0.986_578_392_558_108_6;

/// SplitMix64 from a seed, a unit and a name (FNV-1a).
struct Stream(u64);

impl Stream {
    fn new(seed: u64, unit: u64, part: &str) -> Self {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in part.to_ascii_lowercase().bytes() {
            h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
        }
        Self(splitmix64(seed ^ splitmix64(unit ^ splitmix64(h))))
    }

    /// Uniform in [0, 1), 53 bits.
    fn unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        (splitmix64(self.0) >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// One part as built: a resistor, capacitor or inductor, or a pot's (or
/// rheostat's) track over its elements.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    /// The element's name, or the pot's.
    pub name: String,
    /// The elements it sets, scaled together.
    pub elements: Vec<String>,
    /// Ohms, farads or henries: an element's value, a pot's legs summed.
    pub nominal: f64,
    pub tolerance: Tolerance,
}

impl Part {
    /// Set the part to `value` in `netlist`, its elements scaled by
    /// `value / nominal` from the values the part was read with.
    pub fn set(&self, netlist: &mut Netlist, base: &Netlist, value: f64) -> Result<(), NetlistError> {
        let k = value / self.nominal;
        for e in &self.elements {
            let v = base.value(e).ok_or_else(|| NetlistError::UnknownElement { name: e.clone() })?;
            netlist.set_value(e, v * k)?;
        }
        Ok(())
    }
}

/// The grades a netlist's parts are built to: one per kind, and any
/// part's own.
#[derive(Clone, Debug, PartialEq)]
pub struct Tolerances {
    resistors: Tolerance,
    capacitors: Tolerance,
    inductors: Tolerance,
    pots: Tolerance,
    parts: Vec<(String, Tolerance)>,
}

impl Default for Tolerances {
    fn default() -> Self {
        Self::new()
    }
}

impl Tolerances {
    /// Every part exact.
    pub fn new() -> Self {
        Self {
            resistors: Tolerance::EXACT,
            capacitors: Tolerance::EXACT,
            inductors: Tolerance::EXACT,
            pots: Tolerance::EXACT,
            parts: Vec::new(),
        }
    }

    pub fn resistors(mut self, t: Tolerance) -> Self {
        self.resistors = t;
        self
    }

    pub fn capacitors(mut self, t: Tolerance) -> Self {
        self.capacitors = t;
        self
    }

    pub fn inductors(mut self, t: Tolerance) -> Self {
        self.inductors = t;
        self
    }

    /// Pots' and rheostats' tracks (declared [`Variable`]s).
    pub fn pots(mut self, t: Tolerance) -> Self {
        self.pots = t;
        self
    }

    /// One part's own grade (an element's name, or a pot's).
    pub fn part(mut self, name: &str, t: Tolerance) -> Self {
        let name = name.to_ascii_lowercase();
        self.parts.retain(|(n, _)| *n != name);
        self.parts.push((name, t));
        self
    }

    /// The netlist's parts with their grades, in element order (a pot
    /// where its first leg is).
    pub fn parts(&self, netlist: &Netlist) -> Vec<Part> {
        let own = |name: &str, kind: Tolerance| {
            self.parts.iter().find(|(n, _)| n == name).map_or(kind, |(_, t)| *t)
        };
        let mut out: Vec<Part> = Vec::new();
        for e in netlist.elements() {
            if let Some(v) = netlist.variables().iter().find(|v| v.elements().contains(&e.name.as_str())) {
                if matches!(v, Variable::Pot { .. } | Variable::Rheostat { .. }) {
                    if out.iter().any(|p| p.name == v.name()) {
                        continue;
                    }
                    let elements: Vec<String> = v.elements().iter().map(|s| s.to_string()).collect();
                    let nominal = elements.iter().filter_map(|x| netlist.value(x)).sum();
                    out.push(Part {
                        name: v.name().to_string(),
                        elements,
                        nominal,
                        tolerance: own(v.name(), self.pots),
                    });
                    continue;
                }
            }
            let kind = match e.kind {
                ElementKind::Resistor { .. } => self.resistors,
                ElementKind::Capacitor { .. } => self.capacitors,
                ElementKind::Inductor { .. } => self.inductors,
                _ => continue,
            };
            out.push(Part {
                name: e.name.clone(),
                elements: vec![e.name.clone()],
                nominal: netlist.value(&e.name).unwrap_or(0.0),
                tolerance: own(&e.name, kind),
            });
        }
        out
    }

    /// Unit `unit` of the batch `seed`: every part built off its value by
    /// its grade. An override naming no part is an error.
    pub fn unit(&self, netlist: &Netlist, seed: u64, unit: u64) -> Result<Netlist, NetlistError> {
        let parts = self.parts(netlist);
        if let Some((name, _)) = self.parts.iter().find(|(n, _)| !parts.iter().any(|p| p.name == *n)) {
            return Err(NetlistError::UnknownElement { name: name.clone() });
        }
        let mut out = netlist.clone();
        for p in &parts {
            p.set(&mut out, netlist, p.nominal * p.tolerance.factor(seed, unit, &p.name))?;
        }
        Ok(out)
    }
}

impl Netlist {
    /// A resistor's ohms, a capacitor's farads, an inductor's henries; a
    /// controlled source's gain.
    pub fn value(&self, element: &str) -> Option<f64> {
        match self.element(element)?.kind {
            ElementKind::Resistor { ohms, .. } => Some(ohms),
            ElementKind::Capacitor { farads, .. } => Some(farads),
            ElementKind::Inductor { henries, .. } => Some(henries),
            ElementKind::VoltageControlledVoltage { gain, .. }
            | ElementKind::VoltageControlledCurrent { gain, .. }
            | ElementKind::CurrentControlledCurrent { gain, .. } => Some(gain),
            _ => None,
        }
    }

    /// Set what [`value`](Self::value) reads. A resistor, capacitor or
    /// inductor stays positive and finite.
    pub fn set_value(&mut self, element: &str, x: f64) -> Result<(), NetlistError> {
        let refuse = |reason: &str| NetlistError::Unsupported {
            element: element.to_ascii_lowercase(),
            reason: reason.into(),
        };
        let e = self
            .element_mut(element)
            .ok_or_else(|| NetlistError::UnknownElement { name: element.to_ascii_lowercase() })?;
        match &mut e.kind {
            ElementKind::Resistor { ohms: v, .. }
            | ElementKind::Capacitor { farads: v, .. }
            | ElementKind::Inductor { henries: v, .. } => {
                if !(x.is_finite() && x > 0.0) {
                    return Err(refuse("a part's value is positive and finite"));
                }
                *v = x;
            }
            ElementKind::VoltageControlledVoltage { gain, .. }
            | ElementKind::VoltageControlledCurrent { gain, .. }
            | ElementKind::CurrentControlledCurrent { gain, .. } => *gain = x,
            _ => return Err(refuse("no value to set: not a passive part or a controlled source")),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
