//! A granular engine over live input: windowed grains from a recorded
//! buffer, a fixed pool of voices.

use crate::kernel::node::{Stage, Tail};
use crate::units::{Seconds, Semitones};

/// One playing grain.
#[derive(Copy, Clone, Debug, Default)]
struct Grain {
    active: bool,
    /// Where it reads, in input samples (absolute).
    position: f64,
    /// Input samples per output sample.
    rate: f64,
    /// Samples played, and its length.
    age: u32,
    length: u32,
}

/// Points in the window table.
const WINDOW_POINTS: usize = 1024;

/// A granulator: the input recorded into a buffer, and grains of it —
/// Hann-windowed, each read at its own rate — started at a steady
/// density from a voice pool fixed at construction.
///
/// # Models
///
/// Granular synthesis (Roads, *Microsound*, 2001, ch. 3; Truax's
/// real-time granulation of sampled sound, 1988): grains of `size`
/// start every `1/density` seconds, each reading the input from
/// `position` (plus up to `spray` more, at random) seconds back, at
/// `2^(pitch/12)` times speed, under a Hann window. The sum is scaled
/// by `1/max(1, density·size/2)`, so steady, coherent grains (no spray,
/// no pitch) overlap-add to the input at unity. No hardware claim.
///
/// # Derivation
///
/// A phase accumulator schedules grains (exact density on average, any
/// rate); a grain takes the first free voice of the pool (or is dropped
/// and counted, [`dropped`](Self::dropped)); reads use cubic Hermite
/// interpolation; the start is held far enough back that a grain read
/// faster than real time never overtakes the newest input. A xorshift
/// generator draws the spray, seeded so a run repeats.
///
/// # Verified
///
/// `crates/stave-dsp/src/effects/granular/tests.rs::grains_follow_the_density`:
/// grains counted in the output match `density·time` within one, 5 to
/// 200 grains/s at 44.1, 48 and 96 kHz;
/// `crates/stave-dsp/src/effects/granular/tests.rs::grains_follow_the_pitch`:
/// each grain of a 440 Hz sine shifted −12 to +12 semitones measures
/// within 0.5 cents of the setting (worst 0.002);
/// `crates/stave-dsp/src/effects/granular/tests.rs::coherent_grains_are_unity`:
/// steady grains on DC sum to 1 within 1e-3; no allocation after
/// `prepare` (`crates/stave-dsp/tests/realtime_effects.rs::spectral_effects_never_allocate_after_prepare`).
///
/// # Cost
///
/// Datasheet `"Granulator 50 grains/s, 80 ms"`: per active grain, a
/// window lookup and a 4-point interpolation per sample.
///
/// # Aliasing
///
/// Upward-pitched grains read the buffer faster with a cubic
/// interpolator and no band-limiting: content above `fs/(2·rate)`
/// folds, as in a sampler without anti-aliasing.
///
/// # Valid ranges
///
/// `density > 0`, `size` from 1 ms, `pitch` within ±24 semitones,
/// `position + spray` within the `memory` given at construction (longer
/// is clamped). At most `voices` grains play at once (`density·size`
/// must stay below it, or grains drop). No latency.
///
/// ```
/// use stave_dsp::effects::granular::Granulator;
/// use stave_dsp::units::{ms, seconds, semitones};
/// use stave_dsp::Stage;
///
/// let mut cloud = Granulator::new(seconds(2.0), 64);
/// cloud.set_density(40.0);
/// cloud.set_size(ms(80.0));
/// cloud.set_pitch(semitones(12.0));
/// cloud.set_position(ms(100.0));
/// cloud.prepare(48_000.0);
/// let y: Vec<f32> = (0..9_600).map(|n| cloud.tick((n as f32 * 0.01).sin())).collect();
/// assert!(y[6_000..].iter().any(|v| v.abs() > 0.1));
/// assert_eq!(cloud.dropped(), 0);
/// ```
#[derive(Clone, Debug)]
pub struct Granulator {
    memory: Seconds,
    density: f64,
    size: Seconds,
    rate: f64,
    position: Seconds,
    spray: Seconds,
    sample_rate: f32,
    buffer: Vec<f32>,
    written: u64,
    grains: Vec<Grain>,
    /// Grains owed, fractional.
    phase: f64,
    rng: u32,
    dropped: u64,
    window: Vec<f32>,
}

impl Granulator {
    /// A granulator remembering `memory` of input with a pool of
    /// `voices` grains: 0.5 s back by default, 20 grains/s of 50 ms at
    /// the input's pitch.
    ///
    /// # Panics
    ///
    /// If `voices` is 0.
    pub fn new(memory: Seconds, voices: usize) -> Self {
        assert!(voices > 0, "Granulator needs at least one voice");
        let mut g = Self {
            memory,
            density: 20.0,
            size: Seconds::new(0.05),
            rate: 1.0,
            position: Seconds::new(0.0),
            spray: Seconds::new(0.0),
            sample_rate: 0.0,
            buffer: Vec::new(),
            written: 0,
            grains: vec![Grain::default(); voices],
            phase: 0.0,
            rng: 0x9e37_79b9,
            dropped: 0,
            window: (0..=WINDOW_POINTS)
                .map(|i| {
                    let x = i as f64 / WINDOW_POINTS as f64;
                    (0.5 - 0.5 * (core::f64::consts::TAU * x).cos()) as f32
                })
                .collect(),
        };
        g.plan(48_000.0);
        g
    }

    /// Grains started per second.
    pub fn set_density(&mut self, per_second: f64) {
        self.density = per_second.max(0.0);
    }

    /// Each grain's length.
    pub fn set_size(&mut self, size: Seconds) {
        self.size = size;
    }

    /// Each grain's transposition (clamped to ±24 semitones).
    pub fn set_pitch(&mut self, pitch: Semitones) {
        self.rate = (f64::from(pitch.get()).clamp(-24.0, 24.0) / 12.0).exp2();
    }

    /// How far back grains start reading.
    pub fn set_position(&mut self, position: Seconds) {
        self.position = position;
    }

    /// The most a grain's start is pushed further back, at random.
    pub fn set_spray(&mut self, spray: Seconds) {
        self.spray = spray;
    }

    /// Grains not started because every voice was busy, since the last
    /// reset: the pool's limit firing.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Grains playing now.
    pub fn active(&self) -> usize {
        self.grains.iter().filter(|g| g.active).count()
    }

    fn plan(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        // The memory plus the longest grain, which must not be
        // overwritten while it plays.
        let len = (self.memory.get() * f64::from(sample_rate)).ceil() as usize
            + (self.size.get().max(0.5) * f64::from(sample_rate)) as usize
            + 16;
        self.buffer = vec![0.0; len.next_power_of_two()];
        self.clear();
    }

    fn clear(&mut self) {
        self.buffer.fill(0.0);
        self.written = 0;
        for g in &mut self.grains {
            g.active = false;
        }
        self.phase = 0.0;
        self.rng = 0x9e37_79b9;
        self.dropped = 0;
    }

    /// A uniform draw in `[0, 1)`.
    fn random(&mut self) -> f64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        f64::from(self.rng) / (f64::from(u32::MAX) + 1.0)
    }

    /// Starts a grain on the first free voice.
    fn spawn(&mut self) {
        let fs = f64::from(self.sample_rate);
        let length = (self.size.get() * fs).round().max(2.0) as u32;
        let spray = self.spray.get() * self.random();
        let Some(slot) = self.grains.iter().position(|g| !g.active) else {
            self.dropped += 1;
            return;
        };
        // Far enough back that a fast grain never passes the newest
        // input, and no further than the buffer holds.
        let soonest = (self.rate - 1.0).max(0.0) * f64::from(length) + 4.0;
        let furthest = self.buffer.len() as f64 - f64::from(length) - 8.0;
        let delay = ((self.position.get() + spray) * fs).clamp(soonest, furthest.max(soonest));
        self.grains[slot] = Grain {
            active: true,
            position: self.written as f64 - delay,
            rate: self.rate,
            age: 0,
            length,
        };
    }

    /// The buffer at absolute position `p`, cubic Hermite.
    #[inline]
    fn read(&self, p: f64) -> f32 {
        let mask = self.buffer.len() - 1;
        let i = p.floor();
        let t = (p - i) as f32;
        let i = i as i64;
        let at = |k: i64| self.buffer[(i + k) as usize & mask];
        let (xm, x0, x1, x2) = (at(-1), at(0), at(1), at(2));
        let c1 = 0.5 * (x1 - xm);
        let c2 = xm - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
        let c3 = 0.5 * (x2 - xm) + 1.5 * (x0 - x1);
        ((c3 * t + c2) * t + c1) * t + x0
    }

    #[inline]
    fn window_at(&self, x: f64) -> f32 {
        let p = x * WINDOW_POINTS as f64;
        let i = (p as usize).min(WINDOW_POINTS - 1);
        let f = (p - i as f64) as f32;
        self.window[i] + (self.window[i + 1] - self.window[i]) * f
    }
}

impl Stage for Granulator {
    /// Re-plans the buffer for the rate (this allocates) and clears it.
    fn prepare(&mut self, sample_rate: f32) {
        if sample_rate != self.sample_rate {
            self.plan(sample_rate);
        }
        self.clear();
    }

    fn reset(&mut self) {
        self.clear();
    }

    /// Realtime: never allocates; costs a window lookup and a cubic read
    /// per active grain.
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let mask = self.buffer.len() - 1;
        self.buffer[self.written as usize & mask] = x;
        self.written += 1;
        self.phase += self.density / f64::from(self.sample_rate);
        while self.phase >= 1.0 {
            self.phase -= 1.0;
            self.spawn();
        }
        let mut y = 0.0f32;
        for k in 0..self.grains.len() {
            let g = self.grains[k];
            if !g.active {
                continue;
            }
            let w = self.window_at(f64::from(g.age) / f64::from(g.length));
            y += w * self.read(g.position);
            let g = &mut self.grains[k];
            g.position += g.rate;
            g.age += 1;
            if g.age >= g.length {
                g.active = false;
            }
        }
        let overlap = 0.5 * self.density * self.size.get();
        y / overlap.max(1.0) as f32
    }

    /// The longest grain still to play.
    fn tail(&self) -> Tail {
        Tail::samples((self.size.get() * f64::from(self.sample_rate)).ceil() as u32 + 1)
    }
}
