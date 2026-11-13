//! Variable-length delay line with fractional reads, and the [`Delay`]
//! stage built on it.
//!
//! Storage, not a stage: allocates once in [`DelayLine::allocate`] for
//! the longest delay, never on the audio path. Write the current sample,
//! read any number of samples back, then advance. The line's type
//! parameter is its [`Interpolator`] (linear by default):
//! [`interpolated`](DelayLine::interpolated) reads through it, while
//! [`read`](DelayLine::read) (linear), [`read_cubic`](DelayLine::read_cubic)
//! and [`read_int`](DelayLine::read_int) are there on every line.
//! [`Comb`](crate::Comb) and [`DelayAllpass`](crate::DelayAllpass)
//! are the recirculating stages built on it.

use crate::kernel::node::{Stage, Tail};
use crate::primitives::time::interpolators::{Interpolator, Linear, MAX_TAPS};

/// A circular buffer of past samples, read at whole or fractional
/// delays through its interpolator `I`.
///
/// # Models
/// An ideal delay `z⁻ᴰ` for any real `D` up to the allocated length,
/// as good as `I` reconstructs between samples.
///
/// # Derivation
/// A power-of-two ring indexed by mask; fractional reads per `I`.
///
/// # Verified
/// `crates/stave-dsp/src/primitives/time/delay.rs::integer_reads_are_transparent`,
/// `crates/stave-dsp/src/primitives/time/delay.rs::read_block_matches_read` and
/// `crates/stave-dsp/src/primitives/time/delay.rs::cubic_read_beats_linear_where_it_matters`: integer reads are bit-transparent, the block read is
/// the read loop bit for bit, the linear and cubic HF losses are pinned;
/// `interpolators::tests` holds each `I`'s error budget.
///
/// # Cost
/// `DelayLine write+read_cubic` and the `Delay<…>` lines on the
/// datasheet.
///
/// # Aliasing
/// None at a fixed delay; see [`Interpolator`] for modulated reads.
///
/// # Valid ranges
/// Reads from `I::NEWER` to `capacity − 1 − I::OLDER` samples
/// (clamped); `capacity` is `max_samples` rounded up to a power of two.
#[derive(Clone, Debug)]
pub struct DelayLine<I = Linear> {
    buf: Vec<f32>,
    write_idx: usize,
    capacity: usize,
    interpolator: I,
}

impl DelayLine<Linear> {
    /// An empty, linearly interpolated line; [`allocate`](Self::allocate)
    /// sizes it.
    pub const fn new() -> Self {
        Self {
            buf: Vec::new(),
            write_idx: 0,
            capacity: 0,
            interpolator: Linear,
        }
    }
}

impl<I: Interpolator> Default for DelayLine<I> {
    fn default() -> Self {
        Self {
            buf: Vec::new(),
            write_idx: 0,
            capacity: 0,
            interpolator: I::default(),
        }
    }
}

impl<I: Interpolator> DelayLine<I> {
    /// Allocate space for `max_samples` of delay. Calling again resizes
    /// (which allocates — never from the audio thread) and clears. The
    /// interpolator builds its tables here.
    pub fn allocate(&mut self, max_samples: usize) {
        let cap = max_samples.max(2).next_power_of_two();
        if cap != self.capacity {
            self.buf = vec![0.0_f32; cap];
            self.capacity = cap;
        } else {
            for s in self.buf.iter_mut() {
                *s = 0.0;
            }
        }
        self.write_idx = 0;
        self.interpolator.prepare();
        self.interpolator.reset();
    }

    /// The interpolator, for inspection.
    pub fn interpolator(&self) -> &I {
        &self.interpolator
    }

    /// Read `samples` back from the write head through the interpolator
    /// `I`, clamped to `[I::NEWER, capacity − 1 − I::OLDER]`. A stateful
    /// interpolator ([`Thiran`](super::interpolators::Thiran)) keeps its
    /// memory here, so read it once per sample.
    #[inline]
    pub fn interpolated(&mut self, samples: f32) -> f32 {
        let span = I::NEWER + I::OLDER;
        if self.capacity <= span {
            return 0.0;
        }
        let (lo, hi) = (I::NEWER as f32, (self.capacity - 1 - I::OLDER) as f32);
        // A NaN delay reads the shortest (it fails the first compare).
        let s = if samples >= lo { samples.min(hi) } else { lo };
        let whole = s.floor() as usize;
        let frac = s - whole as f32;
        let mask = self.capacity - 1;
        let mut taps = [0.0f32; MAX_TAPS];
        // taps[j] is (whole − NEWER + j) samples old.
        let newest = (self.write_idx + self.capacity - (whole - I::NEWER)) & mask;
        for (j, t) in taps.iter_mut().enumerate().take(span + 1) {
            *t = self.buf[(newest + self.capacity - j) & mask];
        }
        self.interpolator.interpolate(&taps[..=span], frac)
    }

    pub fn reset(&mut self) {
        for s in self.buf.iter_mut() {
            *s = 0.0;
        }
        self.write_idx = 0;
        self.interpolator.reset();
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Write a sample at the current write position (does not advance).
    #[inline]
    pub fn write(&mut self, x: f32) {
        if self.capacity == 0 {
            return;
        }
        self.buf[self.write_idx] = x;
    }

    /// Advance the write head one sample. Call once per sample after
    /// `write` (or interleave write/advance however suits the topology).
    #[inline]
    pub fn advance(&mut self) {
        if self.capacity == 0 {
            return;
        }
        self.write_idx = (self.write_idx + 1) & (self.capacity - 1);
    }

    /// Read `samples` samples back from the write head, linearly
    /// interpolated. `samples` must be in `[0, capacity-1]`. Cheap but
    /// lossy at HF for fractional reads (a frac-0.5 read is a 2-tap
    /// averager: −10 dB at 0.4·fs) — modulated musical delays should
    /// prefer [`read_cubic`](Self::read_cubic).
    #[inline]
    pub fn read(&self, samples: f32) -> f32 {
        if self.capacity == 0 {
            return 0.0;
        }
        let s = samples.clamp(0.0, (self.capacity - 1) as f32);
        let i_floor = s.floor() as usize;
        let frac = s - i_floor as f32;
        let mask = self.capacity - 1;
        let i0 = (self.write_idx + self.capacity - i_floor) & mask;
        let i1 = (i0 + self.capacity - 1) & mask;
        let a = self.buf[i0];
        let b = self.buf[i1];
        a + (b - a) * frac
    }

    /// Read at an INTEGER delay — no interpolation math at all.
    /// For fixed integer-length structures (allpass loops with
    /// rounded lengths) where `read`'s clamp/floor/lerp is pure
    /// overhead. `samples` must be in `[0, capacity-1]`.
    #[inline]
    pub fn read_int(&self, samples: usize) -> f32 {
        if self.capacity == 0 {
            return 0.0;
        }
        let mask = self.capacity - 1;
        self.buf[(self.write_idx + self.capacity - (samples & mask)) & mask]
    }

    /// BLOCK read along an arithmetic delay ramp: `out[i]` is the
    /// linear-interp read at `start + delta * i` samples back from
    /// the CURRENT head. The ring wrap and interpolation setup are
    /// hoisted out of the loop — this is the tap-major primitive
    /// (feed-forward multitap structures walk contiguous regions;
    /// per-call read() paid clamp/floor/mask per sample).
    /// Bit-identical to the equivalent read() loop (the invariant).
    #[inline]
    pub fn read_block(&self, start: f32, delta: f32, out: &mut [f32]) {
        if self.capacity == 0 {
            out.fill(0.0);
            return;
        }
        let mask = self.capacity - 1;
        let max = (self.capacity - 1) as f32;
        // FAST PATH: delta of exactly -1.0 (a steady tap reading a
        // just-written block — steps are exactly 0 when nothing
        // glides) is a contiguous walk with CONSTANT frac: two
        // slice runs, auto-vectorizable. Bit-identical to the
        // general loop (same lerp expression; the invariant covers it).
        if delta == -1.0 && start <= max && start >= 1.0 {
            let i_floor = start.floor() as usize;
            let frac = start - i_floor as f32;
            let n = out.len();
            let mut j0 = (self.write_idx + self.capacity - i_floor) & mask;
            for o in out.iter_mut().take(n) {
                let a = self.buf[j0];
                let b = self.buf[(j0 + self.capacity - 1) & mask];
                *o = a + (b - a) * frac;
                j0 = (j0 + 1) & mask;
            }
            return;
        }
        for (i, o) in out.iter_mut().enumerate() {
            // Multiply form, not accumulation — bit-identical to
            // the per-call loop (the invariant caught the FP drift).
            let s = (start + delta * i as f32).clamp(0.0, max);
            let i_floor = s.floor() as usize;
            let frac = s - i_floor as f32;
            let i0 = (self.write_idx + self.capacity - i_floor) & mask;
            let i1 = (i0 + self.capacity - 1) & mask;
            let a = self.buf[i0];
            let b = self.buf[i1];
            *o = a + (b - a) * frac;
        }
    }

    /// Read `samples` back with 4-point Catmull-Rom interpolation —
    /// the modulated-delay-grade read. Measured frac-0.5
    /// losses (pinned in tests): −0.9 dB vs linear's −3.0 dB at
    /// 0.25·fs; −7.0 vs −10.2 dB at 0.4·fs — the win lives in the
    /// musical band, as 4-point kernels do.
    /// `samples` is clamped to `[1, capacity−2]` for the outer taps.
    #[inline]
    pub fn read_cubic(&self, samples: f32) -> f32 {
        if self.capacity == 0 {
            return 0.0;
        }
        let s = samples.clamp(1.0, (self.capacity - 2) as f32);
        let i_floor = s.floor() as usize;
        let frac = s - i_floor as f32;
        let mask = self.capacity - 1;
        // xm1 is one sample NEWER than x0; x1/x2 progressively older.
        let i0 = (self.write_idx + self.capacity - i_floor) & mask;
        let im1 = (i0 + 1) & mask;
        let i1 = (i0 + self.capacity - 1) & mask;
        let i2 = (i0 + self.capacity - 2) & mask;
        let (xm1, x0, x1, x2) = (self.buf[im1], self.buf[i0], self.buf[i1], self.buf[i2]);
        let c1 = 0.5 * (x1 - xm1);
        let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
        let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
        ((c3 * frac + c2) * frac + c1) * frac + x0
    }
}

/// A delay stage: writes each input and reads it back `ms` later
/// through the interpolator `I` — the voice of a chorus, flanger,
/// vibrato or echo.
///
/// # Models
/// `y[n] = x[n − D]`, `D = ms · sample_rate / 1000`, fractional `D`
/// reconstructed by `I`.
///
/// # Derivation
/// A [`DelayLine<I>`] written, read and advanced once per sample.
///
/// # Verified
/// `crates/stave-dsp/src/primitives/time/delay.rs::delay_stage_reads_back_at_its_setting` (integer delays exact for every
/// interpolator) and `crates/stave-dsp/src/primitives/time/interpolators/tests.rs::modulated_reads_stay_finite`
/// (a delay swept at audio rates, every interpolator).
///
/// # Cost
/// `Delay<Linear>`, `Delay<Lagrange3>`, `Delay<Lagrange5>`,
/// `Delay<Thiran>`, `Delay<Sinc<8>>`, `Delay<Sinc<16>>` on the datasheet.
///
/// # Aliasing
/// None at a fixed delay; see [`Interpolator`] for modulated reads.
///
/// # Valid ranges
/// `0 ≤ ms ≤ max_ms` (clamped); the shortest delay is `I::NEWER`
/// samples. Its delay is the effect, so it reports `latency` 0 and the
/// delay as its tail.
#[derive(Clone, Debug)]
pub struct Delay<I: Interpolator = Linear> {
    line: DelayLine<I>,
    max_ms: f32,
    ms: f32,
    samples: f32,
    sample_rate: f32,
}

impl<I: Interpolator> Delay<I> {
    /// A delay that can reach `max_ms`, set to `ms`. The line is
    /// allocated in `prepare`.
    pub fn new(max_ms: f32, ms: f32) -> Self {
        debug_assert!(max_ms >= 0.0, "max delay {max_ms} ms");
        let mut d = Self {
            line: DelayLine::default(),
            max_ms,
            ms,
            samples: 0.0,
            sample_rate: 48_000.0,
        };
        d.set_ms(ms);
        d
    }

    /// The delay in milliseconds, held inside `[0, max_ms]`. Cheap
    /// enough to call every sample (a modulated delay).
    #[inline]
    pub fn set_ms(&mut self, ms: f32) {
        self.ms = ms.clamp(0.0, self.max_ms);
        self.samples = self.ms * self.sample_rate / 1000.0;
    }

    /// The delay in samples at the prepared rate.
    pub fn samples(&self) -> f32 {
        self.samples
    }
}

impl<I: Interpolator> Stage for Delay<I> {
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.sample_rate = sample_rate;
        let longest = (self.max_ms * 0.001 * sample_rate).ceil() as usize;
        self.line.allocate(longest + I::NEWER + I::OLDER + 2);
        self.set_ms(self.ms);
    }

    fn reset(&mut self) {
        self.line.reset();
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.line.write(x);
        let y = self.line.interpolated(self.samples);
        self.line.advance();
        y
    }

    fn tail(&self) -> Tail {
        Tail::samples(self.samples.ceil() as u32 + I::OLDER as u32 + 1).plus(I::SETTLE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RMS gain of a tone at `rel`·fs read at frac 0.5, vs the input.
    fn frac_read_gain_db(cubic: bool, rel: f64) -> f64 {
        let fs = 48_000.0f64;
        let f = rel * fs;
        let n = 4096;
        let mut dl = DelayLine::new();
        dl.allocate(64);
        let mut sum_in = 0.0f64;
        let mut sum_out = 0.0f64;
        for i in 0..n {
            let x = (core::f64::consts::TAU * f * i as f64 / fs).sin() as f32;
            dl.write(x);
            let y = if cubic {
                dl.read_cubic(4.5)
            } else {
                dl.read(4.5)
            };
            dl.advance();
            if i > 64 {
                sum_in += f64::from(x) * f64::from(x);
                sum_out += f64::from(y) * f64::from(y);
            }
        }
        10.0 * (sum_out / sum_in).log10()
    }

    /// HF interpolation limits — MEASURED 2026-07-04 (frac-0.5 reads):
    ///   0.25·fs (12 kHz @ 48 k): linear −3.0 dB, cubic −0.9 dB
    ///   0.40·fs (19.2 kHz):      linear −10.2 dB, cubic −7.0 dB
    /// Catmull-Rom's win lives in the musical band; near Nyquist a
    /// 4-point kernel can't do much better (the doc's ≤0.5 dB @0.4fs
    /// aspiration needed allpass/sinc — recorded as unrealistic;
    /// revisit only if a modulated delay product demands it).
    #[test]
    fn cubic_read_beats_linear_where_it_matters() {
        let lin_mid = frac_read_gain_db(false, 0.25);
        let cub_mid = frac_read_gain_db(true, 0.25);
        assert!(
            (lin_mid + 3.0).abs() < 0.3,
            "linear @0.25fs moved: {lin_mid:.2}"
        );
        assert!(cub_mid > -1.3, "cubic @0.25fs out of family: {cub_mid:.2}");
        assert!(cub_mid > lin_mid + 1.7, "musical-band win eroded");
        let lin_hf = frac_read_gain_db(false, 0.4);
        let cub_hf = frac_read_gain_db(true, 0.4);
        assert!(
            (lin_hf + 10.2).abs() < 0.4,
            "linear @0.4fs moved: {lin_hf:.2}"
        );
        assert!(
            (cub_hf + 7.0).abs() < 0.4,
            "cubic @0.4fs moved: {cub_hf:.2}"
        );
    }

    /// Integer reads are bit-transparent for both interpolators.
    #[test]
    fn integer_reads_are_transparent() {
        let mut dl = DelayLine::new();
        dl.allocate(32);
        for i in 0..64 {
            let x = (i as f32 * 0.37).sin();
            dl.write(x);
            if i >= 8 {
                assert_eq!(
                    dl.read(8.0).to_bits(),
                    dl.read_cubic(8.0).to_bits(),
                    "sample {i}"
                );
            }
            dl.advance();
        }
    }
}

#[cfg(test)]
mod block_read_properties {
    use super::*;

    /// The block read IS the read loop, bit for bit.
    #[test]
    fn read_block_matches_read() {
        let mut d = DelayLine::new();
        d.allocate(128);
        let mut lcg = 7u32;
        for _ in 0..300 {
            lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
            d.write((lcg >> 8) as f32 / (1u32 << 23) as f32 - 1.0);
            d.advance();
        }
        let mut blk = [0.0f32; 64];
        d.read_block(40.3, -0.97, &mut blk);
        for (i, &b) in blk.iter().enumerate() {
            let r = d.read(40.3 - 0.97 * i as f32);
            assert!(r.to_bits() == b.to_bits(), "i {i}: {r} vs {b}");
        }
    }

    /// Integer reads match read() at integer positions exactly.
    #[test]
    fn read_int_matches_read() {
        let mut d = DelayLine::new();
        d.allocate(64);
        for k in 0..200 {
            d.write(k as f32 * 0.01);
            d.advance();
        }
        for k in [0usize, 1, 7, 33, 63] {
            assert_eq!(d.read_int(k).to_bits(), d.read(k as f32).to_bits());
        }
    }
}

#[cfg(test)]
mod block_fast_path_properties {
    use super::*;

    /// The -1.0 fast path is bit-identical to the general loop.
    #[test]
    fn fast_path_matches_general() {
        let mut d = DelayLine::new();
        d.allocate(256);
        let mut lcg = 99u32;
        for _ in 0..600 {
            lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
            d.write((lcg >> 8) as f32 / (1u32 << 23) as f32 - 1.0);
            d.advance();
        }
        let mut fast = [0.0f32; 128];
        d.read_block(190.7, -1.0, &mut fast);
        for (i, &f) in fast.iter().enumerate() {
            let r = d.read(190.7 - i as f32);
            assert!(r.to_bits() == f.to_bits(), "i {i}: {r} vs {f}");
        }
    }
}

#[cfg(test)]
mod delay_stage_properties {
    use super::*;
    use crate::primitives::time::interpolators::{Lagrange3, Lagrange5, Sinc, Thiran};

    fn integer_delay_is_exact<I: Interpolator>() {
        let mut d = Delay::<I>::new(10.0, 0.25); // 12 samples at 48 kHz
        d.prepare(48_000.0);
        assert_eq!(d.samples(), 12.0);
        let x: Vec<f32> = (0..200).map(|i| (i as f32 * 0.37).sin()).collect();
        let y: Vec<f32> = x.iter().map(|&v| d.tick(v)).collect();
        for n in 12..200 {
            assert!((y[n] - x[n - 12]).abs() < 1e-5, "{n}: {} vs {}", y[n], x[n - 12]);
        }
    }

    /// Every interpolator reads an integer delay back exactly.
    #[test]
    fn delay_stage_reads_back_at_its_setting() {
        integer_delay_is_exact::<Linear>();
        integer_delay_is_exact::<Lagrange3>();
        integer_delay_is_exact::<Lagrange5>();
        integer_delay_is_exact::<Thiran>();
        integer_delay_is_exact::<Sinc<8>>();
        integer_delay_is_exact::<Sinc<16>>();
    }

    /// The linear line's `interpolated` is its `read`, bit for bit.
    #[test]
    fn linear_interpolated_is_read() {
        let mut d = DelayLine::new();
        d.allocate(64);
        for k in 0..100 {
            d.write((k as f32 * 0.11).cos());
            d.advance();
        }
        for s in [0.0f32, 0.3, 5.5, 17.25, 61.9] {
            assert_eq!(d.interpolated(s).to_bits(), d.read(s).to_bits(), "{s}");
        }
    }
}
