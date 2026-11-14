//! Parts a player turns (N6): a netlist element declared variable — a
//! potentiometer (two resistors and a wiper), a pot wired as a rheostat,
//! a variable resistor or a variable capacitor — so a running circuit
//! changes its value without being rebuilt.
//!
//! A declaration names elements the netlist already has, so one deck
//! still feeds ngspice and stave: the deck writes the pot's two legs as
//! resistors at some position, and the declaration says they are one
//! part. The position the deck was written at is read back from the
//! legs' values.
//!
//! ```
//! use stave_dsp::circuit::netlist::{Netlist, Taper, Variable};
//!
//! let mut netlist = Netlist::parse_spice(
//!     "* volume pot
//!      V1 in 0 0
//!      RA in out 90k
//!      RB out 0 10k
//!      .control
//!      wrdata out v(out)
//!      .endc",
//! )
//! .unwrap();
//! // RB grows as the pot turns up: it is the first leg.
//! netlist.add_variable(Variable::pot("volume", ["rb", "ra"], Taper::Linear)).unwrap();
//! assert_eq!(netlist.variables().len(), 1);
//! ```

use super::{ElementKind, Netlist, NetlistError};
use crate::circuit::devices::potentiometer::{Bounce, LinearCubic, Tracking, Wiper};
use crate::units::Seconds;

/// How a pot's resistance follows its rotation: the fraction of the
/// track between the first leg's ends at `position` (0 to 1).
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Taper {
    /// The fraction is the position (a "B" pot).
    Linear,
    /// `(b^x − 1)/(b − 1)` with `b = ((1 − m)/m)²`, so the fraction at
    /// half rotation is `midpoint`: 0.1 is the usual audio ("A") taper,
    /// 0.9 the reverse ("C"); 0.5 is linear. An idealised law: real
    /// tracks are [`Curve`](Self::Curve)s.
    Exponential { midpoint: f64 },
    /// A manufacturer's law (H6): [`LinearCubic::ALPHA_15A`] and its
    /// family, or its reverse.
    Curve(LinearCubic),
    /// An S-shaped law symmetric about half rotation (50 % there): each
    /// half the [`Exponential`](Self::Exponential) law of `midpoint`
    /// squeezed into it, `½·A(2x)` below and `1 − ½·A(2 − 2x)` above. A
    /// `midpoint` below ½ turns slowly at both ends (the shape of Alpha's
    /// B family, 4B being JIS W, and of balance and crossfade pots);
    /// above ½, fast at the ends and slow through the middle.
    Symmetric { midpoint: f64 },
}

impl Taper {
    /// The audio ("A", logarithmic) taper: 10 % at half rotation.
    pub const AUDIO: Self = Self::Exponential { midpoint: 0.1 };
    /// The reverse audio ("C") taper: 90 % at half rotation.
    pub const REVERSE_AUDIO: Self = Self::Exponential { midpoint: 0.9 };

    /// The law turned end for end, `1 − f(1 − x)`: an A taper's C.
    pub fn reversed(self) -> Self {
        match self {
            Self::Linear => Self::Linear,
            Self::Exponential { midpoint } => Self::Exponential { midpoint: 1.0 - midpoint },
            Self::Curve(law) => Self::Curve(law.reversed()),
            Self::Symmetric { midpoint } => Self::Symmetric { midpoint: 1.0 - midpoint },
        }
    }

    fn base(midpoint: f64) -> Option<f64> {
        let b = ((1.0 - midpoint) / midpoint).powi(2);
        ((b - 1.0).abs() > 1e-12 && b.is_finite()).then_some(b)
    }

    /// The track fraction at `position` (clamped to 0–1).
    pub fn fraction(self, position: f64) -> f64 {
        let x = position.clamp(0.0, 1.0);
        match self {
            Self::Linear => x,
            Self::Exponential { midpoint } => match Self::base(midpoint) {
                Some(b) => (b.powf(x) - 1.0) / (b - 1.0),
                None => x,
            },
            Self::Curve(law) => law.fraction(x),
            Self::Symmetric { midpoint } => {
                let half = Self::Exponential { midpoint };
                if x <= 0.5 {
                    0.5 * half.fraction(2.0 * x)
                } else {
                    1.0 - 0.5 * half.fraction(2.0 - 2.0 * x)
                }
            }
        }
    }

    /// The position whose fraction is `fraction` (clamped to 0–1): the
    /// inverse of [`fraction`](Self::fraction).
    pub fn position(self, fraction: f64) -> f64 {
        let f = fraction.clamp(0.0, 1.0);
        match self {
            Self::Linear => f,
            Self::Exponential { midpoint } => match Self::base(midpoint) {
                Some(b) => (1.0 + f * (b - 1.0)).ln() / b.ln(),
                None => f,
            },
            Self::Curve(law) => law.rotation(f),
            Self::Symmetric { midpoint } => {
                let half = Self::Exponential { midpoint };
                if f <= 0.5 {
                    0.5 * half.position(2.0 * f)
                } else {
                    1.0 - 0.5 * half.position(2.0 - 2.0 * f)
                }
            }
        }
    }
}

/// A netlist element (or a pair) declared variable. Names are the
/// netlist's element names (any case).
#[derive(Clone, Debug, PartialEq)]
pub enum Variable {
    /// A potentiometer: `legs[0]` runs from one end to the wiper and
    /// grows as the pot turns up (`end_ohms + track · taper(position)`),
    /// `legs[1]` is the rest of the track (`end_ohms + track · (1 −
    /// taper(position))`). The track is the legs' sum less both end
    /// resistances, read from the netlist.
    Pot {
        name: String,
        legs: [String; 2],
        taper: Taper,
        /// The resistance each leg keeps at its end stop (the wiper's
        /// contact and the track's termination). A wave digital filter
        /// needs it positive: a port's resistance must never reach zero.
        end_ohms: f64,
        /// This gang's deviation from the law (H6; [`Tracking::EXACT`]
        /// for a single pot or the reference gang).
        tracking: Tracking,
        /// A scratchy wiper, or none.
        wiper: Option<Wiper>,
    },
    /// A pot wired as a variable resistor (its wiper tied to one end):
    /// `end_ohms + track_ohms · taper(position)`.
    Rheostat {
        element: String,
        track_ohms: f64,
        taper: Taper,
        end_ohms: f64,
        tracking: Tracking,
        wiper: Option<Wiper>,
    },
    /// A resistor set in ohms.
    Resistor { element: String },
    /// A capacitor set in farads.
    Capacitor { element: String },
    /// A switch (H6): a resistor at `on_ohms` closed and `off_ohms`
    /// open, thrown at once — no glide — or moved in `ramp_seconds` from
    /// one to the other along a straight line in log-resistance (a soft
    /// switch's click suppression), its contacts bouncing on closing if
    /// `bounce` says so.
    Switch {
        element: String,
        on_ohms: f64,
        off_ohms: f64,
        ramp_seconds: f64,
        bounce: Option<Bounce>,
    },
}

impl Variable {
    /// A pot with no end resistance (a DK circuit takes it; a wave
    /// digital one wants [`with_end_ohms`](Self::with_end_ohms)).
    pub fn pot(name: &str, legs: [&str; 2], taper: Taper) -> Self {
        Self::Pot {
            name: name.to_ascii_lowercase(),
            legs: legs.map(str::to_ascii_lowercase),
            taper,
            end_ohms: 0.0,
            tracking: Tracking::EXACT,
            wiper: None,
        }
    }

    /// A pot of `track_ohms` wired as a rheostat, no end resistance.
    pub fn rheostat(element: &str, track_ohms: f64, taper: Taper) -> Self {
        Self::Rheostat {
            element: element.to_ascii_lowercase(),
            track_ohms,
            taper,
            end_ohms: 0.0,
            tracking: Tracking::EXACT,
            wiper: None,
        }
    }

    /// A switch on the resistor `element`: `on_ohms` closed, `off_ohms`
    /// open, thrown at once, no bounce. The deck writes the element at
    /// either value; the nearer (in log-resistance) is its position.
    pub fn switch(element: &str, on_ohms: f64, off_ohms: f64) -> Self {
        Self::Switch {
            element: element.to_ascii_lowercase(),
            on_ohms,
            off_ohms,
            ramp_seconds: 0.0,
            bounce: None,
        }
    }

    /// The same switch moving over `seconds` (H6's click suppression).
    pub fn with_ramp(mut self, ramp: Seconds) -> Self {
        let seconds = ramp.get();
        if let Self::Switch { ramp_seconds, .. } = &mut self {
            *ramp_seconds = seconds.max(0.0);
        }
        self
    }

    /// The same switch bouncing as it closes.
    pub fn with_bounce(mut self, chatter: Bounce) -> Self {
        if let Self::Switch { bounce, .. } = &mut self {
            *bounce = Some(chatter);
        }
        self
    }

    /// The same pot or rheostat as a second gang that strays by
    /// `gang` (H6).
    pub fn with_tracking(mut self, gang: Tracking) -> Self {
        if let Self::Pot { tracking, .. } | Self::Rheostat { tracking, .. } = &mut self {
            *tracking = gang;
        }
        self
    }

    /// The same pot or rheostat with a scratchy wiper (H6).
    pub fn with_wiper(mut self, scratch: Wiper) -> Self {
        if let Self::Pot { wiper, .. } | Self::Rheostat { wiper, .. } = &mut self {
            *wiper = Some(scratch);
        }
        self
    }

    pub fn resistor(element: &str) -> Self {
        Self::Resistor {
            element: element.to_ascii_lowercase(),
        }
    }

    pub fn capacitor(element: &str) -> Self {
        Self::Capacitor {
            element: element.to_ascii_lowercase(),
        }
    }

    /// The same pot or rheostat with `ohms` of end resistance per leg.
    pub fn with_end_ohms(mut self, ohms: f64) -> Self {
        if let Self::Pot { end_ohms, .. } | Self::Rheostat { end_ohms, .. } = &mut self {
            *end_ohms = ohms;
        }
        self
    }

    /// The name a circuit finds it by: a pot's own name, else its
    /// element's.
    pub fn name(&self) -> &str {
        match self {
            Self::Pot { name, .. } => name,
            Self::Rheostat { element, .. }
            | Self::Resistor { element }
            | Self::Capacitor { element }
            | Self::Switch { element, .. } => element,
        }
    }

    /// The elements it varies.
    pub fn elements(&self) -> Vec<&str> {
        match self {
            Self::Pot { legs, .. } => legs.iter().map(String::as_str).collect(),
            Self::Rheostat { element, .. }
            | Self::Resistor { element }
            | Self::Capacitor { element }
            | Self::Switch { element, .. } => vec![element.as_str()],
        }
    }
}

fn refuse(element: &str, reason: &str) -> NetlistError {
    NetlistError::Unsupported {
        element: element.to_string(),
        reason: reason.to_string(),
    }
}

impl Netlist {
    /// Declare elements variable. Each named element must exist, be a
    /// resistor (a capacitor for [`Variable::Capacitor`]) and belong to
    /// no other variable; names are unique.
    pub fn add_variable(&mut self, variable: Variable) -> Result<(), NetlistError> {
        let name = variable.name();
        if self.variables.iter().any(|v| v.name() == name) {
            return Err(NetlistError::Duplicate { name: name.to_string() });
        }
        let capacitor = matches!(variable, Variable::Capacitor { .. });
        let elements = variable.elements();
        if elements.len() == 2 && elements[0] == elements[1] {
            return Err(refuse(name, "a pot's two legs are two resistors"));
        }
        for element in &elements {
            let kind = &self
                .element(element)
                .ok_or_else(|| NetlistError::UnknownElement {
                    name: element.to_string(),
                })?
                .kind;
            match (capacitor, kind) {
                (false, ElementKind::Resistor { .. }) | (true, ElementKind::Capacitor { .. }) => {}
                (false, _) => return Err(refuse(element, "a pot, rheostat or variable resistor is a resistor")),
                (true, _) => return Err(refuse(element, "a variable capacitor is a capacitor")),
            }
            if self.variables.iter().any(|v| v.elements().contains(element)) {
                return Err(refuse(element, "already part of another variable"));
            }
        }
        if let Variable::Pot { end_ohms, .. } | Variable::Rheostat { end_ohms, .. } = &variable {
            if !(end_ohms.is_finite() && *end_ohms >= 0.0) {
                return Err(refuse(name, "an end resistance is finite and not negative"));
            }
        }
        if let Variable::Rheostat { track_ohms, .. } = &variable {
            if !(track_ohms.is_finite() && *track_ohms > 0.0) {
                return Err(refuse(name, "a rheostat's track is positive and finite"));
            }
        }
        if let Variable::Switch { on_ohms, off_ohms, .. } = &variable {
            if !(on_ohms.is_finite() && off_ohms.is_finite() && *on_ohms > 0.0 && off_ohms > on_ohms) {
                return Err(refuse(name, "a switch is a positive resistance, open above closed"));
            }
        }
        self.variables.push(variable);
        Ok(())
    }

    /// The declared variables, in order.
    pub fn variables(&self) -> &[Variable] {
        &self.variables
    }
}
