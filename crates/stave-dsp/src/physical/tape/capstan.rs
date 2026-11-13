//! The tape's speed past the play head (DSP.md H5): slow drift, the
//! motor's start and stop, and splices' jumps in time.

use super::Machine;
use crate::kernel::node::{Stage, Tail};
use crate::primitives::control::drift::OrnsteinUhlenbeck;
use crate::primitives::random::SplitMix64;

/// The fewest samples the read may sit behind the newest one: the cubic
/// reads one sample either side of its pair.
const MIN_LAG: f64 = 2.0;
/// The lag's slack beyond what its processes need, seconds.
const MARGIN_S: f64 = 0.001;
/// The position loop's largest correction, a fraction of the speed
/// (35 cents): a lag far from its target is chased, not snapped.
const SERVO_MAX: f64 = 0.02;
/// Below this fraction of its speed the tape's content sits under the
/// playback equaliser's low turnover, and the output falls with the
/// speed (an induction head's voltage is proportional to it).
const AUDIBLE: f64 = 0.05;
/// A coasting motor counts as stopped below this fraction of speed.
const STOPPED: f64 = 1e-3;
/// Drift's headroom in standard deviations of the position error.
const DRIFT_SIGMAS: f64 = 6.0;

/// Slow speed drift: the capstan's speed error as an Ornstein–Uhlenbeck
/// process, a fraction of the speed.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Drift {
    /// The speed error's standard deviation (0.001 is 0.1 %).
    pub rms: f64,
    /// Its correlation time, seconds.
    pub tau_s: f64,
}

impl Drift {
    pub const OFF: Self = Self { rms: 0.0, tau_s: 1.0 };
    /// A servo-controlled studio capstan: 0.02 % rms over 5 s
    /// (assumed; makers print the static tolerance, not the wander).
    pub const STEADY: Self = Self { rms: 2e-4, tau_s: 5.0 };
    /// A tired transport: 0.2 % rms over 2 s, the order of the static
    /// speed tolerance machines print, as a wander (assumed; voicing).
    pub const LOOSE: Self = Self { rms: 2e-3, tau_s: 2.0 };
}

/// How the transport stops.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Stop {
    /// Coasting against viscous drag: the speed decays as `e^(−t/τ)`.
    Coast { tau_s: f64 },
    /// A friction brake's constant torque: the speed falls linearly to
    /// zero in `seconds`.
    Brake { seconds: f64 },
}

/// The motor: a first-order spin-up and a stop.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Motor {
    /// The spin-up's time constant, seconds: the speed rises as
    /// `1 − e^(−t/τ)`, a motor's mechanical time constant `J·R/(k_t·k_e)`.
    pub start_s: f64,
    pub stop: Stop,
}

impl Motor {
    /// A studio deck: 0.1 s spin-up, braked to rest in 0.4 s (assumed:
    /// a direct-drive capstan with tension brakes; voicing).
    pub const STUDIO: Self = Self {
        start_s: 0.1,
        stop: Stop::Brake { seconds: 0.4 },
    };
    /// A consumer deck: 0.3 s spin-up, coasting down with a 0.5 s time
    /// constant (assumed: a belt-driven flywheel; voicing).
    pub const CONSUMER: Self = Self {
        start_s: 0.3,
        stop: Stop::Coast { tau_s: 0.5 },
    };
}

/// Splices: a tape cut and joined, its content jumping in time as the
/// joint passes the head.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Splices {
    /// Splices per minute of tape at the nominal speed (0: only
    /// [`Capstan::splice`]).
    pub per_minute: f64,
    /// The largest jump, seconds: each splice skips or repeats a
    /// uniform draw in `±jump_s`.
    pub jump_s: f64,
    /// The cut's angle to the tape's length, degrees (90 is a butt
    /// splice).
    pub angle_deg: f64,
    /// The tape's width, metres.
    pub width_m: f64,
}

impl Splices {
    pub const NONE: Self = Self {
        per_minute: 0.0,
        jump_s: 0.0,
        angle_deg: 45.0,
        width_m: 6.35e-3,
    };
    /// An edited quarter-inch master: two splices a minute, each cut at
    /// 45° (an editing block's diagonal slot), jumping up to 20 ms
    /// (assumed; voicing).
    pub const EDITED: Self = Self {
        per_minute: 2.0,
        jump_s: 0.02,
        angle_deg: 45.0,
        width_m: 6.35e-3,
    };

    /// How long the joint takes to cross the gap at `metres_per_second`:
    /// `width / (v · tan θ)` (a butt splice: none).
    pub fn crossing_s(&self, metres_per_second: f64) -> f64 {
        let t = self.angle_deg.to_radians().tan();
        if t.is_finite() && t.abs() < 1e12 {
            self.width_m / (metres_per_second * t)
        } else {
            0.0
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum State {
    Running,
    Stopping,
    Stopped,
}

/// The tape's speed past the play head: drift, start and stop, splices.
///
/// # Models
/// A transport playing a tape back: the capstan's slow speed drift
/// (Ornstein–Uhlenbeck, H9), the motor's first-order spin-up and its
/// stop (coasting or braked), and splices, where the content jumps in
/// time across a diagonal joint. Pitch follows the speed.
///
/// # Derivation
/// Zavalishin & Parker's tape coordinates (DAFx-18): the read position
/// in the input's time advances by the speed ratio
/// `r = m(t)·(1 + ε(t) + servo)` a sample, read by a cubic (Catmull–Rom)
/// from a line sized at `prepare`. `ε` is the drift; `m` the motor,
/// `1 − (1 − m₀)e^(−t/τ)` up, `e^(−t/τ)` coasting or `1 − t/T` braked
/// down. A first-order position loop (time constant `servo_s`, as a
/// phase-locked capstan or a synchroniser holds tape position) brings
/// the lag back to its base, so a running line stays bounded: it takes
/// out speed error below `1/(2π·servo_s)`, and the delivered speed
/// error's variance is `σ²·τ_s/(τ + τ_s)`, the lag's
/// `σ²·τ·τ_s²/(τ + τ_s)`. A start from rest cues the tape so the
/// spin-up's lost time `τ` lands the lag on its base. A splice cut at
/// angle `θ` on tape of width `w` crosses the gap in `w/(v·tan θ)`,
/// over which the head reads a linear mix of the two pieces (a straight
/// cut covers the gap's width linearly); splices arrive as a Poisson
/// process in tape length. Below 5 % of the speed the output falls in
/// proportion to it: the content is under the playback equaliser's low
/// turnover, where an induction head's voltage follows the speed
/// (an approximation of the equaliser, which is not modelled).
///
/// # Verified
/// `crates/stave-test/tests/tape_matches_reference.rs::capstan_drift_has_its_process_statistics`
/// (the speed error's variance within 4 % of `σ²` and correlation
/// within 0.03 of `e^(−Δt/τ)`; the delivered speed's and the lag's
/// variances within 6 % of the closed forms),
/// `crates/stave-test/tests/tape_matches_reference.rs::capstan_ramps_follow_their_time_constants`
/// (spin-up, coast and brake against their curves to 1e-9, a tone's
/// pitch through a spin-up within 0.5 % of the speed, the lag back on
/// its base) and
/// `crates/stave-test/tests/tape_matches_reference.rs::transport_off_is_bit_identical`.
///
/// # Cost
/// Datasheet `"Capstan (steady, studio motor, edited)"`.
///
/// # Aliasing
/// A modulated delay: its sidebands are the signal's; the cubic's own
/// loss above a quarter of the rate. A spin-up from rest sweeps the
/// pitch up from zero, so nothing is transposed above the input's band.
///
/// # Valid ranges
/// `rms` to 1 %, `tau_s` above a few samples, start and stop times to a
/// few seconds, jumps to a second; the latency is the base lag, fixed at
/// `prepare` from the configuration.
#[derive(Clone, Debug)]
pub struct Capstan {
    drift: Drift,
    motor: Option<Motor>,
    splices: Splices,
    servo_s: f64,
    speed: f64,
    seed: u64,
    enabled: bool,
    rate: f64,
    buf: Vec<f32>,
    mask: usize,
    /// Samples written, offset by the line's length so reads never
    /// index below zero.
    written: u64,
    /// The read position, in the input's samples.
    pos: f64,
    base: f64,
    max_lag: f64,
    ou: OrnsteinUhlenbeck,
    rng: SplitMix64,
    state: State,
    m: f64,
    up: f64,
    down: f64,
    epsilon: f64,
    ratio: f64,
    /// Tape (in nominal samples) until the next splice.
    next_splice: f64,
    /// A splice crossing: samples done, its length, its jump.
    fade: Option<(f64, f64, f64)>,
    spliced: bool,
    clamps: u64,
}

impl Capstan {
    /// Off (a pass-through) until configured; `seed` replays every draw.
    /// Prepared at 48 kHz at 7.5 ips.
    pub fn new(seed: u64) -> Self {
        let mut c = Self {
            drift: Drift::OFF,
            motor: None,
            splices: Splices::NONE,
            servo_s: 10.0,
            speed: Machine::ips(7.5),
            seed,
            enabled: false,
            rate: 0.0,
            buf: Vec::new(),
            mask: 0,
            written: 0,
            pos: 0.0,
            base: 0.0,
            max_lag: 0.0,
            ou: OrnsteinUhlenbeck::new(0.0, 0.0, 1.0, seed),
            rng: SplitMix64::new(seed),
            state: State::Running,
            m: 1.0,
            up: 0.0,
            down: 0.0,
            epsilon: 0.0,
            ratio: 1.0,
            next_splice: f64::INFINITY,
            fade: None,
            spliced: false,
            clamps: 0,
        };
        c.prepare(48_000.0);
        c
    }

    pub fn with_drift(mut self, drift: Drift) -> Self {
        self.drift = drift;
        self.configure()
    }

    pub fn with_motor(mut self, motor: Motor) -> Self {
        self.motor = Some(motor);
        self.configure()
    }

    pub fn with_splices(mut self, splices: Splices) -> Self {
        self.splices = splices;
        self.configure()
    }

    /// The position loop's time constant, seconds (default 10).
    pub fn with_servo(mut self, seconds: f64) -> Self {
        self.servo_s = seconds.max(0.1);
        self.configure()
    }

    fn configure(mut self) -> Self {
        self.ou = OrnsteinUhlenbeck::new(0.0, self.drift.rms, self.drift.tau_s, self.seed);
        let rate = self.rate as f32;
        self.prepare(rate);
        self
    }

    /// The nominal tape speed, metres per second: it times splice
    /// crossings and the tape length between splices (realtime).
    pub fn set_speed(&mut self, metres_per_second: f64) {
        self.speed = metres_per_second.max(1e-6);
    }

    pub fn drift(&self) -> Drift {
        self.drift
    }

    pub fn motor(&self) -> Option<Motor> {
        self.motor
    }

    pub fn servo_s(&self) -> f64 {
        self.servo_s
    }

    /// Whether anything is configured: off, `tick` returns its input.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Spin down (needs a [`Motor`]).
    pub fn stop(&mut self) {
        if self.motor.is_some() && self.state == State::Running {
            self.state = State::Stopping;
        }
    }

    /// Spin up (needs a [`Motor`]): from rest the tape is cued so the
    /// spin-up lands the lag on its base.
    pub fn start(&mut self) {
        if self.motor.is_none() {
            return;
        }
        if self.state == State::Stopped {
            self.m = 0.0;
            self.pos = (self.written - 1) as f64 - self.target();
        }
        self.state = State::Running;
    }

    pub fn is_stopped(&self) -> bool {
        self.state == State::Stopped
    }

    /// The speed ratio delivered at the last sample (1 is nominal).
    pub fn speed_ratio(&self) -> f64 {
        self.ratio
    }

    /// The motor's part of the speed at the last sample.
    pub fn motor_ratio(&self) -> f64 {
        self.m
    }

    /// The drift's speed error at the last sample.
    pub fn speed_error(&self) -> f64 {
        self.epsilon
    }

    /// How far the read sits behind the input, seconds.
    pub fn lag(&self) -> f64 {
        ((self.written - 1) as f64 - self.pos) / self.rate
    }

    /// Whether a splice reached the head at the last sample.
    pub fn spliced(&self) -> bool {
        self.spliced
    }

    /// Times the lag hit the line's ends (or a splice could not jump):
    /// never, inside the valid ranges.
    pub fn clamps(&self) -> u64 {
        self.clamps
    }

    /// A splice now (if splices are configured and the tape is moving).
    pub fn splice(&mut self) {
        if self.splices.jump_s > 0.0 || self.splices.per_minute > 0.0 {
            self.begin_splice();
        }
    }

    /// The lag the loop aims at: the base, less what a spin-up still
    /// has to lose (`(1 − m)·a/(1 − a)` samples, the discrete
    /// exponential's remaining deficit).
    fn target(&self) -> f64 {
        if self.motor.is_some() && self.up > 0.0 {
            self.base - (1.0 - self.m) * self.up / (1.0 - self.up)
        } else {
            self.base
        }
    }

    fn draw_splice_gap(&mut self) {
        self.next_splice = if self.splices.per_minute > 0.0 {
            -self.rng.next_open_unit().ln() * 60.0 / self.splices.per_minute * self.rate
        } else {
            f64::INFINITY
        };
    }

    fn begin_splice(&mut self) {
        if self.state != State::Running || self.fade.is_some() || self.ratio < AUDIBLE {
            return;
        }
        let lag = (self.written - 1) as f64 - self.pos;
        let mut jump = (2.0 * self.rng.next_centred()) * self.splices.jump_s * self.rate;
        let fits = |j: f64| (MIN_LAG..=self.max_lag).contains(&(lag - j));
        if !fits(jump) {
            jump = -jump;
        }
        if !fits(jump) {
            self.clamps += 1;
            jump = 0.0;
        }
        let length = (self.splices.crossing_s(self.speed * self.ratio) * self.rate).round().max(1.0);
        self.fade = Some((0.0, length, jump));
        self.spliced = true;
    }

    /// The cubic (Catmull–Rom) through the line at position `p`.
    #[inline]
    fn read(&self, p: f64) -> f64 {
        let i = p.floor();
        let t = p - i;
        let i = i as u64 as usize;
        let at = |k: usize| f64::from(self.buf[k & self.mask]);
        let (y0, y1, y2, y3) = (at(i.wrapping_sub(1)), at(i), at(i + 1), at(i + 2));
        let c1 = 0.5 * (y2 - y0);
        let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
        ((c3 * t + c2) * t + c1) * t + y1
    }
}

impl Stage for Capstan {
    fn prepare(&mut self, sample_rate: f32) {
        self.rate = f64::from(sample_rate);
        self.enabled = self.drift.rms > 0.0
            || self.motor.is_some()
            || self.splices.per_minute > 0.0
            || self.splices.jump_s > 0.0;
        if !self.enabled {
            self.buf = Vec::new();
            self.base = 0.0;
            return;
        }
        self.ou.prepare(sample_rate);
        let (sigma, tau, servo) = (self.drift.rms, self.drift.tau_s, self.servo_s);
        let drift_s = DRIFT_SIGMAS * sigma * servo * (tau / (tau + servo)).sqrt();
        let start_s = self.motor.map_or(0.0, |m| m.start_s);
        let jump_s = self.splices.jump_s;
        self.base = ((MARGIN_S + drift_s + start_s + jump_s) * self.rate).ceil() + MIN_LAG;
        let stop_s = match self.motor.map(|m| m.stop) {
            Some(Stop::Coast { tau_s }) => tau_s * (1.0 / STOPPED).ln(),
            Some(Stop::Brake { seconds }) => seconds,
            None => 0.0,
        };
        self.max_lag = 2.0 * self.base + (stop_s + jump_s) * self.rate;
        let len = (self.max_lag as usize + 8).next_power_of_two();
        if len != self.buf.len() {
            self.buf = vec![0.0; len];
        }
        self.mask = len - 1;
        self.up = self.motor.map_or(0.0, |m| (-1.0 / (self.rate * m.start_s.max(1e-6))).exp());
        self.down = match self.motor.map(|m| m.stop) {
            Some(Stop::Coast { tau_s }) => (-1.0 / (self.rate * tau_s.max(1e-6))).exp(),
            Some(Stop::Brake { seconds }) => 1.0 / (self.rate * seconds.max(1e-6)),
            None => 0.0,
        };
        self.reset();
    }

    fn reset(&mut self) {
        if !self.enabled {
            return;
        }
        self.buf.iter_mut().for_each(|s| *s = 0.0);
        self.written = self.buf.len() as u64;
        self.pos = (self.written - 1) as f64 - self.base;
        self.ou.reset();
        self.rng = SplitMix64::new(self.seed ^ 0x5911_ce5a_7e00_0001);
        self.state = State::Running;
        self.m = 1.0;
        self.epsilon = 0.0;
        self.ratio = 1.0;
        self.fade = None;
        self.spliced = false;
        self.clamps = 0;
        self.draw_splice_gap();
    }

    fn tick(&mut self, x: f32) -> f32 {
        if !self.enabled {
            return x;
        }
        self.buf[self.written as usize & self.mask] = x;
        self.written += 1;
        self.spliced = false;
        if self.state == State::Stopped {
            self.ratio = 0.0;
            return 0.0;
        }
        self.epsilon = if self.drift.rms > 0.0 { self.ou.tick_f64() } else { 0.0 };
        let mut servo = 0.0;
        match self.state {
            State::Running => {
                if self.m < 1.0 {
                    self.m = 1.0 - (1.0 - self.m) * self.up;
                    if 1.0 - self.m < 1e-12 {
                        self.m = 1.0;
                    }
                }
                // The lag after this step at the motor's speed alone.
                let lag = (self.written - 1) as f64 - self.pos - self.m;
                servo =((lag - self.target()) / (self.servo_s * self.rate)).clamp(-SERVO_MAX, SERVO_MAX);
            }
            State::Stopping => {
                match self.motor.map(|m| m.stop) {
                    Some(Stop::Coast { .. }) => {
                        self.m *= self.down;
                        if self.m < STOPPED {
                            self.m = 0.0;
                        }
                    }
                    _ => self.m = (self.m - self.down).max(0.0),
                }
                if self.m == 0.0 {
                    self.state = State::Stopped;
                    self.ratio = 0.0;
                    self.fade = None;
                    return 0.0;
                }
            }
            State::Stopped => unreachable!(),
        }
        let r = self.m * (1.0 + self.epsilon + servo);
        self.ratio = r;
        self.pos += r;
        let newest = (self.written - 1) as f64;
        let lag = newest - self.pos;
        if !(MIN_LAG..=self.max_lag).contains(&lag) {
            self.clamps += 1;
            self.pos = newest - lag.clamp(MIN_LAG, self.max_lag);
        }
        self.next_splice -= r;
        if self.next_splice <= 0.0 {
            self.draw_splice_gap();
            self.begin_splice();
        }
        let mut y = self.read(self.pos);
        if let Some((done, length, jump)) = self.fade {
            let t = (done + 1.0) / length;
            y += t * (self.read(self.pos + jump) - y);
            if done + 1.0 >= length {
                self.pos += jump;
                self.fade = None;
            } else {
                self.fade = Some((done + 1.0, length, jump));
            }
        }
        let gain = (r / AUDIBLE).clamp(0.0, 1.0);
        (gain * y) as f32
    }

    fn latency(&self) -> u32 {
        self.base as u32
    }

    fn tail(&self) -> Tail {
        if self.enabled {
            Tail::Samples(self.max_lag as u32 + 4)
        } else {
            Tail::None
        }
    }
}

#[cfg(test)]
mod tests;
