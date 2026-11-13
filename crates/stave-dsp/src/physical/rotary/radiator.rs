//! One rotor of a rotary speaker, heard at two microphones.

use super::{Directivity, Mic, Rotor, Speed, Wall, MAX_WALLS, SPEED_OF_SOUND};
use crate::kernel::node::{Stage, Tail};
use crate::primitives::time::delay::DelayLine;
use crate::primitives::time::interpolators::{Interpolator, Lagrange3};
use core::f64::consts::TAU;

/// Iterations of the emission-time fixed point per path and sample,
/// warm-started from the last sample's: each gains a factor `v/c`
/// (about 0.02 for a horn at tremolo).
const ITERATIONS: usize = 2;
/// The farthest microphone `prepare` sizes the delay lines for unless
/// one is farther already, m: [`Radiator::set_mic`] refuses beyond the
/// prepared room.
const ROOM: f64 = 4.0;
/// The farthest wall `prepare` sizes for unless one is farther already,
/// m.
const WALL_ROOM: f64 = 1.0;

/// `cos δ` and `sin δ` for the small angles a rotor turns while sound
/// crosses the path (|δ| < 0.3 rad): Taylor to the 8th and 9th powers,
/// within 2e-12.
#[inline]
fn small_cos_sin(d: f64) -> (f64, f64) {
    let d2 = d * d;
    let c = 1.0 - d2 / 2.0 * (1.0 - d2 / 12.0 * (1.0 - d2 / 30.0 * (1.0 - d2 / 56.0)));
    let s = d * (1.0 - d2 / 6.0 * (1.0 - d2 / 20.0 * (1.0 - d2 / 42.0 * (1.0 - d2 / 72.0))));
    (c, s)
}

/// One rotor of a rotary speaker: a source spinning on a circle,
/// radiating through a [`Directivity`], heard at two [`Mic`]s directly
/// and by its reflections in up to [`MAX_WALLS`] [`Wall`]s.
///
/// # Models
/// Smith, Serafin, Abel & Berners, "Doppler Simulation and the Leslie"
/// (DAFx-02): the source at `r(cos θ, sin θ)` (their eq. 15), each
/// listener reading a delay line at the propagation delay, near field
/// and all (eqs. 1, 4, 18 — not the far-field approximation, eq. 20);
/// reflections as further read taps (their §3.4, Fig. 2) at the image
/// sources' delays. The rotor's speed approaches the switch's target as
/// a first-order lag — the motor's torque against the rotor's inertia
/// and drag — with the [`Rotor`]'s rise and fall times read as the time
/// to [`REACH`](super::REACH) of the change.
///
/// # Derivation
/// A microphone hears at `t` what the source emitted at `t − D`, where
/// `D·c` is the distance from the source's position *at* `t − D`: a
/// fixed point, solved per path and sample from the last sample's `D`
/// (two steps, each `v/c` closer), the rotor's angle at
/// `t − D` from its angle, speed and acceleration now. That delay alone
/// makes the received frequency `f/(1 + Ḋ)` — the moving-source Doppler
/// shift, exactly (eq. 1 with a still listener). The input is split by
/// a one-pole at the directivity's corner into two lines read at the
/// same delay, weighted by each lobe at the emission angle, and by
/// `distance_mic/distance_path` (spherical spreading, unity at the
/// axis's distance). Walls mirror the source and its axis (`p' = p −
/// 2(p·n − w)n`). The speed follows `ω ← ω + (ω* − ω)(1 − e^(−T/τ))`
/// with `τ` from the rise or fall time; the angle advances by the
/// trapezoid of the speed.
///
/// # Verified
/// `crates/stave-dsp/src/physical/rotary/tests.rs::doppler_is_the_moving_source_shift`
/// (every zero crossing of a 4 kHz tone arrives when the geometry says,
/// within 0.5 µs, at 48–192 kHz; the shift's peak against `rω/c`),
/// `crates/stave-dsp/src/physical/rotary/tests.rs::amplitude_follows_the_directivity`
/// (the envelope against [`Directivity::response`] and spreading,
/// within 0.05 dB, at 500 Hz–4 kHz (48 kHz) and 8 kHz (96 kHz) for the
/// horn and 100–400 Hz for the drum; Lagrange3 adds up to 0.23 dB of
/// droop at 8 kHz and 48 kHz as its fraction moves),
/// `crates/stave-dsp/src/physical/rotary/tests.rs::walls_add_their_image`,
/// and in stave-test
/// `crates/stave-test/tests/rotary_matches_reference.rs::rotors_ramp_in_the_published_times`
/// (the 122H guide's speeds and rise/fall times at 44.1–192 kHz).
///
/// # Cost
/// `"Radiator (horn, back wall, stereo)"` on the datasheet; the
/// [`Cabinet`](super::Cabinet) is two of them and a crossover.
///
/// # Aliasing
/// A modulated delay is a time-varying resampler: images leak through
/// the interpolator `I`'s stopband (Lagrange3 by default; use a
/// [`Sinc`](crate::primitives::time::interpolators::Sinc) for the top
/// octave). `I` must be a FIR interpolator: each line is read at
/// several delays a sample.
///
/// # Valid ranges
/// Tangential speeds well below `c` (a horn at tremolo moves about 6
/// m/s); microphones outside the rotor's circle, out to the room
/// `prepare` sized (4 m, or the farthest microphone set before it,
/// with walls out to 1 m or the farthest set before it); up to
/// [`MAX_WALLS`] walls, each farther from the axis than the radius.
/// Placements beyond the prepared lines are refused and counted, never
/// clamped.
#[derive(Clone, Debug)]
pub struct Radiator<I: Interpolator = Lagrange3> {
    rotor: Rotor,
    directivity: Directivity,
    mics: [Mic; 2],
    walls: [Wall; MAX_WALLS],
    wall_count: usize,
    sample_rate: f64,
    /// Revolutions a second, now and wanted; acceleration (rev/s²).
    rate: f64,
    target: f64,
    accel: f64,
    /// `1 − e^(−T/τ)` rising and falling.
    rise_step: f64,
    fall_step: f64,
    /// Angle, radians, counter-clockwise from the front.
    angle: f64,
    low: DelayLine<I>,
    high: DelayLine<I>,
    pole: f32,
    split: f32,
    /// Each path's last delay, samples: [mic][direct, walls…].
    delays: [[f64; MAX_WALLS + 1]; 2],
    /// The longest path the lines hold, m.
    reach: f64,
    refused: u32,
}

impl<I: Interpolator> Radiator<I> {
    /// `rotor` radiating through `directivity`, at chorale speed, heard
    /// by microphones 1 m away at ±60°, no walls. Allocates in
    /// `prepare` (and here, at 48 kHz).
    pub fn new(rotor: Rotor, directivity: Directivity) -> Self {
        let mut r = Self {
            rotor,
            directivity,
            mics: [Mic::at(60.0, 1.0), Mic::at(-60.0, 1.0)],
            walls: [Wall { angle: 0.0, distance: 1.0, reflection: 0.0 }; MAX_WALLS],
            wall_count: 0,
            sample_rate: 48_000.0,
            rate: f64::from(rotor.slow.get()),
            target: f64::from(rotor.slow.get()),
            accel: 0.0,
            rise_step: 0.0,
            fall_step: 0.0,
            angle: 0.0,
            low: DelayLine::default(),
            high: DelayLine::default(),
            pole: 0.0,
            split: 0.0,
            delays: [[0.0; MAX_WALLS + 1]; 2],
            reach: 0.0,
            refused: 0,
        };
        r.build();
        r
    }

    /// The rotor.
    pub fn rotor(&self) -> &Rotor {
        &self.rotor
    }

    /// Switch the speed; the rotor ramps there by its rise or fall time.
    pub fn set_speed(&mut self, speed: Speed) {
        self.target = match speed {
            Speed::Stop => 0.0,
            Speed::Chorale => f64::from(self.rotor.slow.get()),
            Speed::Tremolo => f64::from(self.rotor.fast.get()),
        };
    }

    /// Jump to the target speed now (a rotor already spinning there).
    pub fn settle(&mut self) {
        self.rate = self.target;
        self.accel = 0.0;
    }

    /// The speed now, revolutions a second.
    pub fn rate(&self) -> f64 {
        self.rate
    }

    /// The angle now, radians counter-clockwise from the front (0 at
    /// `reset`).
    pub fn angle(&self) -> f64 {
        self.angle
    }

    /// The microphones.
    pub fn mics(&self) -> [Mic; 2] {
        self.mics
    }

    /// Place microphone `i` (0 left, 1 right). Realtime; `false` (and
    /// counted in [`refused`](Self::refused)) when it is inside the
    /// rotor's circle, or a path to it would be longer than the lines
    /// `prepare` sized.
    pub fn set_mic(&mut self, i: usize, mic: Mic) -> bool {
        let farthest = self.farthest_wall(&self.walls[..self.wall_count]);
        if !(mic.distance > self.rotor.radius && self.path_bound(mic.distance, farthest) <= self.reach) {
            self.refused += 1;
            return false;
        }
        self.mics[i.min(1)] = mic;
        self.init_delays();
        true
    }

    /// The reflecting walls. Realtime; `false` (counted, the walls kept)
    /// for more than [`MAX_WALLS`] or a wall whose image path would be
    /// longer than the lines `prepare` sized.
    pub fn set_walls(&mut self, walls: &[Wall]) -> bool {
        let mic = self.mics.iter().map(|m| m.distance).fold(0.0, f64::max);
        if walls.len() > MAX_WALLS || self.path_bound(mic, self.farthest_wall(walls)) > self.reach {
            self.refused += 1;
            return false;
        }
        self.walls[..walls.len()].copy_from_slice(walls);
        self.wall_count = walls.len();
        self.init_delays();
        true
    }

    fn farthest_wall(&self, walls: &[Wall]) -> f64 {
        walls.iter().map(|w| w.distance).fold(self.rotor.radius, f64::max)
    }

    /// The longest path, m, to a microphone `mic` away with walls out to
    /// `wall`: the image is at most `2·wall + radius` from the axis.
    fn path_bound(&self, mic: f64, wall: f64) -> f64 {
        mic + self.rotor.radius + 2.0 * wall
    }

    /// Mic placements and walls refused so far.
    pub fn refused(&self) -> u32 {
        self.refused
    }

    /// The directivity (realtime).
    pub fn set_directivity(&mut self, directivity: Directivity) {
        self.directivity = directivity;
        self.pole = directivity.pole(self.sample_rate) as f32;
    }

    fn build(&mut self) {
        let fs = self.sample_rate;
        let step = |ramp| 1.0 - (-1.0 / (Rotor::time_constant(ramp) * fs)).exp();
        self.rise_step = step(self.rotor.rise);
        self.fall_step = step(self.rotor.fall);
        self.pole = self.directivity.pole(fs) as f32;
        let room = self.mics.iter().map(|m| m.distance).fold(ROOM, f64::max);
        let wall = self.farthest_wall(&self.walls[..self.wall_count]).max(WALL_ROOM);
        self.reach = self.path_bound(room, wall);
        let samples = (self.reach / SPEED_OF_SOUND * fs).ceil() as usize + I::NEWER + I::OLDER + 8;
        self.low.allocate(samples);
        self.high.allocate(samples);
        self.init_delays();
    }

    /// Start every path's fixed point at its geometric delay now.
    fn init_delays(&mut self) {
        let (s, c) = self.angle.sin_cos();
        for i in 0..2 {
            for j in 0..=self.wall_count {
                let d = self.path(i, j, c, s).0;
                self.delays[i][j] = d / SPEED_OF_SOUND * self.sample_rate;
            }
        }
    }

    /// Path `j` (0 direct, then the walls) to microphone `i` with the
    /// rotor at `(cos θ, sin θ)`: the distance, and the cosine of the
    /// angle between the source's axis and the way to the microphone.
    #[inline]
    fn path(&self, i: usize, j: usize, c: f64, s: f64) -> (f64, f64) {
        let r = self.rotor.radius;
        let (mut px, mut py, mut ax, mut ay) = (r * c, r * s, c, s);
        if j > 0 {
            let w = &self.walls[j - 1];
            let (ny, nx) = w.angle.to_radians().sin_cos();
            let k = 2.0 * (px * nx + py * ny - w.distance);
            px -= k * nx;
            py -= k * ny;
            let k = 2.0 * (ax * nx + ay * ny);
            ax -= k * nx;
            ay -= k * ny;
        }
        let (mx, my) = self.mics[i].position();
        let (vx, vy) = (mx - px, my - py);
        let d = (vx * vx + vy * vy).sqrt();
        (d, (ax * vx + ay * vy) / d)
    }

    /// Advance the rotor one sample.
    #[inline]
    fn spin(&mut self) {
        let step = if self.target.abs() > self.rate.abs() { self.rise_step } else { self.fall_step };
        let next = self.rate + (self.target - self.rate) * step;
        let t = 1.0 / self.sample_rate;
        let sign = if self.rotor.clockwise { -1.0 } else { 1.0 };
        self.angle = (self.angle + sign * TAU * 0.5 * (self.rate + next) * t).rem_euclid(TAU);
        self.accel = (next - self.rate) * self.sample_rate;
        self.rate = next;
    }

    /// Advance one sample: both microphones.
    #[inline]
    pub fn tick_stereo(&mut self, x: f32) -> (f32, f32) {
        self.spin();
        self.split = self.pole * self.split + (1.0 - self.pole) * x;
        self.split = crate::kernel::denormal::flush(self.split);
        self.low.write(self.split);
        self.high.write(x - self.split);
        let fs = self.sample_rate;
        let sign = if self.rotor.clockwise { -1.0 } else { 1.0 };
        // Angular speed and acceleration, rad/s and rad/s².
        let (w, a) = (sign * TAU * self.rate, sign * TAU * self.accel);
        let (s0, c0) = self.angle.sin_cos();
        let mut out = [0.0f32; 2];
        for (i, o) in out.iter_mut().enumerate() {
            let near = self.mics[i].distance;
            let mut sum = 0.0f64;
            for j in 0..=self.wall_count {
                let mut d = self.delays[i][j];
                let mut geometry = (0.0, 0.0);
                for _ in 0..ITERATIONS {
                    // The angle at emission, t − D.
                    let back = d / fs;
                    let (cd, sd) = small_cos_sin(w * back - 0.5 * a * back * back);
                    let (c, s) = (c0 * cd + s0 * sd, s0 * cd - c0 * sd);
                    geometry = self.path(i, j, c, s);
                    d = geometry.0 / SPEED_OF_SOUND * fs;
                }
                self.delays[i][j] = d;
                let (dist, cos_phi) = geometry;
                let reflection = if j == 0 { 1.0 } else { f64::from(self.walls[j - 1].reflection) };
                let level = reflection * near / dist;
                let lo = f64::from(self.low.interpolated(d as f32));
                let hi = f64::from(self.high.interpolated(d as f32));
                sum += level * (self.directivity.low.gain(cos_phi) * lo + self.directivity.high.gain(cos_phi) * hi);
            }
            *o = sum as f32;
        }
        self.low.advance();
        self.high.advance();
        (out[0], out[1])
    }
}

impl<I: Interpolator> Stage for Radiator<I> {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = f64::from(sample_rate.max(1.0));
        self.build();
    }

    /// Silence the lines and the split, and put the rotor back at angle
    /// 0 (its speed is kept).
    fn reset(&mut self) {
        self.low.reset();
        self.high.reset();
        self.split = 0.0;
        self.angle = 0.0;
        self.init_delays();
    }

    /// The left microphone.
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.tick_stereo(x).0
    }

    /// The longest path the lines hold.
    fn tail(&self) -> Tail {
        Tail::samples(self.low.capacity() as u32)
    }
}
