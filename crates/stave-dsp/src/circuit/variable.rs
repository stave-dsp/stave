//! Turning a running circuit's knobs (N6): the parts a netlist declares
//! variable ([`Variable`]), found by name as typed values — [`Pot`],
//! [`VariableResistor`], [`VariableCapacitor`] — and set while the
//! circuit runs, gliding to each new value ([`Smoothing`]) so automation
//! never clicks.
//!
//! A method never rebuilds for a knob move. Each keeps its prepared
//! matrices for the values they were built at and corrects them by a
//! low-rank update (Holters & Zölzer, "Physical modelling of a wah-wah
//! effect pedal as a case study for application of the nodal DK method
//! to circuits with variable parts", DAFx-11): a variable part changes
//! one entry or one rank-one stamp of the system matrix, so by the
//! Woodbury identity the solved matrices move by a product through a
//! `k`×`k` system, `k` the number of variable branches — never a new
//! factorisation of the whole circuit. Each refresh starts again from
//! the prepared matrices, so nothing drifts however long a knob moves.
//!
//! - [`Dk`](crate::circuit::methods::dk::Dk): a variable resistor is an
//!   MNA branch whose row reads `v(a) − v(b) − R·i = 0`, so a new
//!   resistance is one diagonal entry, well conditioned from a short to
//!   a megohm; a variable capacitor is its companion conductance `2C/T`,
//!   a rank-one stamp.
//! - [`Wdf`](crate::circuit::methods::wdf::Wdf): the part's leaf takes
//!   its new port resistance, every adaptor above it re-adapts (one pass
//!   up its tree), and the tree's port conductance at the R-type root is
//!   a rank-one stamp of the root's nodal matrix, which updates the
//!   root's scattering rows.
//!
//! A refresh costs `O(k³ + k²·c + r·k·c)` for `r` output rows and `c`
//! columns of the reduced matrices — the size of the state space, not of
//! the netlist. While any part glides the matrices refresh every
//! [`Smoothing::interval`] samples, so a block of `N` samples refreshes
//! at most `⌈N / interval⌉ + 1` times, however many parts move.
//!
//! A moved resistor can move the DC operating point. The running
//! circuit follows it through its own dynamics; `prepare` solves it
//! again at the new values (`reset` returns to the one last solved, as
//! it must stay realtime).
//!
//! ```
//! use stave_dsp::circuit::methods::dk::Dk;
//! use stave_dsp::circuit::netlist::{Netlist, Taper, Variable};
//! use stave_dsp::circuit::Circuit;
//! use stave_dsp::Stage;
//!
//! // A treble-cut: the pot's upper leg grows as it turns up.
//! let mut netlist = Netlist::parse_spice(
//!     "* tone
//!      V1 in 0 0
//!      R1 in out 10k
//!      RTA out w 50k
//!      RTB w 0 50k
//!      C1 w 0 22n
//!      .control
//!      wrdata out v(out)
//!      .endc",
//! )
//! .unwrap();
//! netlist.add_variable(Variable::pot("tone", ["rta", "rtb"], Taper::AUDIO)).unwrap();
//! let mut tone = Circuit::<Dk>::new(&netlist).unwrap();
//! tone.drive("V1").unwrap();
//! tone.prepare(48_000.0);
//! let knob = tone.pot("tone").unwrap();
//! let dark = tone.analog_response(5_000.0).unwrap().magnitude();
//! tone.set_position(knob, 1.0); // glides there over the next few ms
//! for _ in 0..4_800 {
//!     tone.tick(0.0);
//! }
//! let bright = tone.analog_response(5_000.0).unwrap().magnitude();
//! assert!(bright > dark, "{bright} > {dark}");
//! ```

use crate::circuit::netlist::{ElementKind, Netlist, NetlistError, Taper, Variable};
use crate::units::{ms, Seconds};

/// A pot or rheostat of a running circuit, set by position (0 to 1).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Pot(pub(crate) usize);

/// A variable resistor of a running circuit, set in ohms.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct VariableResistor(pub(crate) usize);

/// A variable capacitor of a running circuit, set in farads.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct VariableCapacitor(pub(crate) usize);

/// How a set value is reached: each part glides toward its target
/// through two one-poles of time constant `time / 2` each (critically
/// damped, so block-rate automation's staircase leaves a ripple falling
/// at 12 dB/octave), and while anything glides the
/// circuit's matrices refresh every `interval` samples (and once more
/// on arrival). `time` zero jumps at the next sample.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Smoothing {
    pub time: Seconds,
    pub interval: u32,
}

impl Smoothing {
    /// 5 ms, refreshed every sample while gliding: a pot swept end to
    /// end by 64-sample automation under a 1 kHz tone sprays −102 dB off
    /// the carrier, against −62 dB stepped; refreshing every 4, 8, 16
    /// samples costs a quarter, an eighth, a sixteenth as much and sprays
    /// −87, −80, −74 dB (`crates/stave-test/tests/variable_parts.rs`).
    pub const DEFAULT: Self = Self {
        time: ms(5.0),
        interval: 1,
    };
    /// No glide: a set value applies, whole, at the next sample.
    pub const NONE: Self = Self {
        time: Seconds::new(0.0),
        interval: 1,
    };
}

impl Default for Smoothing {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// How a part's value becomes its branches' values.
#[derive(Copy, Clone, Debug, PartialEq)]
enum Law {
    Pot { taper: Taper, track: f64, end: f64 },
    Rheostat { taper: Taper, track: f64, end: f64 },
    Resistor,
    Capacitor,
}

/// What a variable branch is, in the netlist.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum BranchKind {
    Resistor,
    Capacitor,
}

/// One element a part varies: a resistor (its value in ohms) or a
/// capacitor (in farads).
#[derive(Clone, Debug)]
pub(crate) struct Branch {
    pub element: String,
    pub kind: BranchKind,
}

#[derive(Clone, Debug)]
struct Part {
    name: String,
    law: Law,
    /// In the part's own unit: a position, ohms or farads.
    target: f64,
    /// The glide's first stage, and its output: the value in effect.
    middle: f64,
    current: f64,
    first: usize,
}

impl Part {
    fn arrived(&self) -> bool {
        let scale = match self.law {
            Law::Pot { .. } | Law::Rheostat { .. } => 1.0,
            Law::Resistor | Law::Capacitor => self.target.abs(),
        };
        let close = 1e-6 * scale;
        (self.target - self.current).abs() <= close && (self.target - self.middle).abs() <= close
    }
}

/// A running circuit's variable parts: their targets, their glides, and
/// the values in effect for each branch.
#[derive(Clone, Debug, Default)]
pub(crate) struct Variables {
    parts: Vec<Part>,
    pub branches: Vec<Branch>,
    /// Each branch's value in effect: ohms or farads.
    pub values: Vec<f64>,
    smoothing: Smoothing,
    alpha: f64,
    countdown: u32,
    moving: bool,
}

fn refuse(element: &str, reason: &str) -> NetlistError {
    NetlistError::Unsupported {
        element: element.to_string(),
        reason: reason.to_string(),
    }
}

fn ohms_of(netlist: &Netlist, element: &str) -> f64 {
    match netlist.element(element).map(|e| &e.kind) {
        Some(ElementKind::Resistor { ohms, .. }) => *ohms,
        _ => f64::NAN,
    }
}

fn farads_of(netlist: &Netlist, element: &str) -> f64 {
    match netlist.element(element).map(|e| &e.kind) {
        Some(ElementKind::Capacitor { farads, .. }) => *farads,
        _ => f64::NAN,
    }
}

impl Variables {
    /// The netlist's declared variables, each at the value its elements
    /// were written at (a pot's position read back from its legs).
    pub fn new(netlist: &Netlist) -> Result<Self, NetlistError> {
        let mut v = Self::default();
        for variable in netlist.variables() {
            let first = v.branches.len();
            let resistor = |element: &str| Branch {
                element: element.to_string(),
                kind: BranchKind::Resistor,
            };
            let (law, value) = match variable {
                Variable::Pot {
                    name,
                    legs,
                    taper,
                    end_ohms,
                } => {
                    let (a, b) = (ohms_of(netlist, &legs[0]), ohms_of(netlist, &legs[1]));
                    let track = a + b - 2.0 * end_ohms;
                    if !(track > 0.0 && track.is_finite() && a >= *end_ohms && b >= *end_ohms) {
                        return Err(refuse(name, "each leg holds at least its end resistance, and the track is positive"));
                    }
                    v.branches.extend([resistor(&legs[0]), resistor(&legs[1])]);
                    let law = Law::Pot {
                        taper: *taper,
                        track,
                        end: *end_ohms,
                    };
                    (law, taper.position((a - end_ohms) / track))
                }
                Variable::Rheostat {
                    element,
                    track_ohms,
                    taper,
                    end_ohms,
                } => {
                    let r = ohms_of(netlist, element);
                    if !(r >= *end_ohms && r <= end_ohms + track_ohms) {
                        return Err(refuse(element, "a rheostat's resistance lies on its track"));
                    }
                    v.branches.push(resistor(element));
                    let law = Law::Rheostat {
                        taper: *taper,
                        track: *track_ohms,
                        end: *end_ohms,
                    };
                    (law, taper.position((r - end_ohms) / track_ohms))
                }
                Variable::Resistor { element } => {
                    v.branches.push(resistor(element));
                    (Law::Resistor, ohms_of(netlist, element))
                }
                Variable::Capacitor { element } => {
                    v.branches.push(Branch {
                        element: element.clone(),
                        kind: BranchKind::Capacitor,
                    });
                    (Law::Capacitor, farads_of(netlist, element))
                }
            };
            v.parts.push(Part {
                name: variable.name().to_string(),
                law,
                target: value,
                middle: value,
                current: value,
                first,
            });
        }
        // As written: a circuit with nothing moved is the netlist exactly.
        v.values = v
            .branches
            .iter()
            .map(|b| match b.kind {
                BranchKind::Resistor => ohms_of(netlist, &b.element),
                BranchKind::Capacitor => farads_of(netlist, &b.element),
            })
            .collect();
        v.countdown = 1;
        // A method is built prepared at 48 kHz.
        v.prepare(48_000.0);
        Ok(v)
    }

    /// The branch an element is, if it is one.
    pub fn branch(&self, element: &str) -> Option<usize> {
        self.branches.iter().position(|b| b.element == element)
    }

    /// The glide's coefficient at `sample_rate`.
    pub fn prepare(&mut self, sample_rate: f32) {
        // Two one-poles of half the time each: a staircase of block-rate
        // automation leaves a ripple falling at 12 dB/octave, not 6.
        let samples = 0.5 * self.smoothing.time.get() * f64::from(sample_rate);
        self.alpha = if samples > 0.0 { 1.0 - (-1.0 / samples).exp() } else { 1.0 };
    }

    pub fn set_smoothing(&mut self, smoothing: Smoothing, sample_rate: f32) {
        self.smoothing = Smoothing {
            interval: smoothing.interval.max(1),
            ..smoothing
        };
        self.prepare(sample_rate);
    }

    /// The part called `name`, if its law is one `accepts` takes.
    fn find(&self, name: &str, what: &str, accepts: fn(&Law) -> bool) -> Result<usize, NetlistError> {
        let name = name.to_ascii_lowercase();
        let i = self
            .parts
            .iter()
            .position(|p| p.name == name)
            .ok_or_else(|| NetlistError::UnknownElement { name: name.clone() })?;
        if accepts(&self.parts[i].law) {
            Ok(i)
        } else {
            Err(refuse(&name, &format!("not a {what}")))
        }
    }

    pub fn find_pot(&self, name: &str) -> Result<Pot, NetlistError> {
        self.find(name, "pot or rheostat", |l| matches!(l, Law::Pot { .. } | Law::Rheostat { .. }))
            .map(Pot)
    }

    pub fn find_resistor(&self, name: &str) -> Result<VariableResistor, NetlistError> {
        self.find(name, "variable resistor", |l| *l == Law::Resistor)
            .map(VariableResistor)
    }

    pub fn find_capacitor(&self, name: &str) -> Result<VariableCapacitor, NetlistError> {
        self.find(name, "variable capacitor", |l| *l == Law::Capacitor)
            .map(VariableCapacitor)
    }

    /// Aim part `i` at `value` (a position is clamped to 0–1; ohms and
    /// farads to zero and above).
    pub fn set(&mut self, i: usize, value: f64) {
        let Some(part) = self.parts.get_mut(i) else {
            return;
        };
        let value = match part.law {
            Law::Pot { .. } | Law::Rheostat { .. } => value.clamp(0.0, 1.0),
            Law::Resistor | Law::Capacitor => value.max(0.0),
        };
        if value.is_nan() || value == part.target {
            return;
        }
        part.target = value;
        if !self.moving {
            self.countdown = 1;
        }
        self.moving = true;
    }

    /// Part `i`'s value in effect, in its own unit.
    pub fn current(&self, i: usize) -> f64 {
        self.parts.get(i).map_or(f64::NAN, |p| p.current)
    }

    /// Something is gliding.
    #[inline]
    pub fn moving(&self) -> bool {
        self.moving
    }

    /// One sample of glide; true when the branch values changed and the
    /// circuit's matrices want refreshing.
    pub fn advance(&mut self) -> bool {
        let mut moving = false;
        for part in &mut self.parts {
            if part.current != part.target || part.middle != part.target {
                part.middle += self.alpha * (part.target - part.middle);
                part.current += self.alpha * (part.middle - part.current);
                if part.arrived() {
                    part.middle = part.target;
                    part.current = part.target;
                } else {
                    moving = true;
                }
            }
        }
        self.moving = moving;
        self.countdown = self.countdown.saturating_sub(1);
        if self.countdown == 0 || !moving {
            self.countdown = self.smoothing.interval;
            return self.compute();
        }
        false
    }

    /// Every part at its target now; true when a branch value changed.
    pub fn settle(&mut self) -> bool {
        for part in &mut self.parts {
            part.middle = part.target;
            part.current = part.target;
        }
        self.moving = false;
        self.countdown = 1;
        self.compute()
    }

    /// Take `other`'s targets and values (the same declarations).
    pub fn mirror(&mut self, other: &Self) {
        for (mine, theirs) in self.parts.iter_mut().zip(&other.parts) {
            mine.target = theirs.target;
            mine.middle = theirs.middle;
            mine.current = theirs.current;
        }
        self.values.copy_from_slice(&other.values);
        self.moving = other.moving;
    }

    /// The branch values from the parts' current values; true when one
    /// changed.
    fn compute(&mut self) -> bool {
        let mut changed = false;
        let values = &mut self.values;
        let mut put = |at: usize, value: f64| {
            if values[at] != value {
                values[at] = value;
                changed = true;
            }
        };
        for part in &self.parts {
            let x = part.current;
            let at = part.first;
            match part.law {
                Law::Pot { taper, track, end } => {
                    let f = taper.fraction(x);
                    put(at, end + track * f);
                    put(at + 1, end + track * (1.0 - f));
                }
                Law::Rheostat { taper, track, end } => put(at, end + track * taper.fraction(x)),
                Law::Resistor | Law::Capacitor => put(at, x),
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests;
