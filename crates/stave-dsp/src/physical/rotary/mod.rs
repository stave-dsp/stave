//! Rotary speakers: a horn and a drum spinning in a cabinet, heard by
//! two microphones.
//!
//! A rotating source moves toward and away from a listener, so what it
//! radiates arrives Doppler-shifted, louder and brighter as its mouth
//! faces the listener, and through the cabinet's reflections as a
//! chorus of differently shifted copies (Smith, Serafin, Abel &
//! Berners, "Doppler Simulation and the Leslie", DAFx-02). A
//! [`Radiator`] is one rotor: a source on a circle with a directivity,
//! its motor's inertia ramping the speed between chorale and tremolo,
//! each microphone reading it through a delay that is the propagation
//! time from where the source *was* when it emitted. A [`Cabinet`]
//! splits the signal at a crossover into a horn and a drum.
//!
//! **What is published and what is assumed.** Rotor speeds and ramp
//! times are the Hammond 122H/142H user guide's defaults (horn 44 and
//! 402 rpm, rising in 1.8 s and falling in 2.4 s; drum 42 and 372 rpm,
//! 7 s and 5.5 s), and the 800 Hz crossover is the guide's. The horn and
//! drum radii, the directivity patterns and the cabinet's reflections
//! are not in the sources at hand (Herrera, Hanson & Abel's measured
//! horn and drum responses, AES 2009, are paywalled): they are stated
//! voicing assumptions, each a field to change.

mod cabinet;
mod radiator;

pub use cabinet::Cabinet;
pub use radiator::Radiator;

use crate::units::{Hz, Seconds};

/// The speed of sound in air at 20 °C, m/s.
pub const SPEED_OF_SOUND: f64 = 343.0;

/// What "reaching" a speed means for a ramp time: covering this
/// fraction of the change. A rotor's speed approaches its target as a
/// first-order lag (a motor's torque against the rotor's inertia and its
/// drag), so it never arrives exactly; the published rise and fall times
/// are read as the time to 95 %, three time constants (`ln 20`).
pub const REACH: f64 = 0.95;

/// The most reflecting walls a [`Radiator`] takes.
pub const MAX_WALLS: usize = 4;

/// A rotor: its two speeds, how long its motor takes to change between
/// them, and the radius its sound leaves from.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Rotor {
    /// Chorale (slow) speed, revolutions a second.
    pub slow: Hz,
    /// Tremolo (fast) speed, revolutions a second.
    pub fast: Hz,
    /// Time to reach a faster speed ([`REACH`]).
    pub rise: Seconds,
    /// Time to reach a slower speed, or stop.
    pub fall: Seconds,
    /// Radius of the point the sound leaves from (the horn's mouth, the
    /// drum's opening), m.
    pub radius: f64,
    /// Spin direction seen from above: true for clockwise.
    pub clockwise: bool,
}

impl Rotor {
    /// The Hammond 122H's horn rotor at its factory settings (user
    /// guide, rotor control panel: 44 rpm, 402 rpm, rise 1.8 s, fall
    /// 2.4 s). The 15 cm mouth radius is an assumption.
    pub const HORN_122H: Self = Self {
        slow: Hz::new(44.0 / 60.0),
        fast: Hz::new(402.0 / 60.0),
        rise: Seconds::new(1.8),
        fall: Seconds::new(2.4),
        radius: 0.15,
        clockwise: false,
    };
    /// The Hammond 122H's low (drum) rotor at its factory settings (42
    /// rpm, 372 rpm, rise 7 s, fall 5.5 s), spinning against the horn.
    /// The 20 cm opening radius is an assumption.
    pub const DRUM_122H: Self = Self {
        slow: Hz::new(42.0 / 60.0),
        fast: Hz::new(372.0 / 60.0),
        rise: Seconds::new(7.0),
        fall: Seconds::new(5.5),
        radius: 0.2,
        clockwise: true,
    };

    /// The first-order time constant behind a ramp time ([`REACH`]).
    pub fn time_constant(ramp: Seconds) -> f64 {
        ramp.get() / -(1.0 - REACH).ln()
    }
}

/// The rotor speed switch.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Speed {
    /// Braking to a stop (at the fall rate).
    Stop,
    /// The slow speed.
    #[default]
    Chorale,
    /// The fast speed.
    Tremolo,
}

/// One frequency range's radiation pattern: `floor + (1 − floor) ·
/// ((1 + cos φ)/2)^order`, `φ` the angle between the source's axis and
/// the direction to the listener. Order 0 is omnidirectional, 1 a
/// cardioid above the floor; higher orders narrow the beam.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Lobe {
    pub order: f32,
    /// The level straight behind, linear.
    pub floor: f32,
}

impl Lobe {
    /// The pattern's gain at `cos φ`.
    #[inline]
    pub fn gain(&self, cos_phi: f64) -> f64 {
        let f = f64::from(self.floor);
        let c = (0.5 * (1.0 + cos_phi)).clamp(0.0, 1.0);
        f + (1.0 - f) * c.powf(f64::from(self.order))
    }
}

/// A source's frequency-dependent directivity: a `low` lobe below a
/// one-pole split at `corner` and a `high` lobe above (the split's two
/// halves sum to the input, so a source whose lobes agree is that lobe
/// at every frequency).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Directivity {
    pub corner: Hz,
    pub low: Lobe,
    pub high: Lobe,
}

impl Directivity {
    /// Radiates equally every way (Smith et al.'s omnidirectional
    /// horn, their diffuser's aim).
    pub const OMNI: Self = Self {
        corner: Hz::new(1_000.0),
        low: Lobe { order: 0.0, floor: 1.0 },
        high: Lobe { order: 0.0, floor: 1.0 },
    };
    /// A horn: a cardioid 10 dB down behind below 1.5 kHz, a beam of
    /// order 3 20 dB down behind above (voicing: Smith et al.'s Fig. 7
    /// shows the treble's level swinging most with angle, but its values
    /// are not tabulated).
    pub const HORN_122H: Self = Self {
        corner: Hz::new(1_500.0),
        low: Lobe { order: 1.0, floor: 0.3 },
        high: Lobe { order: 3.0, floor: 0.1 },
    };
    /// A drum's opening: wide, 6–10 dB of swing (voicing: Pekonen et
    /// al., DAFx-11, find the bass mostly amplitude-modulated).
    pub const DRUM_122H: Self = Self {
        corner: Hz::new(300.0),
        low: Lobe { order: 0.5, floor: 0.5 },
        high: Lobe { order: 1.0, floor: 0.3 },
    };

    /// The one-pole split's coefficient `a = e^(−2π corner/fs)`.
    pub(crate) fn pole(&self, sample_rate: f64) -> f64 {
        (-core::f64::consts::TAU * f64::from(self.corner.get()) / sample_rate).exp()
    }

    /// The pattern's magnitude at `cos φ` for a tone at `hz`, as the
    /// discrete split realises it at `sample_rate`:
    /// `|g_low(φ) H(ω) + g_high(φ) (1 − H(ω))|`, `H = (1 − a)/(1 − a e^(−jω))`.
    pub fn response(&self, cos_phi: f64, hz: f64, sample_rate: f64) -> f64 {
        let a = self.pole(sample_rate);
        let w = core::f64::consts::TAU * hz / sample_rate;
        let (s, c) = w.sin_cos();
        // H = (1 − a) / (1 − a cos w + j a sin w).
        let (dr, di) = (1.0 - a * c, a * s);
        let d2 = dr * dr + di * di;
        let (hr, hi) = ((1.0 - a) * dr / d2, -(1.0 - a) * di / d2);
        let (gl, gh) = (self.low.gain(cos_phi), self.high.gain(cos_phi));
        let re = gl * hr + gh * (1.0 - hr);
        let im = gl * hi - gh * hi;
        (re * re + im * im).sqrt()
    }
}

/// A microphone: its bearing around the rotors' axis (degrees,
/// counter-clockwise from the cabinet's front) and its distance from the
/// axis (m), in the rotors' plane.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Mic {
    pub angle: f64,
    pub distance: f64,
}

impl Mic {
    pub const fn at(angle: f64, distance: f64) -> Self {
        Self { angle, distance }
    }

    /// Its position, m.
    pub fn position(&self) -> (f64, f64) {
        let (s, c) = self.angle.to_radians().sin_cos();
        (self.distance * c, self.distance * s)
    }
}

/// A reflecting wall behind a rotor: the plane whose normal points at
/// `angle` (degrees) at `distance` from the axis (m), reflecting
/// `reflection` of what reaches it. Each is an image source (the rotor
/// mirrored in it), heard with its own delay and Doppler shift. A wall
/// between a rotor and a microphone is not modelled (louvres are open).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Wall {
    pub angle: f64,
    pub distance: f64,
    pub reflection: f32,
}

impl Wall {
    /// The 122's closed back, 27 cm behind the rotors, reflecting half
    /// (an assumption).
    pub const BACK_122H: Self = Self { angle: 180.0, distance: 0.27, reflection: 0.5 };
}

#[cfg(test)]
mod tests;
