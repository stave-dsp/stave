//! Interleaved velvet-noise late reverberation (Välimäki & Prawda 2021):
//! four feedback loops, each read through a sparse FIR of one extended
//! velvet-noise sequence, interleaved so no two impulses ever coincide.

use crate::kernel::denormal;
use crate::kernel::node::{Stage, Tail};

use super::attenuation::{loop_target, Design, GraphicEq};
use super::t60::T60;
use super::{decay_tail, loop_gain};

/// Branches (interleaved sequences): four is where Välimäki & Prawda's
/// listeners stopped hearing the repetition.
const BRANCHES: usize = 4;

/// Sequence lengths in grid cells: primes, so the sequences never line
/// up (the paper's 97, 101, 103, 107).
const PRIMES: [usize; BRANCHES] = [97, 101, 103, 107];

/// The default impulse density, per second: 2205 (a grid of 20 samples
/// at 44.1 kHz), where velvet noise sounds as smooth as white noise.
const DENSITY: f32 = 2_205.0;

/// Samples per chunk of [`Stage::process`]: shorter than every loop.
const CHUNK: usize = 64;

/// A seed for the impulse positions and signs: the same sequences at
/// every rate and on every run.
const SEED: u32 = 0x5EED_0B0E;

/// One branch: a feedback loop of `length` samples, its sparse taps, its
/// attenuation, and the recent outputs the interleaving delays read.
#[derive(Clone, Debug, Default)]
struct Branch {
    /// The loop's samples, a power-of-two ring.
    line: Vec<f32>,
    length: usize,
    /// Tap delays (samples back from the newest) and signed weights.
    taps: Vec<usize>,
    signs: Vec<f32>,
    weights: Vec<f32>,
    gain: f32,
    eq: GraphicEq,
    /// The branch's last outputs, for the interleaving delays.
    recent: Vec<f32>,
}

/// A late reverberator of interleaved velvet noise: its impulse response
/// is exactly one ±impulse in every grid cell of `fs/density` samples,
/// decaying at the set reverberation time.
///
/// # Models
/// Välimäki & Prawda, "Late-reverberation synthesis using interleaved
/// velvet-noise sequences", IEEE/ACM TASLP 29, 2021: `M = 4` branches,
/// branch `i` a feedback loop of `Lᵢ = Cᵢ·M·T` samples (`Cᵢ` prime, `T`
/// the grid) read by a sparse FIR whose taps are one extended
/// velvet-noise sequence (one impulse in the first `T` samples of each
/// `M·T` cell), the branches offset by `i·T` so their impulses
/// interleave and never collide. No hardware claim; wet only; the right
/// channel interleaves the branches in the opposite order (4-3-2-1), the
/// paper's decorrelated stereo.
///
/// # Derivation
/// Each loop falls `Lᵢ·60/(fs·T60)` dB per pass: a gain, or with
/// [`set_t60`](Self::set_t60) the same graphic-EQ attenuation filter as
/// [`Fdn`](super::Fdn)'s. Within a pass each tap is weighted by the
/// decay at its own delay at the 1 kHz band's T60 (the limit of the
/// paper's segmented decay: every impulse its own segment, free here
/// because a float tap multiplies anyway), so at that band the envelope
/// is a smooth exponential, not a staircase. Positions and signs come
/// from a fixed seed; the grid is `round(fs/density)` samples, so the
/// reverb is the same in seconds at every rate.
///
/// # Verified
/// In `crates/stave-dsp/src/effects/reverb/tests.rs`:
/// `velvet_noise_density_is_the_design` (the impulse response has
/// exactly one non-zero sample per grid cell for its whole decay: the
/// density is `fs/round(fs/2205)`, 2181.8 /s at 48 kHz, exactly, and
/// no two impulses coincide), `velvet_noise_t60_matches_the_setting`
/// (broadband T30 within 3 % of the setting at 1 and 2.5 s; per octave
/// band with a profile, 125 Hz – 4 kHz within 5 %), plus the shared
/// stability, rate-invariance and tail tests every reverb runs.
///
/// # Cost
/// `"VelvetNoise (4 branches, 408 taps)"` (per sample, `tick`),
/// `"VelvetNoise (4 branches, 408 taps, process)"` (64-sample blocks,
/// where each tap reads a contiguous run and vectorises; bit-identical,
/// `velvet_noise_process_is_tick`) and `"VelvetNoise (graphic EQ)"` on
/// the datasheet, beside the `Fdn`'s. Its 408 taps are the density
/// times the loop period, so in floating point it is not cheaper than
/// the shelf-absorption `Fdn`; its case is an exact echo density from
/// the first sample and a smooth, non-repeating late field.
///
/// # Aliasing
/// None (linear, integer delays).
///
/// # Valid ranges
/// T60 in `(0, ∞]` seconds; below about twice the longest loop (0.4 s)
/// the other bands step audibly once per loop (the paper's limit).
/// Density 500–10 000 impulses per second. Memory: four loops of
/// about 200 ms.
#[derive(Clone, Debug)]
pub struct VelvetNoise {
    branches: [Branch; BRANCHES],
    t60_s: f32,
    profile: Option<T60>,
    density: f32,
    sample_rate: f32,
    /// The grid `T`, samples.
    grid: usize,
    design: Design,
    /// The write position, shared by every ring.
    pos: usize,
}

impl Default for VelvetNoise {
    fn default() -> Self {
        Self::new()
    }
}

impl VelvetNoise {
    /// Interleaved branches.
    pub const BRANCHES: usize = BRANCHES;

    /// T60 1.5 s at 2205 impulses per second. The loops are allocated in
    /// `prepare`.
    pub fn new() -> Self {
        Self {
            branches: Default::default(),
            t60_s: 1.5,
            profile: None,
            density: DENSITY,
            sample_rate: 48_000.0,
            grid: 0,
            design: Design::new(48_000.0),
            pos: 0,
        }
    }

    /// The same T60 in every band, seconds (`f32::INFINITY` never
    /// decays). Replaces a [`set_t60`](Self::set_t60) profile.
    // TODO(U4): take `units::Seconds` once it lands.
    pub fn set_t60_s(&mut self, t60_s: f32) {
        debug_assert!(t60_s > 0.0, "T60 {t60_s} s");
        self.t60_s = t60_s;
        self.profile = None;
        self.update();
    }

    /// A reverberation time per octave band, held by a graphic-EQ
    /// attenuation filter in every loop. Allocation-free.
    pub fn set_t60(&mut self, profile: T60) {
        self.profile = Some(profile);
        self.update();
    }

    /// Impulses per second, in `[500, 10 000]`; takes effect at the next
    /// `prepare` (it moves every tap).
    pub fn set_density(&mut self, per_second: f32) {
        self.density = per_second.clamp(500.0, 10_000.0);
    }

    /// The grid `T` in samples at the prepared rate: one impulse in
    /// every `T` (0 before `prepare`).
    pub fn grid(&self) -> usize {
        self.grid
    }

    /// Mono in, stereo out (wet only).
    #[inline]
    pub fn tick_stereo(&mut self, x: f32) -> (f32, f32) {
        if self.grid == 0 {
            return (0.0, 0.0); // not prepared
        }
        let pos = self.pos;
        let graphic = self.profile.is_some();
        let (mut left, mut right) = (0.0f32, 0.0f32);
        for (i, b) in self.branches.iter_mut().enumerate() {
            let mask = b.line.len() - 1;
            let oldest = b.line[(pos + b.line.len() - b.length) & mask];
            let fed = if graphic {
                b.eq.tick(oldest)
            } else {
                b.gain * oldest
            };
            b.line[pos & mask] = denormal::flush(x + fed);
            let mut y = 0.0f32;
            for (&k, &w) in b.taps.iter().zip(&b.weights) {
                y += w * b.line[(pos + b.line.len() - k) & mask];
            }
            let rmask = b.recent.len() - 1;
            b.recent[pos & rmask] = y;
            let grid = self.grid;
            left += b.recent[(pos + b.recent.len() - i * grid) & rmask];
            right += b.recent[(pos + b.recent.len() - (BRANCHES - 1 - i) * grid) & rmask];
        }
        self.pos = pos.wrapping_add(1);
        (left, right)
    }

    /// One chunk (at most [`CHUNK`] samples, shorter than every loop) of
    /// [`Stage::process`].
    fn process_chunk(&mut self, chunk: &mut [f32]) {
        let n = chunk.len();
        let pos = self.pos;
        let graphic = self.profile.is_some();
        let mut ys = [[0.0f32; CHUNK]; BRANCHES];
        for (b, y) in self.branches.iter_mut().zip(&mut ys) {
            let len = b.line.len();
            let mask = len - 1;
            // The feedback first: it reads a loop length back, before
            // this chunk.
            for (j, &x) in chunk.iter().enumerate() {
                let at = pos.wrapping_add(j);
                let oldest = b.line[(at + len - b.length) & mask];
                let fed = if graphic {
                    b.eq.tick(oldest)
                } else {
                    b.gain * oldest
                };
                b.line[at & mask] = denormal::flush(x + fed);
            }
            // Then the taps, each over the chunk's contiguous run (split
            // where the ring wraps).
            let y = &mut y[..n];
            for (&k, &w) in b.taps.iter().zip(&b.weights) {
                let start = (pos + len - k) & mask;
                let first = n.min(len - start);
                for (yj, &u) in y[..first].iter_mut().zip(&b.line[start..start + first]) {
                    *yj += w * u;
                }
                for (yj, &u) in y[first..].iter_mut().zip(&b.line[..n - first]) {
                    *yj += w * u;
                }
            }
            let rmask = b.recent.len() - 1;
            for (j, &yj) in y.iter().enumerate() {
                b.recent[pos.wrapping_add(j) & rmask] = yj;
            }
        }
        let grid = self.grid;
        for (j, out) in chunk.iter_mut().enumerate() {
            let at = pos.wrapping_add(j);
            let (mut left, mut right) = (0.0f32, 0.0f32);
            for (i, b) in self.branches.iter().enumerate() {
                let (len, rmask) = (b.recent.len(), b.recent.len() - 1);
                left += b.recent[(at + len - i * grid) & rmask];
                right += b.recent[(at + len - (BRANCHES - 1 - i) * grid) & rmask];
            }
            *out = 0.5 * (left + right);
        }
        self.pos = pos.wrapping_add(n);
    }

    fn update(&mut self) {
        let rate = self.sample_rate;
        let profile = self.profile.map(|p| p.seconds());
        // The within-pass decay follows the 1 kHz band.
        let reference = profile.map_or(self.t60_s, |p| p[5]);
        // Level: the first pass through every branch carries unit
        // energy, as a unit impulse does (the same at every rate).
        let taps: usize = PRIMES.iter().sum();
        let scale = 1.0 / (taps as f32).sqrt();
        for b in &mut self.branches {
            b.gain = loop_gain(b.length as f32, self.t60_s, rate);
            for ((w, &s), &k) in b.weights.iter_mut().zip(&b.signs).zip(&b.taps) {
                *w = s * scale * loop_gain(k as f32, reference, rate);
            }
            if let Some(bands) = profile {
                let target = loop_target(&bands, b.length as f64, rate);
                self.design.fit(&target, &mut b.eq);
            }
        }
    }

    fn longest_t60(&self) -> f32 {
        match self.profile {
            Some(p) => p.longest().get() as f32,
            None => self.t60_s,
        }
    }
}

impl Stage for VelvetNoise {
    /// Allocates the loops and places the impulses for the rate.
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        if sample_rate != self.sample_rate {
            self.design = Design::new(sample_rate);
        }
        self.sample_rate = sample_rate;
        let grid = ((sample_rate / self.density).round() as usize).max(2);
        self.grid = grid;
        let cell = BRANCHES * grid;
        let mut lcg = SEED;
        let mut next = move || {
            lcg = lcg.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            lcg
        };
        for (b, &prime) in self.branches.iter_mut().zip(&PRIMES) {
            b.length = prime * cell;
            // Rings hold a chunk more than they reach back, so a chunk of
            // `process` never overwrites what it still reads.
            b.line = vec![0.0; (b.length + CHUNK + 1).next_power_of_two()];
            b.recent = vec![0.0; (BRANCHES * grid + CHUNK).next_power_of_two()];
            b.taps.clear();
            b.signs.clear();
            for m in 0..prime {
                // An extended velvet-noise impulse: within the first
                // `grid` samples of its cell.
                let r = (next() >> 8) as f32 / (1u32 << 24) as f32;
                b.taps
                    .push(m * cell + (r * (grid - 1) as f32).round() as usize);
                b.signs.push(if next() >> 31 == 0 { 1.0 } else { -1.0 });
            }
            b.weights = vec![0.0; prime];
        }
        self.update();
        self.reset();
    }

    fn reset(&mut self) {
        for b in &mut self.branches {
            b.line.fill(0.0);
            b.recent.fill(0.0);
            b.eq.reset();
        }
        self.pos = 0;
    }

    /// The mid (`(left + right) / 2`) of [`tick_stereo`](VelvetNoise::tick_stereo).
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let (l, r) = self.tick_stereo(x);
        0.5 * (l + r)
    }

    /// Bit-identical to [`tick`](Stage::tick) per sample, a chunk at a
    /// time: each tap then reads a contiguous run of the loop, which
    /// vectorises (the loops are longer than any chunk, so a chunk's
    /// feedback is known before its taps are read).
    fn process(&mut self, block: &mut [f32]) {
        if self.grid == 0 {
            block.fill(0.0);
            return;
        }
        for chunk in block.chunks_mut(CHUNK) {
            self.process_chunk(chunk);
        }
    }

    fn tail(&self) -> Tail {
        let longest = self.branches.iter().map(|b| b.length).max().unwrap_or(0);
        decay_tail(
            self.longest_t60(),
            self.sample_rate,
            (longest + BRANCHES * self.grid) as f32,
        )
    }
}
