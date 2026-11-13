//! The head-to-tape spacing over time (DSP.md H5): dropouts, splice
//! joints and wear, each a loss by Wallace's spacing law.

use super::Machine;
use super::playback::dot;
use crate::kernel::node::{Stage, Tail};
use crate::primitives::control::drift::Wiener;
use crate::primitives::random::SplitMix64;

/// The filter's half span: 5 ms at every rate (the spacing loss's kernel
/// falls as `1/t²`; this span holds it within 0.5 dB of the law to
/// 100 µs of tape travel).
const HALF_SPAN_S: f64 = 0.005;
/// The largest spacing, as tape travel time `d/v` (seconds), the filter
/// is held to; beyond it the loss is clamped (and counted).
const MAX_TRAVEL_S: f64 = 1e-4;
/// Control updates per second: the spacing's taps are designed this
/// often and crossfaded between.
const CONTROL_HZ: f64 = 8_000.0;
/// Dropouts and joints at once; one more is dropped (and counted).
const EVENTS: usize = 8;
/// The slowest speed ratio the loss is evaluated at.
const MIN_RATIO: f64 = 1e-3;

/// A tape's condition: its dropouts, its splices' joints, its wear.
/// Depths are extra head-to-tape spacing, micrometres.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Condition {
    /// Dropouts per minute of tape at the nominal speed (a Poisson
    /// process: exponential gaps).
    pub dropouts_per_minute: f64,
    /// A dropout's mean extra spacing, µm (exponentially distributed:
    /// its depth in dB at a wavelength is exponential too).
    pub depth_um: f64,
    /// A dropout's median length in time at the nominal speed, ms
    /// (log-normal).
    pub duration_ms: f64,
    /// The standard deviation of the duration's logarithm.
    pub duration_spread: f64,
    /// A splice joint's extra spacing, µm, and its length along the
    /// tape, mm (`Spacing::splice`).
    pub joint_um: f64,
    pub joint_mm: f64,
    /// Wear now, 0 (fresh) to 1, and how much it grows an hour of play
    /// (a Wiener process with that drift and diffusion, never
    /// receding).
    pub wear: f64,
    pub wear_per_hour: f64,
    /// Full wear's extra spacing, µm (the surface's roughness), and its
    /// hiss, dBFS rms (scaled by the wear).
    pub wear_um: f64,
    pub hiss_dbfs: f64,
}

impl Condition {
    pub const NONE: Self = Self {
        dropouts_per_minute: 0.0,
        depth_um: 0.0,
        duration_ms: 0.0,
        duration_spread: 0.0,
        joint_um: 0.0,
        joint_mm: 0.0,
        wear: 0.0,
        wear_per_hour: 0.0,
        wear_um: 0.0,
        hiss_dbfs: f64::NEG_INFINITY,
    };
    /// Fresh tape: two shallow dropouts a minute (0.5 µm, 3 ms), clean
    /// joints (assumed; voicing).
    pub const FRESH: Self = Self {
        dropouts_per_minute: 2.0,
        depth_um: 0.5,
        duration_ms: 3.0,
        duration_spread: 0.6,
        joint_um: 2.0,
        joint_mm: 2.0,
        wear: 0.0,
        wear_per_hour: 0.0,
        wear_um: 2.0,
        hiss_dbfs: -90.0,
    };
    /// A used tape: 15 dropouts a minute (1 µm, 5 ms), wear 0.3 growing
    /// 0.01 an hour (assumed; voicing).
    pub const USED: Self = Self {
        dropouts_per_minute: 15.0,
        depth_um: 1.0,
        duration_ms: 5.0,
        duration_spread: 0.7,
        joint_um: 3.0,
        joint_mm: 3.0,
        wear: 0.3,
        wear_per_hour: 0.01,
        wear_um: 3.0,
        hiss_dbfs: -70.0,
    };
    /// A worn tape: one dropout a second (2 µm, 8 ms), wear 0.8 growing
    /// 0.05 an hour (assumed; voicing).
    pub const WORN: Self = Self {
        dropouts_per_minute: 60.0,
        depth_um: 2.0,
        duration_ms: 8.0,
        duration_spread: 0.8,
        joint_um: 5.0,
        joint_mm: 4.0,
        wear: 0.8,
        wear_per_hour: 0.05,
        wear_um: 4.0,
        hiss_dbfs: -60.0,
    };

    fn is_active(&self) -> bool {
        self.dropouts_per_minute > 0.0
            || (self.joint_um > 0.0 && self.joint_mm > 0.0)
            || (self.wear_um > 0.0 && (self.wear > 0.0 || self.wear_per_hour > 0.0))
    }
}

/// A dropout or a joint as it begins.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Dropout {
    /// Its peak extra spacing, µm.
    pub depth_um: f64,
    /// Its length at the nominal speed, seconds.
    pub seconds: f64,
    /// A splice's joint, not a dropout.
    pub joint: bool,
}

#[derive(Copy, Clone, Debug)]
struct Event {
    depth_um: f64,
    /// Samples since it began (negative: not yet), and its length.
    at: f64,
    length: f64,
}

/// The head-to-tape spacing over time: dropouts, joints and wear.
///
/// # Models
/// Contact loss between the play head and the tape: a dropout (debris
/// or a coating flaw lifting the tape), a splice's joint, and a worn
/// surface's roughness each add spacing `d`, which costs every
/// wavelength `λ` Wallace's `e^(−2πd/λ)` (54.6·d/λ dB; Chowdhury
/// DAFx-19 eq. 13's spacing term): a level dip that deepens with
/// frequency. Dropouts arrive as a Poisson process along the tape, with
/// exponential depths and log-normal lengths per [`Condition`]; wear
/// grows over playing time and raises a hiss.
///
/// # Derivation
/// At speed `v` the loss is `e^(−|ω|·c)` with `c = fs·d/v` samples, the
/// spacing as tape travel time. Its band-limited impulse response is in
/// closed form, `h[n] = c·(1 − (−1)ⁿe^(−πc))/(π(c² + n²))` (the inverse
/// DTFT of `e^(−c|ω|)` on `|ω| ≤ π`), Blackman-windowed over ±5 ms: zero
/// phase (the head's spatial response is symmetric), so the stage's
/// latency is its half span. The taps are redesigned at 8 kHz from the
/// spacing then (a dropout is a Hann bump of its depth over its length)
/// and crossfaded across each period; with no spacing the output is the
/// input, delayed. The speed ratio a [`Capstan`](super::Capstan) delivers
/// scales `c` by `1/r` (the same tape at a slower speed). Hiss: white
/// Gaussian noise at `wear × 10^(hiss_dbfs/20)` rms, read through the
/// same spacing. Every draw is seeded.
///
/// # Verified
/// `crates/stave-test/tests/tape_matches_reference.rs::spacing_loss_matches_eq_13`
/// (within 0.5 dB of eq. 13's spacing ratio from 20 Hz to 20 kHz
/// wherever it is above −60 dB, 0.5–19 µm at 3.75–15 ips and 44.1–96 kHz;
/// a dropout's peak at 10 kHz within 0.5 dB of 54.6·d/λ),
/// `crates/stave-test/tests/tape_matches_reference.rs::dropouts_have_their_process_statistics`
/// (rate within 3 standard errors, gaps' mean and coefficient of
/// variation within 5 % of the exponential's, depth mean and tail, the
/// log-duration's mean and spread) and
/// `crates/stave-test/tests/tape_matches_reference.rs::transport_off_is_bit_identical`.
///
/// # Cost
/// Datasheet `"Spacing (used tape)"` (wear and dropouts as they come)
/// and `"Spacing (in a dropout)"` (the crossfaded redesign every
/// period, the worst case).
///
/// # Aliasing
/// None: a linear, time-varying filter moving at 8 kHz in steps far
/// smaller than its own response.
///
/// # Valid ranges
/// Spacing to 100 µs of tape travel (19 µm at 7.5 ips; clamped beyond,
/// counted); any rate.
#[derive(Clone, Debug)]
pub struct Spacing {
    condition: Condition,
    speed: f64,
    ratio: f64,
    seed: u64,
    enabled: bool,
    rate: f64,
    half: usize,
    window: Vec<f64>,
    history: Vec<f64>,
    hpos: usize,
    from: Vec<f64>,
    to: Vec<f64>,
    c_from: f64,
    c_to: f64,
    period: usize,
    phase: usize,
    events: [Option<Event>; EVENTS],
    next_dropout: f64,
    rng: SplitMix64,
    noise: SplitMix64,
    hiss: f64,
    wear: f64,
    growth: Wiener,
    started: Option<Dropout>,
    /// A joint begun between ticks, reported after the next one.
    pending: bool,
    clamps: u64,
}

impl Spacing {
    /// A tape in `condition` (`Condition::NONE`: a pass-through);
    /// `seed` replays every draw. Prepared at 48 kHz at 7.5 ips.
    pub fn new(condition: Condition, seed: u64) -> Self {
        let mut s = Self {
            condition,
            speed: Machine::ips(7.5),
            ratio: 1.0,
            seed,
            enabled: false,
            rate: 0.0,
            half: 0,
            window: Vec::new(),
            history: Vec::new(),
            hpos: 0,
            from: Vec::new(),
            to: Vec::new(),
            c_from: 0.0,
            c_to: 0.0,
            period: 1,
            phase: 0,
            events: [None; EVENTS],
            next_dropout: f64::INFINITY,
            rng: SplitMix64::new(seed),
            noise: SplitMix64::new(seed),
            hiss: 0.0,
            wear: condition.wear,
            growth: Wiener::new(0.0, 0.0, 0.0, seed),
            started: None,
            pending: false,
            clamps: 0,
        };
        s.prepare(48_000.0);
        s
    }

    pub fn condition(&self) -> Condition {
        self.condition
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// The nominal tape speed, metres per second (realtime).
    pub fn set_speed(&mut self, metres_per_second: f64) {
        self.speed = metres_per_second.max(1e-6);
    }

    /// The transport's speed ratio now (from a [`Capstan`](super::Capstan)).
    pub fn set_speed_ratio(&mut self, ratio: f64) {
        self.ratio = ratio;
    }

    /// Wear now, 0 to 1 (realtime; it keeps growing at the condition's
    /// rate).
    pub fn set_wear(&mut self, wear: f64) {
        self.wear = wear.clamp(0.0, 1.0);
        self.prepare_growth();
    }

    pub fn wear(&self) -> f64 {
        self.wear
    }

    /// The spacing now (wear and every event at this instant), µm.
    pub fn spacing_um(&self) -> f64 {
        self.spacing_at(0.0)
    }

    /// The dropout or joint that began at the last sample.
    pub fn started(&self) -> Option<Dropout> {
        self.started
    }

    /// Times a limit fired: a spacing beyond the valid range, or an
    /// event with every slot busy.
    pub fn clamps(&self) -> u64 {
        self.clamps
    }

    /// A splice's joint reaches the head now: in the input's time, so
    /// its dip lands on the content the splice joined, the stage's
    /// latency later.
    pub fn splice(&mut self) {
        let c = self.condition;
        if self.enabled && c.joint_um > 0.0 && c.joint_mm > 0.0 {
            let delay = self.half as f64;
            self.begin(c.joint_um, c.joint_mm * 1e-3 / self.speed, true, delay);
            self.pending = true;
        }
    }

    fn prepare_growth(&mut self) {
        let per_hour = self.condition.wear_per_hour;
        let control = (self.rate / self.period as f64) as f32;
        let mut w = Wiener::new(self.wear, per_hour / 3600.0, per_hour / 60.0, self.seed ^ 0x3ea7);
        w.prepare(control);
        self.growth = w;
    }

    fn begin(&mut self, depth_um: f64, seconds: f64, joint: bool, delay: f64) {
        let length = (seconds * self.rate / self.ratio.max(MIN_RATIO)).max(1.0);
        match self.events.iter_mut().find(|e| e.is_none()) {
            Some(slot) => {
                *slot = Some(Event { depth_um, at: -delay, length });
                self.started = Some(Dropout { depth_um, seconds, joint });
            }
            None => self.clamps += 1,
        }
    }

    fn draw_gap(&mut self) {
        let per_minute = self.condition.dropouts_per_minute;
        self.next_dropout = if per_minute > 0.0 {
            -self.rng.next_open_unit().ln() * 60.0 / per_minute * self.rate
        } else {
            f64::INFINITY
        };
    }

    /// The spacing `ahead` samples from now, µm.
    fn spacing_at(&self, ahead: f64) -> f64 {
        let worn = self.wear * self.condition.wear_um;
        let bumps: f64 = self
            .events
            .iter()
            .flatten()
            .map(|e| {
                let x = ((e.at + ahead) / e.length).clamp(0.0, 1.0);
                e.depth_um * (std::f64::consts::PI * x).sin().powi(2)
            })
            .sum();
        worn + bumps
    }

    /// `c`, samples, for a spacing in µm at the present speed.
    fn travel(&mut self, um: f64) -> f64 {
        let seconds = um * 1e-6 / (self.speed * self.ratio.max(MIN_RATIO));
        if seconds > MAX_TRAVEL_S {
            self.clamps += 1;
        }
        seconds.min(MAX_TRAVEL_S) * self.rate
    }

    /// One control step: aim the taps at the spacing a period ahead.
    fn control(&mut self) {
        if self.condition.wear_per_hour > 0.0 {
            self.wear = self.wear.max(self.growth.tick_f64()).clamp(0.0, 1.0);
        }
        std::mem::swap(&mut self.from, &mut self.to);
        self.c_from = self.c_to;
        let um = self.spacing_at(self.period as f64);
        let c = if um > 0.0 { self.travel(um) } else { 0.0 };
        self.c_to = c;
        if c != self.c_from {
            design(c, &self.window, self.half, &mut self.to);
        } else {
            self.to.copy_from_slice(&self.from);
        }
        self.hiss = self.wear * 10f64.powf(self.condition.hiss_dbfs / 20.0);
    }
}

/// The spacing loss's taps for `c` samples of tape travel, windowed.
pub(super) fn design(c: f64, window: &[f64], half: usize, taps: &mut [f64]) {
    if c <= 0.0 {
        taps.iter_mut().for_each(|t| *t = 0.0);
        taps[half] = 1.0;
        return;
    }
    let tail = (-std::f64::consts::PI * c).exp();
    for k in 0..=half {
        let n = k as f64;
        let sign = if k % 2 == 0 { 1.0 } else { -1.0 };
        let h = c * (1.0 - sign * tail) / (std::f64::consts::PI * (c * c + n * n));
        taps[half + k] = h * window[half + k];
        taps[half - k] = h * window[half - k];
    }
}

impl Stage for Spacing {
    fn prepare(&mut self, sample_rate: f32) {
        self.rate = f64::from(sample_rate);
        self.enabled = self.condition.is_active();
        if !self.enabled {
            self.half = 0;
            return;
        }
        let half = (self.rate * HALF_SPAN_S).round() as usize;
        let n = 2 * half + 1;
        if n != self.window.len() {
            self.window = (0..n)
                .map(|j| {
                    let x = std::f64::consts::TAU * j as f64 / (n - 1) as f64;
                    0.42 - 0.5 * x.cos() + 0.08 * (2.0 * x).cos()
                })
                .collect();
            self.history = vec![0.0; 2 * n];
            self.from = vec![0.0; n];
            self.to = vec![0.0; n];
        }
        self.half = half;
        self.period = (self.rate / CONTROL_HZ).round().max(1.0) as usize;
        self.reset();
    }

    fn reset(&mut self) {
        if !self.enabled {
            return;
        }
        self.history.iter_mut().for_each(|h| *h = 0.0);
        self.hpos = 0;
        self.rng = SplitMix64::new(self.seed ^ 0xd809_0075);
        self.noise = SplitMix64::new(self.seed ^ 0x4155_0001);
        self.events = [None; EVENTS];
        self.started = None;
        self.pending = false;
        self.clamps = 0;
        self.wear = self.condition.wear;
        self.prepare_growth();
        self.draw_gap();
        // Start settled on the present spacing.
        let c = {
            let um = self.spacing_at(0.0);
            if um > 0.0 { self.travel(um) } else { 0.0 }
        };
        design(c, &self.window, self.half, &mut self.to);
        self.c_to = c;
        self.phase = 0;
        self.hiss = self.wear * 10f64.powf(self.condition.hiss_dbfs / 20.0);
    }

    fn tick(&mut self, x: f32) -> f32 {
        if !self.enabled {
            return x;
        }
        if !self.pending {
            self.started = None;
        }
        self.pending = false;
        if self.phase == 0 {
            self.control();
        }
        let mut x = f64::from(x);
        if self.hiss > 0.0 {
            x += self.hiss * self.noise.next_normal();
        }
        let n = self.from.len();
        self.history[self.hpos] = x;
        self.history[self.hpos + n] = x;
        let recent = &self.history[self.hpos + 1..=self.hpos + n];
        let y = if self.c_from == 0.0 && self.c_to == 0.0 {
            recent[n - 1 - self.half]
        } else if self.c_from == self.c_to {
            dot(recent, &self.to)
        } else {
            let t = (self.phase + 1) as f64 / self.period as f64;
            let a = dot(recent, &self.from);
            a + t * (dot(recent, &self.to) - a)
        };
        self.hpos += 1;
        if self.hpos == n {
            self.hpos = 0;
        }
        self.phase += 1;
        if self.phase == self.period {
            self.phase = 0;
        }
        for slot in self.events.iter_mut() {
            if let Some(e) = slot {
                e.at += 1.0;
                if e.at >= e.length {
                    *slot = None;
                }
            }
        }
        self.next_dropout -= self.ratio;
        if self.next_dropout <= 0.0 {
            self.draw_gap();
            let c = self.condition;
            let depth = -self.rng.next_open_unit().ln() * c.depth_um;
            let seconds = c.duration_ms * 1e-3 * (c.duration_spread * self.rng.next_normal()).exp();
            self.begin(depth, seconds, false, 0.0);
        }
        y as f32
    }

    fn latency(&self) -> u32 {
        self.half as u32
    }

    fn tail(&self) -> Tail {
        if self.enabled {
            Tail::Samples(2 * self.half as u32 + 1)
        } else {
            Tail::None
        }
    }
}

#[cfg(test)]
mod tests;
