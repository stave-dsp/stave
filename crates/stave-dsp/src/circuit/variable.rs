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
//! The parts are hardware (H6): a pot's law may be its manufacturer's
//! ([`Taper::Curve`]); a second gang strays from the first by a drawn
//! [`Tracking`]; a worn [`Wiper`] crackles while it moves (the circuit
//! refreshes every sample then, and is exactly the clean circuit again
//! once still); a [`Switch`] jumps between its two resistances, with no
//! glide — so it pops by whatever DC the circuit leaves across it — or
//! ramps in log-resistance over a set time (click suppression), its
//! contacts bouncing on closing if declared so ([`Bounce`]). None of it
//! applies unless declared: a plain pot is exactly N6's.
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

use crate::circuit::devices::potentiometer::{Bounce, Tracking, Wiper, MAX_BOUNCES};
use crate::circuit::netlist::{ElementKind, Netlist, NetlistError, Taper, Variable};
use crate::primitives::random::SplitMix64;
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

/// A switch of a running circuit, thrown open or closed (H6).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Switch(pub(crate) usize);

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
    Pot { taper: Taper, track: f64, end: f64, tracking: Tracking },
    Rheostat { taper: Taper, track: f64, end: f64, tracking: Tracking },
    Resistor,
    Capacitor,
    /// Its value is how closed it is, 0 to 1, in log-resistance.
    Switch { on: f64, off: f64, ramp: f64, bounce: Option<Bounce> },
}

/// A scratchy wiper's running state.
#[derive(Clone, Debug)]
struct Scratch {
    wiper: Wiper,
    rng: SplitMix64,
    alpha: f64,
    norm: f64,
    noise: f64,
    last: f64,
    /// The track-fraction offset in effect.
    jitter: f64,
}

/// A switch's contact: its chatter after the last closing.
#[derive(Clone, Debug, Default)]
struct Contact {
    edges: [u32; 2 * MAX_BOUNCES],
    count: usize,
    /// Samples since the last closing, while its chatter lasts.
    since: Option<u32>,
    closings: u64,
}

impl Contact {
    /// Whether the contact is open at this point of the chatter.
    fn open(&self) -> bool {
        let Some(t) = self.since else {
            return false;
        };
        self.edges[..self.count].chunks(2).any(|pair| t >= pair[0] && t < pair[1])
    }
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
    scratch: Option<Scratch>,
    contact: Contact,
}

impl Part {
    fn arrived(&self) -> bool {
        let scale = match self.law {
            Law::Pot { .. } | Law::Rheostat { .. } | Law::Switch { .. } => 1.0,
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
    sample_rate: f64,
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
                    tracking,
                    ..
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
                        tracking: *tracking,
                    };
                    (law, taper.position((a - end_ohms) / track))
                }
                Variable::Rheostat {
                    element,
                    track_ohms,
                    taper,
                    end_ohms,
                    tracking,
                    ..
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
                        tracking: *tracking,
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
                Variable::Switch {
                    element,
                    on_ohms,
                    off_ohms,
                    ramp_seconds,
                    bounce,
                } => {
                    let r = ohms_of(netlist, element);
                    if r.is_nan() || r <= 0.0 {
                        return Err(refuse(element, "a switch is a positive resistance"));
                    }
                    v.branches.push(resistor(element));
                    let closed = (r.ln() - on_ohms.ln()).abs() <= (r.ln() - off_ohms.ln()).abs();
                    let law = Law::Switch {
                        on: *on_ohms,
                        off: *off_ohms,
                        ramp: *ramp_seconds,
                        bounce: *bounce,
                    };
                    (law, if closed { 1.0 } else { 0.0 })
                }
            };
            let scratch = match variable {
                Variable::Pot { wiper: Some(w), .. } | Variable::Rheostat { wiper: Some(w), .. } => Some(Scratch {
                    wiper: *w,
                    rng: SplitMix64::new(w.seed),
                    alpha: 1.0,
                    norm: 1.0,
                    noise: 0.0,
                    last: value,
                    jitter: 0.0,
                }),
                _ => None,
            };
            v.parts.push(Part {
                name: variable.name().to_string(),
                law,
                target: value,
                middle: value,
                current: value,
                first,
                scratch,
                contact: Contact::default(),
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
        self.sample_rate = f64::from(sample_rate);
        for part in &mut self.parts {
            if let Some(s) = part.scratch.as_mut() {
                // A one-pole at the crackle's corner, and the gain that
                // brings its output back to unit rms: variance a/(2 − a).
                let a = 1.0 - (-std::f64::consts::TAU * s.wiper.corner_hz / f64::from(sample_rate)).exp();
                s.alpha = a.clamp(1e-9, 1.0);
                s.norm = ((2.0 - s.alpha) / s.alpha).sqrt();
                s.rng = SplitMix64::new(s.wiper.seed);
                s.noise = 0.0;
                s.jitter = 0.0;
                s.last = part.current;
            }
        }
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

    pub fn find_switch(&self, name: &str) -> Result<Switch, NetlistError> {
        self.find(name, "switch", |l| matches!(l, Law::Switch { .. })).map(Switch)
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
            Law::Switch { .. } => {
                if value >= 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
        };
        if value.is_nan() || value == part.target {
            return;
        }
        if let Law::Switch { bounce: Some(b), .. } = part.law {
            if value == 1.0 {
                let c = &mut part.contact;
                c.count = b.edges(self.sample_rate, c.closings, &mut c.edges);
                c.closings += 1;
                c.since = Some(0);
            }
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
            if let Law::Switch { ramp, .. } = part.law {
                if Self::throw(part, ramp, self.sample_rate) {
                    moving = true;
                }
                continue;
            }
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
            if let Some(s) = part.scratch.as_mut() {
                // The wiper's speed in rotations a second, through the
                // crackle; nothing once the knob is still.
                let speed = (part.current - s.last).abs() * self.sample_rate;
                s.last = part.current;
                s.noise += s.alpha * (s.rng.next_normal() - s.noise);
                s.jitter = s.wiper.scratch * speed * s.norm * s.noise;
                // A last step's jitter is undone on the next sample.
                if s.jitter != 0.0 {
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

    /// One sample of a switch: its ramp toward the target, its chatter;
    /// true while either lasts.
    fn throw(part: &mut Part, ramp: f64, sample_rate: f64) -> bool {
        let step = if ramp > 0.0 { 1.0 / (ramp * sample_rate) } else { 1.0 };
        let gap = part.target - part.current;
        part.current = if gap.abs() <= step { part.target } else { part.current + step * gap.signum() };
        part.middle = part.current;
        let c = &mut part.contact;
        if let Some(t) = c.since {
            let end = c.edges[..c.count].last().copied().unwrap_or(0);
            c.since = if t + 1 > end || part.target == 0.0 { None } else { Some(t + 1) };
        }
        part.current != part.target || c.since.is_some()
    }

    /// Every part at its target now; true when a branch value changed.
    pub fn settle(&mut self) -> bool {
        for part in &mut self.parts {
            part.middle = part.target;
            part.current = part.target;
            part.contact.since = None;
            if let Some(s) = part.scratch.as_mut() {
                s.jitter = 0.0;
                s.last = part.current;
            }
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
            mine.scratch.clone_from(&theirs.scratch);
            mine.contact.clone_from(&theirs.contact);
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
            let jitter = part.scratch.as_ref().map_or(0.0, |s| s.jitter);
            let fraction = |taper: Taper, tracking: Tracking| (tracking.fraction(taper.fraction(x)) + jitter).clamp(0.0, 1.0);
            match part.law {
                Law::Pot {
                    taper,
                    track,
                    end,
                    tracking,
                } => {
                    let f = fraction(taper, tracking);
                    put(at, end + track * f);
                    put(at + 1, end + track * (1.0 - f));
                }
                Law::Rheostat {
                    taper,
                    track,
                    end,
                    tracking,
                } => put(at, end + track * fraction(taper, tracking)),
                Law::Resistor | Law::Capacitor => put(at, x),
                Law::Switch { on, off, .. } => {
                    let closed = if part.contact.open() { 0.0 } else { x };
                    put(at, off * (on / off).powf(closed));
                }
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests;
