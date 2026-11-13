//! A string — plucked, struck or bowed — as a digital waveguide.
//!
//! A string's transverse waves travel both ways at the wave speed and
//! reflect, inverted, at the nut and the bridge; Smith's digital
//! waveguide (1992) runs exactly that as delay lines, with the losses
//! and the stiffness spread along the real string lumped into filters
//! at one end. The contact point — where a plectrum, a hammer or a bow
//! acts — splits the string in two, so the excitation enters both ways
//! at once and the position is heard as a comb on the partials.
//!
//! **From the plugins.** Aeolian's strings were Karplus–Strong loops
//! (promoted by S7 as `primitives::resonators::KarplusStrong`) bowed by
//! low-passed noise standing in for stick–slip; here the bow is the
//! friction curve itself, and a pluck or strike has a position. Its
//! doubled courses, vibrato drift and in-loop saturator are voicing and
//! stay in the plugin, as do Smear's semitone and octave couplings
//! between resonators (its lattice is S7's modal `Bank`).

mod bow;
mod dispersion;

pub use bow::{Bow, Friction};
pub use dispersion::{Dispersion, MAX_SECTIONS};

use dispersion::{one_pole_group, one_pole_lag};

use crate::kernel::denormal;
use crate::kernel::node::{Stage, Tail};
use crate::primitives::time::delay::DelayLine;
use crate::primitives::time::interpolators::{Interpolator, Lagrange3};
use crate::units::{Hz, Seconds};

/// The loop as designed: both lines' lengths (samples, including the
/// interpolators' fractions), the loss filter `g(1 − p)/(1 − p z⁻¹)`
/// and the dispersion filter.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Tuning {
    /// Contact → nut → contact.
    pub nut: f32,
    /// Contact → bridge → contact.
    pub bridge: f32,
    /// The loss filter's pole `p`.
    pub pole: f64,
    /// The loss filter's gain `g`.
    pub gain: f64,
    pub dispersion: Dispersion,
}

/// What an excitation does at the contact point.
#[derive(Copy, Clone, Debug, PartialEq)]
enum Shape {
    /// A raised-cosine velocity pulse (a hammer's contact).
    Strike,
    /// A raised-cosine ramp to a held velocity (a plectrum's release:
    /// a step in force), which then leaks away slowly.
    Pluck,
}

#[derive(Copy, Clone, Debug)]
struct Excitation {
    shape: Shape,
    length: u32,
    at: u32,
    level: f32,
    from: f32,
    held: f32,
}

impl Excitation {
    const IDLE: Self = Self { shape: Shape::Strike, length: 0, at: 0, level: 0.0, from: 0.0, held: 0.0 };

    #[inline]
    fn next(&mut self, leak: f32) -> f32 {
        let running = self.at < self.length;
        let phase = (self.at + 1) as f32 / self.length.max(1) as f32;
        match self.shape {
            Shape::Strike => {
                self.held *= leak;
                if !running {
                    return self.held;
                }
                self.at += 1;
                // A Hann pulse over `length` samples (an impulse at 1).
                let p = (self.at as f32) / (self.length + 1) as f32;
                self.held + self.level * 0.5 * (1.0 - (core::f32::consts::TAU * p).cos())
            }
            Shape::Pluck => {
                if running {
                    self.at += 1;
                    self.held = self.from + self.level * 0.5 * (1.0 - (core::f32::consts::PI * phase).cos());
                } else {
                    self.held *= leak;
                }
                self.held
            }
        }
    }
}

/// A string as a digital waveguide: plucked, struck or bowed at a
/// contact point, with stiffness and frequency-dependent loss.
///
/// # Models
/// The transverse waves of a string fixed at a rigid nut and bridge
/// (Smith 1992; Smith, *Physical Audio Signal Processing*, "Digital
/// Waveguide Models"): velocity waves in two folded delay lines, one
/// each side of the contact point at `β` of the length from the bridge,
/// each a round trip with its −1 reflection. The nut's reflection
/// carries the whole string's losses (a one-pole lowpass, Jaffe & Smith
/// 1983) and its stiffness (a cascade of identical first-order allpass
/// sections, Van Duyne & Smith 1994 as Rauhala & Välimäki (DAFx-06)
/// design them), so partial `k` rings at `f_k = k f₀ √(1 + B k²)`
/// (Fletcher's stiff string). At the contact a velocity enters both
/// ways at once: [`strike`](Self::strike) (a hammer's pulse), [`pluck`](Self::pluck)
/// (a step in force), the input (any exciter), or a [`Bow`] whose
/// friction force is solved against the incoming waves each sample on
/// a hyperbolic friction curve (McIntyre, Schumacher & Woodhouse 1983;
/// Smith's bowed-string waveguide), which settles into Helmholtz motion.
/// The output is the wave the bridge reflects (the bridge's force,
/// read where it returns to the contact point), m/s.
///
/// # Derivation
/// With `v_h` the sum of the waves arriving at the contact and `Δv` the
/// velocity the contact adds, the waves leaving it are each arrival plus
/// `Δv`: a struck or plucked string's `Δv` is the excitation, a bowed
/// one's comes from [`Friction`] (a quadratic in the slip speed, no
/// iteration). The loop's design, at the heard fundamental `f₁`
/// (`f₀ = f₁/√(1 + B)`): the dispersion coefficient by a least-squares
/// fit in cents of partials 2…K to `f_k` with partial 1 exact
/// ([`Dispersion::fit`], from Rauhala & Välimäki's closed form as the
/// published reference); the loss filter so the fundamental and the
/// 8th partial decay in their T60s, each through the loop's group delay
/// at its frequency (the round trip time) — the S7 string's two-point
/// fit, generalised to a dispersive loop; the bridge line so the contact
/// sits at `β` of the fundamental's period from the bridge, the nut line
/// the rest, each read's interpolator phase taken off its length
/// (`Interpolator::response`).
///
/// # Verified
/// `crates/stave-dsp/src/physical/string/tests.rs::partials_follow_the_stiff_string_formula`
/// (measured partials to the 16th, below 2.5 kHz, against
/// `k f₀ √(1 + B k²)`: 65.4 Hz at B = 1e-4 with 8 sections within 1.5
/// cents, 110 Hz at 2e-4 with 16 within 2, 261.6 Hz at 4e-4 with 8
/// within 1.2, 110 Hz at 4e-4 with 16 within 5; measured 0.94, 1.44,
/// 0.72, 3.72, where a flexible string's 16th is 22–84 cents off),
/// `crates/stave-dsp/src/physical/string/tests.rs::partials_decay_as_the_loss_filter_predicts`
/// (each of the first 16 partials' T60 within 2 % of the designed
/// loop's `3 τ_g / −log₁₀|H|`, flexible and stiff, measured 0.15 %;
/// the fundamental and the 8th within 2 % of their settings),
/// `crates/stave-dsp/src/physical/string/tests.rs::pluck_position_combs_the_partials`
/// (struck at β = 1/5: the other partials within 0.2 dB of `|sin π k β|`,
/// measured 0.016, the 5th, 10th, 15th and 20th at least 40 dB down,
/// measured 49.9; a pluck the strike's spectrum over `k` within 0.2 dB),
/// `crates/stave-dsp/src/physical/string/tests.rs::bowed_string_settles_into_helmholtz_motion`
/// (one slip a period, the period within 5 cents, measured 1.0; the
/// slip fraction within 0.02 of β, measured 0.1125, 0.1507, 0.0898 at
/// β = 0.1, 0.15, 0.08; the mean slip velocity within 15 % of
/// `−v_b(1 − β)/β`, measured 12.4 %: the loss rounds the corner and the
/// slip lasts a little longer than β),
/// `crates/stave-dsp/src/physical/string/tests.rs::rauhala_closed_form_is_the_fits_start`
/// (C2, B = 1e-4, eight sections, 44.1 kHz, to 1.5 kHz: the closed form
/// within the paper's ±8.39 cents, measured 4.52; the fit 3.28, rms
/// 1.38 against 3.12) and
/// `crates/stave-dsp/src/physical/string/tests.rs::tails_cover_the_ring`.
///
/// # Cost
/// `"String (plucked, Lagrange3)"`, `"String (stiff, 8 sections)"` and
/// `"String (bowed)"` on the datasheet. Setters that retune
/// (`set_pitch`, `set_inharmonicity`, `set_decay`, `set_position`) refit
/// the loop: block rate, tens of microseconds with dispersion.
///
/// # Aliasing
/// None plucked or struck (linear). Bowed, the stick–slip corner of
/// Helmholtz motion is a sawtooth: its partials above Nyquist fold
/// back, attenuated by the loss filter (oversample a bowed string that
/// must stay clean at high pitch and low loss).
///
/// # Valid ranges
/// Pitch from the `lowest` given at construction to about 0.2·fs.
/// Dispersion is fitted on the partials to the 16th below 2.5 kHz; a
/// cascade of identical first-order sections cannot follow `B k²` much
/// further (27–47 cents off by 6 kHz in the test's cases: a wide-band
/// design wants Abel, Välimäki & Smith's biquads), and above
/// `B ≈ 4e-4` at low pitch even the fitted band runs several cents off.
/// More dispersion than the nut line can absorb is fitted short
/// ([`tuning`](Self::tuning) says what was achieved). Contact position
/// `β` in `(0, 0.5]` (beyond is mirrored), at least two samples of
/// line from the bridge; treble T60 ≤ fundamental T60. A bow plays
/// Helmholtz motion between Schelleng's force limits, about
/// `2Z v_b/(β(μ_s − μ_d))` above.
///
/// ```
/// use stave_dsp::physical::string::Waveguide;
/// use stave_dsp::units::{hz, seconds};
/// use stave_dsp::Stage;
///
/// let mut string: Waveguide = Waveguide::new(hz(40.0), hz(110.0));
/// string.set_decay(seconds(4.0), seconds(0.8));
/// string.set_inharmonicity(1e-4);
/// string.set_position(0.15);
/// string.prepare(48_000.0);
/// string.pluck(0.5, seconds(0.001));
/// let ring: f32 = (0..48_000).map(|_| string.tick(0.0).abs()).sum();
/// assert!(ring > 0.0);
/// ```
#[derive(Clone, Debug)]
pub struct Waveguide<I: Interpolator = Lagrange3> {
    nut: DelayLine<I>,
    bridge: DelayLine<I>,
    lowest: f32,
    pitch: f64,
    inharmonicity: f64,
    sections: usize,
    decay: [f64; 2],
    position: f64,
    sample_rate: f32,
    tuning: Tuning,
    /// f32 copies of the loop filter for the audio path.
    loss: [f32; 2],
    allpass: f32,
    loss_state: f32,
    allpass_state: [f32; MAX_SECTIONS],
    excitation: Excitation,
    leak: f32,
    bow: Option<Bow>,
    friction: Friction,
    impedance: f64,
    sticking: bool,
    contact: f32,
}

/// The treble T60 is set at this partial.
const TREBLE_PARTIAL: f64 = 8.0;
/// Dispersion is fitted on partials up to this many, and this high.
const FIT_PARTIALS: usize = 16;
const FIT_TOP_HZ: f64 = 2_500.0;

impl<I: Interpolator> Waveguide<I> {
    /// A string that can tune down to `lowest`, at `pitch`: flexible
    /// (`B = 0`), contacted at 0.13 of its length from the bridge, T60
    /// 3 s at the fundamental and 0.6 s at the 8th partial, eight
    /// dispersion sections ready, the wave impedance of a violin's D
    /// string (0.2 kg/s) and rosin friction. The lines are allocated in
    /// `prepare`.
    pub fn new(lowest: Hz, pitch: Hz) -> Self {
        let mut s = Self {
            nut: DelayLine::default(),
            bridge: DelayLine::default(),
            lowest: lowest.get(),
            pitch: f64::from(pitch.get()),
            inharmonicity: 0.0,
            sections: 8,
            decay: [3.0, 0.6],
            position: 0.13,
            sample_rate: 48_000.0,
            tuning: Tuning::default(),
            loss: [0.0; 2],
            allpass: 0.0,
            loss_state: 0.0,
            allpass_state: [0.0; MAX_SECTIONS],
            excitation: Excitation::IDLE,
            leak: 1.0,
            bow: None,
            friction: Friction::ROSIN,
            impedance: 0.2,
            sticking: true,
            contact: 0.0,
        };
        s.update();
        s
    }

    /// The fundamental heard (partial 1), block rate.
    pub fn set_pitch(&mut self, pitch: Hz) {
        self.pitch = f64::from(pitch.get());
        self.update();
    }

    /// The inharmonicity coefficient `B` (`π³ E d⁴ / 64 l² T` for a
    /// solid string of diameter `d`, length `l`, tension `T`; a piano's
    /// 1e-4 to 1e-2), block rate.
    pub fn set_inharmonicity(&mut self, b: f64) {
        self.inharmonicity = b.max(0.0);
        self.update();
    }

    /// How many allpass sections model the stiffness (≤ [`MAX_SECTIONS`];
    /// more fit higher partials, each costs a few ns), block rate.
    pub fn set_dispersion_sections(&mut self, sections: usize) {
        self.sections = sections.min(MAX_SECTIONS);
        self.update();
    }

    /// The fundamental's T60 and the 8th partial's.
    pub fn set_decay(&mut self, fundamental: Seconds, treble: Seconds) {
        self.decay = [fundamental.get(), treble.get()];
        self.update();
    }

    /// The contact point, as a fraction of the length from the bridge
    /// (block rate: the lines' split moves).
    pub fn set_position(&mut self, beta: f64) {
        let b = beta.rem_euclid(1.0);
        self.position = if b > 0.5 { 1.0 - b } else { b };
        self.update();
    }

    /// The string's wave impedance `Z = √(T μ)`, kg/s (what a bow's
    /// force is measured against).
    pub fn set_impedance(&mut self, z: f64) {
        self.impedance = z.max(1e-6);
    }

    /// The friction curve the bow plays on.
    pub fn set_friction(&mut self, friction: Friction) {
        self.friction = friction;
    }

    /// Put the bow on the string at the contact point, or take it off
    /// (`None`); realtime.
    pub fn set_bow(&mut self, bow: Option<Bow>) {
        self.bow = bow;
    }

    /// Strike at the contact: a raised-cosine velocity pulse peaking at
    /// `velocity` m/s, `width` long (a hammer's contact time; 0 is one
    /// sample).
    pub fn strike(&mut self, velocity: f32, width: Seconds) {
        let length = (width.get() * f64::from(self.sample_rate)).round().max(1.0) as u32;
        self.excitation = Excitation { shape: Shape::Strike, length, at: 0, level: velocity, from: 0.0, held: self.excitation.held };
    }

    /// Pluck at the contact: the plectrum lets go over `width` (a
    /// raised-cosine step of `velocity` m/s in the contact's velocity,
    /// which leaks away over twenty periods).
    pub fn pluck(&mut self, velocity: f32, width: Seconds) {
        let length = (width.get() * f64::from(self.sample_rate)).round().max(1.0) as u32;
        let held = self.excitation.held;
        self.excitation = Excitation { shape: Shape::Pluck, length, at: 0, level: velocity, from: held, held };
    }

    /// The loop as designed at the prepared rate.
    pub fn tuning(&self) -> Tuning {
        self.tuning
    }

    /// The contact point's velocity this sample, m/s.
    pub fn contact_velocity(&self) -> f32 {
        self.contact
    }

    /// Whether the bow held the string this sample.
    pub fn sticking(&self) -> bool {
        self.bow.is_some() && self.sticking
    }

    /// Design the loop (no allocation).
    fn update(&mut self) {
        let fs = f64::from(self.sample_rate);
        let f1 = self.pitch.clamp(f64::from(self.lowest), 0.45 * fs);
        let b = self.inharmonicity;
        let f0 = f1 / (1.0 + b).sqrt();
        let w0 = core::f64::consts::TAU * f0 / fs;
        let wk = |k: f64| k * w0 * (1.0 + b * k * k).sqrt();
        let w1 = wk(1.0);
        let period = fs / f1;
        let read = self.nut.interpolator();
        let polar = |frac: f64, w: f64| {
            let (re, im) = read.response(frac as f32, w);
            ((re * re + im * im).sqrt(), -im.atan2(re) / w)
        };
        let bridge_len = self.position * period;
        let least = I::NEWER as f64 + 1.0;
        let bridge_len = bridge_len.max(least);
        let shortest = least + bridge_len;
        let partials = (1..=FIT_PARTIALS).take_while(|&k| wk(k as f64) / core::f64::consts::TAU * fs <= FIT_TOP_HZ.min(0.4 * fs)).count();
        // The treble partial: the 8th, or the highest below 0.45·fs.
        let mut kt = TREBLE_PARTIAL;
        while kt > 1.5 && wk(kt) > 0.9 * core::f64::consts::PI {
            kt -= 1.0;
        }
        let wt = wk(kt);
        let (t1, tt) = (self.decay[0], self.decay[1].min(self.decay[0]));
        let mut pole = 0.0f64;
        let (mut d_nut, mut d_bridge) = (period - bridge_len, bridge_len);
        // Lines: the bridge's read at β of the period, the nut's the rest
        // less the nut filters' phase delay, each read's interpolator
        // phase taken off (fixed point: the fraction moves the phase).
        let lines = |d_nut: &mut f64, d_bridge: &mut f64, filters: f64| {
            for _ in 0..4 {
                let (_, tb) = polar(d_bridge.fract(), w1);
                *d_bridge = (*d_bridge + bridge_len - (d_bridge.floor() + tb)).max(least);
                let (_, tn) = polar(d_nut.fract(), w1);
                *d_nut = (*d_nut + period - bridge_len - filters - (d_nut.floor() + tn)).max(least);
            }
        };
        for _ in 0..3 {
            let disp = Dispersion::fit(w0, b, self.sections, partials, shortest, |w| one_pole_lag(pole, w), |w| one_pole_group(pole, w));
            lines(&mut d_nut, &mut d_bridge, (one_pole_lag(pole, w1) + disp.lag(w1)) / w1);
            // Loss: the T60s through the loop's group delay at each.
            let line = d_nut.floor() + d_bridge.floor() + polar(d_nut.fract(), w1).1 + polar(d_bridge.fract(), w1).1;
            let group = |w: f64| line + one_pole_group(pole, w) + disp.group(w);
            let target = |w: f64, t60: f64| {
                let m = polar(d_nut.fract(), w).0 * polar(d_bridge.fract(), w).0;
                if t60 <= 0.0 {
                    0.0
                } else {
                    10f64.powf(-3.0 * group(w) / (t60 * fs)) / m
                }
            };
            let g1 = target(w1, t1);
            let mut p = 0.0;
            if wt > 1.5 * w1 && g1 > 0.0 {
                let rho = target(wt, tt) / g1;
                if rho < 1.0 {
                    let r2 = rho * rho;
                    let beta = (w1.cos() - r2 * wt.cos()) / (r2 - 1.0);
                    if beta < -1.0 {
                        p = -beta - (beta * beta - 1.0).sqrt();
                    }
                }
            }
            pole = p;
            let gain = (g1 * (1.0 - 2.0 * p * w1.cos() + p * p).sqrt() / (1.0 - p)).min(0.999_99);
            lines(&mut d_nut, &mut d_bridge, (one_pole_lag(pole, w1) + disp.lag(w1)) / w1);
            self.tuning = Tuning { nut: d_nut as f32, bridge: d_bridge as f32, pole, gain, dispersion: disp };
        }
        self.loss = [(self.tuning.gain * (1.0 - self.tuning.pole)) as f32, self.tuning.pole as f32];
        self.allpass = self.tuning.dispersion.coefficient as f32;
        self.leak = (-1.0 / (20.0 * period)).exp() as f32;
    }
}

impl<I: Interpolator> Stage for Waveguide<I> {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        let cap = (f64::from(sample_rate) / f64::from(self.lowest.max(1.0))).ceil() as usize + I::OLDER + 4;
        self.nut.allocate(cap);
        self.bridge.allocate(cap / 2 + I::OLDER + 4);
        self.update();
        self.reset();
    }

    fn reset(&mut self) {
        self.nut.reset();
        self.bridge.reset();
        self.loss_state = 0.0;
        self.allpass_state = [0.0; MAX_SECTIONS];
        self.excitation = Excitation::IDLE;
        self.sticking = true;
        self.contact = 0.0;
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let from_bridge = -self.bridge.interpolated(self.tuning.bridge);
        let raw = self.nut.interpolated(self.tuning.nut);
        // The nut's reflection: loss, stiffness, inversion.
        let mut v = self.loss[0] * raw + self.loss[1] * self.loss_state;
        self.loss_state = denormal::flush(v);
        let a = self.allpass;
        for s in self.allpass_state.iter_mut().take(self.tuning.dispersion.sections) {
            let y = a * v + *s;
            *s = denormal::flush(v - a * y);
            v = y;
        }
        let from_nut = -v;
        let incoming = from_nut + from_bridge + x + self.excitation.next(self.leak);
        let contact = match self.bow {
            Some(bow) => {
                let phi = bow.force / (2.0 * self.impedance);
                bow::contact(&self.friction, phi, bow.velocity, f64::from(incoming), &mut self.sticking) as f32
            }
            None => incoming,
        };
        let dv = contact - (from_nut + from_bridge);
        self.contact = contact;
        self.bridge.write(denormal::finite_or_zero(from_nut + dv));
        self.bridge.advance();
        self.nut.write(denormal::finite_or_zero(from_bridge + dv));
        self.nut.advance();
        from_bridge
    }

    /// The fundamental's T60, as 120 dB, plus the loop.
    fn tail(&self) -> Tail {
        let samples = f64::from(self.tuning.nut + self.tuning.bridge).max(1.0);
        let g = self.tuning.gain;
        if g <= 0.0 {
            return Tail::samples(samples.ceil() as u32 + 1);
        }
        Tail::from_pole(g.powf(1.0 / samples)).plus(samples.ceil() as u32 + 1)
    }
}

#[cfg(test)]
mod tests;
