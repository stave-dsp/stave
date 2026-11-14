//! A parametric close microphone on a loudspeaker driver's cone.

use super::{Pattern, SPEED_OF_SOUND};
use crate::kernel::node::{Stage, Tail};
use crate::physical::speaker::Driver;
use crate::primitives::filters::biquad::{Biquad, FilterSpec};
use crate::primitives::time::delay::DelayLine;
use crate::primitives::time::interpolators::{Interpolator, Sinc};
use crate::units::{Db, Hz};

/// The farthest a [`CloseMic`] may stand from the cone, m.
pub const MAX_DISTANCE: f64 = 4.0;

/// The most breakup modes a [`Breakup`] holds.
pub const MAX_MODES: usize = 8;

/// The interpolator reading the cone's velocity history.
type Read = Sinc<8>;

/// Where a microphone stands: `distance` m from the cone's plane, along
/// its axis, `offset` m off the axis, facing the cone; and its pattern.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Placement {
    pub distance: f64,
    pub offset: f64,
    pub pattern: Pattern,
}

impl Placement {
    /// A cardioid 2.5 cm from the cone on its axis: the guitar-cabinet
    /// close microphone.
    pub const CLOSE: Self = Self { distance: 0.025, offset: 0.0, pattern: Pattern::CARDIOID };
}

/// The cone's breakup, approximated: second-order peaks and dips
/// (`(hz, q, db)`) and a second-order rolloff (`rolloff` Hz, 0 for none).
/// A voicing, not a claim — a lumped driver is a piston, and where a real
/// paper cone breaks up depends on its paper, its profile and its
/// surround.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Breakup {
    modes: [(f32, f32, f32); MAX_MODES],
    count: usize,
    pub rolloff: f32,
}

impl Breakup {
    /// A piston: no breakup.
    pub const NONE: Self = Self { modes: [(0.0, 0.0, 0.0); MAX_MODES], count: 0, rolloff: 0.0 };

    /// A 12-inch paper guitar cone, voiced from the shape of Jensen's
    /// published responses (`crates/stave-test/datasheet/jensen_*_spl.txt`:
    /// a presence rise from 1.5 kHz peaking near 2.5–3 kHz, a dip near 4
    /// kHz, the treble gone by 6 kHz). Voicing: no breakup model is
    /// fitted.
    pub const PAPER_12: Self = Self {
        modes: [
            (1_500.0, 1.5, 3.0),
            (2_800.0, 2.5, 5.0),
            (4_200.0, 3.0, -6.0),
            (0.0, 0.0, 0.0),
            (0.0, 0.0, 0.0),
            (0.0, 0.0, 0.0),
            (0.0, 0.0, 0.0),
            (0.0, 0.0, 0.0),
        ],
        count: 3,
        rolloff: 5_500.0,
    };

    /// Modes `(hz, q, db)` and a rolloff.
    ///
    /// # Panics
    ///
    /// With more than [`MAX_MODES`] modes.
    pub fn new(modes: &[(f32, f32, f32)], rolloff: f32) -> Self {
        assert!(modes.len() <= MAX_MODES, "Breakup takes up to {MAX_MODES} modes");
        let mut b = Self::NONE;
        b.modes[..modes.len()].copy_from_slice(modes);
        b.count = modes.len();
        b.rolloff = rolloff;
        b
    }

    /// The modes.
    pub fn modes(&self) -> &[(f32, f32, f32)] {
        &self.modes[..self.count]
    }
}

/// `x` at which the baffled piston's far-field directivity `2·J₁(x)/x`
/// (`x = k·a·sin θ`) is 3 dB down.
pub const PISTON_MINUS_3_DB: f64 = 1.616_339_9;

/// A first-order microphone close to a loudspeaker driver's cone: the
/// driver ([`Driver`]) radiating as a baffled piston, heard at a
/// placement.
///
/// # Models
/// On the axis of a rigid circular piston of radius `a` in an infinite
/// baffle, at distance `r` (`r₂ = √(r² + a²)` to the rim), the exact
/// pressure and particle velocity (Kinsler et al., *Fundamentals of
/// Acoustics*, 4th ed., §7.4: `p = ρc·[v(t − r/c) − v(t − r₂/c)]`, and its
/// gradient) are
///
/// ```text
/// p/ρc = v(t − r/c) − v(t − r₂/c)        u = v(t − r/c) − (r/r₂)·v(t − r₂/c)
/// ```
///
/// where `v` is the cone's velocity. A first-order microphone `A + B·cos
/// θ` facing the cone hears `A·p/ρc + B·u`: two delayed taps of the
/// cone's velocity. That is the proximity effect exactly, for a piston —
/// far away (`r ≫ a`) `u/(p/ρc)` tends to the point source's `1 +
/// c/(jωr)`, close in the bass rises less because the near field is
/// plane. The distance delay `r/c` is in the taps. Approximated: a
/// microphone `offset` off the axis hears the on-axis taps through a
/// second-order lowpass at the piston's far-field −3 dB frequency for
/// that angle (`k·a·sin θ = 1.616`, θ = atan(offset/distance)); the cone's
/// breakup is a [`Breakup`] voicing. Not modelled: the microphone's own
/// capsule response, the cabinet (the baffle is infinite), the dust cap.
///
/// # Derivation
/// The cone's velocity from the [`Driver`] (its own equations), written
/// to a delay line and read at `r/c` and `r₂/c` through an 8-tap windowed
/// sinc; the taps weighted `1` and `A + B·r/r₂`, scaled so that a
/// pressure microphone's small-signal mass-controlled band is at unity
/// (`p/ρc ≈ (r₂ − r)/c · dv/dt` there). The off-axis lowpass and the
/// breakup are RBJ biquads.
///
/// # Verified
/// `crates/stave-dsp/src/physical/cabinet/tests.rs`: the output is the
/// analytic piston (taps at `r/c`, `r₂/c`, the interpolator's exact
/// response) over the driver's analytic velocity to 1e-6
/// (`close_mic_is_the_piston_on_axis`); its gradient-to-pressure ratio
/// is the piston's exactly and, at 2 m, the point source's proximity
/// effect `|1 + c/(jωr)|` within 0.05 dB from 40 Hz to 1 kHz
/// (`proximity_effect_matches_the_formula`); the off-axis lowpass is 3 dB
/// down where the piston's directivity is (`off_axis_lowpass_meets_the_piston`).
///
/// # Cost
/// `"CloseMic (C12N, cardioid, breakup)"` on the datasheet.
///
/// # Aliasing
/// As the [`Driver`]; the rest is linear.
///
/// # Valid ranges
/// `distance` from three samples of travel (2.1 cm at 48 kHz) to
/// [`MAX_DISTANCE`] (closer or farther is refused and counted,
/// [`refused`](Self::refused)); `offset ≥ 0`; breakup and rolloff
/// frequencies below 0.49 of the rate.
#[derive(Clone, Debug)]
pub struct CloseMic {
    driver: Driver,
    placement: Placement,
    breakup: Breakup,
    rate: f64,
    line: DelayLine<Read>,
    /// Tap delays, samples, and the far tap's weight.
    near: f32,
    far: f32,
    weight: f32,
    /// `(r₂ − r)/c`, s.
    tau: f64,
    off_axis: Option<Biquad>,
    modes: [Biquad; MAX_MODES],
    rolloff: Option<Biquad>,
    refused: u64,
}

impl CloseMic {
    /// A microphone at `placement` on `driver`'s cone, the cone breaking
    /// up as `breakup`.
    pub fn new(driver: Driver, placement: Placement, breakup: Breakup) -> Self {
        let mut m = Self {
            driver,
            placement,
            breakup,
            rate: 48_000.0,
            line: DelayLine::default(),
            near: 0.0,
            far: 0.0,
            weight: 0.0,
            tau: 1.0,
            off_axis: None,
            modes: core::array::from_fn(|_| Biquad::passthrough()),
            rolloff: None,
            refused: 0,
        };
        m.design();
        m
    }

    /// The driver (its drive level and source resistance are its own).
    pub fn driver(&mut self) -> &mut Driver {
        &mut self.driver
    }

    /// The placement.
    pub fn placement(&self) -> Placement {
        self.placement
    }

    /// The closest distance at the prepared rate, m.
    pub fn closest(&self) -> f64 {
        Read::NEWER as f64 * SPEED_OF_SOUND / self.rate
    }

    /// Move the microphone; `false` (and counted) when the distance is
    /// outside [`closest`](Self::closest)`..=`[`MAX_DISTANCE`] or the offset
    /// is negative, and then it stays. Realtime-safe (the taps jump: move
    /// it between notes, or smooth the distance yourself).
    pub fn set_placement(&mut self, placement: Placement) -> bool {
        let ok = placement.distance >= self.closest() - 1e-12
            && placement.distance <= MAX_DISTANCE
            && placement.offset >= 0.0
            && (0.0..=1.0).contains(&placement.pattern.pressure);
        if ok {
            self.placement = placement;
            self.design();
        } else {
            self.refused += 1;
        }
        ok
    }

    /// Placements refused since construction.
    pub fn refused(&self) -> u64 {
        self.refused
    }

    /// Set the breakup voicing. Realtime-safe.
    pub fn set_breakup(&mut self, breakup: Breakup) {
        self.breakup = breakup;
        self.design();
    }

    /// The taps' delays (samples) and the far tap's weight, as designed.
    pub fn taps(&self) -> (f32, f32, f32) {
        (self.near, self.far, self.weight)
    }

    /// The scale applied to the taps' difference: `1/(τ·a)`, `τ = (r₂ −
    /// r)/c` and `a` the driver's mass-band acceleration per unit.
    pub fn scale(&self) -> f64 {
        1.0 / (self.tau * self.driver.mass_band_acceleration())
    }

    /// Recompute taps and filters from the placement (no allocation).
    fn design(&mut self) {
        let p = self.placement;
        let ts = *self.driver.parameters();
        let a = ts.radius();
        let r = p.distance;
        let r2 = (r * r + a * a).sqrt();
        let fs = self.rate;
        self.near = (r / SPEED_OF_SOUND * fs) as f32;
        self.far = (r2 / SPEED_OF_SOUND * fs) as f32;
        self.weight = (p.pattern.pressure + p.pattern.gradient() * r / r2) as f32;
        self.tau = (r2 - r) / SPEED_OF_SOUND;
        let nyquist = 0.49 * fs;
        self.off_axis = if p.offset > 0.0 {
            let sin = (p.offset / (p.offset * p.offset + r * r).sqrt()).max(1e-9);
            let hz = PISTON_MINUS_3_DB * SPEED_OF_SOUND / (core::f64::consts::TAU * a * sin);
            (hz < nyquist).then(|| {
                let mut b = Biquad::new(FilterSpec::Lowpass { frequency: Hz::new(hz as f32), q: core::f32::consts::FRAC_1_SQRT_2 });
                b.prepare(fs as f32);
                b
            })
        } else {
            None
        };
        for (k, b) in self.modes.iter_mut().enumerate() {
            match self.breakup.modes().get(k) {
                Some(&(hz, q, db)) if f64::from(hz) < nyquist => {
                    b.set(FilterSpec::Peak { frequency: Hz::new(hz), gain: Db::new(db), q });
                    b.prepare(fs as f32);
                }
                _ => *b = Biquad::passthrough(),
            }
        }
        let roll = self.breakup.rolloff;
        self.rolloff = (roll > 0.0 && f64::from(roll) < nyquist).then(|| {
            let mut b = Biquad::new(FilterSpec::Lowpass { frequency: Hz::new(roll), q: core::f32::consts::FRAC_1_SQRT_2 });
            b.prepare(fs as f32);
            b
        });
    }
}

impl Stage for CloseMic {
    fn prepare(&mut self, sample_rate: f32) {
        self.rate = f64::from(sample_rate);
        self.driver.prepare(sample_rate);
        let a = self.driver.parameters().radius();
        let longest = (MAX_DISTANCE * MAX_DISTANCE + a * a).sqrt() / SPEED_OF_SOUND * self.rate;
        self.line.allocate(longest.ceil() as usize + Read::NEWER + Read::OLDER + 2);
        if self.placement.distance < self.closest() {
            self.placement.distance = self.closest();
            self.refused += 1;
        }
        self.design();
        self.reset();
    }

    fn reset(&mut self) {
        self.driver.reset();
        self.line.reset();
        for b in &mut self.modes {
            b.reset();
        }
        if let Some(b) = &mut self.off_axis {
            b.reset();
        }
        if let Some(b) = &mut self.rolloff {
            b.reset();
        }
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.driver.tick(x);
        self.line.write(self.driver.velocity() as f32);
        let near = self.line.interpolated(self.near);
        let far = self.line.interpolated(self.far);
        self.line.advance();
        let mut y = ((f64::from(near) - f64::from(self.weight) * f64::from(far)) * self.scale()) as f32;
        if let Some(b) = &mut self.off_axis {
            y = b.tick(y);
        }
        for b in &mut self.modes[..self.breakup.count] {
            y = b.tick(y);
        }
        if let Some(b) = &mut self.rolloff {
            y = b.tick(y);
        }
        y
    }

    fn tail(&self) -> Tail {
        self.driver.tail().then(Tail::Samples(self.far.ceil() as u32 + Read::OLDER as u32))
    }
}
