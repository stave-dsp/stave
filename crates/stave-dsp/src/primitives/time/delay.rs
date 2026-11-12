//! Variable-length delay line with fractional reads.
//!
//! Storage, not a stage: allocates once in [`DelayLine::allocate`] for
//! the longest delay, never on the audio path. Write the current sample,
//! read any number of samples back (linear or cubic interpolation), then
//! advance. [`Comb`](crate::Comb) and [`DelayAllpass`](crate::DelayAllpass)
//! are the stages built on it.

#[derive(Clone, Debug, Default)]
pub struct DelayLine {
    buf: Vec<f32>,
    write_idx: usize,
    capacity: usize,
}

impl DelayLine {
    pub const fn new() -> Self {
        Self {
            buf: Vec::new(),
            write_idx: 0,
            capacity: 0,
        }
    }

    /// Allocate space for `max_samples` of delay. Calling again resizes
    /// (which allocates — never from the audio thread) and clears.
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
    }

    pub fn reset(&mut self) {
        for s in self.buf.iter_mut() {
            *s = 0.0;
        }
        self.write_idx = 0;
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
